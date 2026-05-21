# Design Doc — Personal Notion RAG Pipeline (Rust)


## 1. Overview & Goals

Build a single-binary Rust application that ingests the author's personal
Notion notes, embeds them, stores them in a local vector database, and exposes
semantic retrieval as an **MCP server** consumed by the author's personal AI
agent, **OpenClaw**.

Everything runs in one process / one machine — no separate vector DB host, no
remote services beyond the Notion API and (for evaluation only) an LLM judge.

### Primary goals

1. **Ingest** Notion notes (English + Chinese) into a retrievable index.
2. **Serve** retrieval to OpenClaw over MCP so the agent can ground answers in
   the author's notes.
3. **Evaluate** quality with two metrics:
   - **Recall@5** — retrieval accuracy: is a relevant chunk in the top-5?
   - **LLM-as-judge faithfulness** — end-to-end value: does RAG context make
     the agent's answers more grounded vs. no-RAG?

### Goals

- `cargo run -- ingest` populates a local LanceDB from a Notion root page.
- `cargo run -- serve-mcp` exposes a working `search_notes` tool that OpenClaw
  can call over stdio.
- `cargo run -- eval` reports Recall@5 and a faithfulness score on a small
  hand-built gold set.

---

## 2. Extensions

- Incremental / delta sync (full re-ingest each run is acceptable for MVP).
- No multi-user, auth, or hosted deployment.
- No cross-encoder **reranking** model (deferred — see §9). *Hybrid* lexical +
  vector retrieval **is in scope for the MVP** — see §4.5.
- No UI — the only consumers are the CLI and OpenClaw via MCP.
- No support for Notion databases-as-tables, files, or images (text blocks
  only). Page discovery starts from one or more configured root page IDs.

These are explicitly deferred to [§9 Extensibility](#9-extensibility--future-phases).

---

## 3. High-Level Architecture

```
                       ┌────────────────────────────────────────────────┐
                       │            rag_personal (1 binary)             │
                       │                                                │
  Notion API  ──────►  │  Source ──► Chunker ──► Embedder ──► Store     │
  (REST, token)        │  (notion)   (split)    (fastembed)  (LanceDB)  │
                       │                            │           ▲       │
                       │                            │           │       │
                       │   Retriever ◄──────────────┘           │       │
                       │      ▲   │ (embed query, top-k search) │       │
                       │      │   └──────────────────────────────┘      │
                       │      │                                         │
                       │  ┌───┴─────────────┐     ┌──────────────────┐  │
                       │  │  MCP Server      │     │  Eval Harness    │  │
                       │  │  (stdio, rmcp)   │     │  Recall@5 +      │  │
                       │  │  search_notes()  │     │  LLM-as-judge    │  │
                       │  └───────┬──────────┘     └──────────────────┘  │
                       └──────────┼──────────────────────────────────────┘
                                  │ stdio (MCP)
                                  ▼
                            ┌───────────┐
                            │  OpenClaw │  ──► chat surfaces (Telegram, etc.)
                            └───────────┘
```

### Data flow

**Ingestion (offline, `ingest` command)**

1. Walk Notion from configured root page ID(s), recursing into child blocks.
2. Extract plain text per block; assemble per-page documents with metadata.
3. Chunk documents (structure-aware, bounded to the embedder's token limit).
4. Embed each chunk with the `passage:` prefix.
5. Upsert `{id, text, vector, metadata}` rows into LanceDB.

**Retrieval (online, used by `serve-mcp` and `eval`)**

1. Embed the query with the `query:` prefix.
2. Vector search LanceDB for top-k nearest chunks (cosine).
3. Return chunk text + source metadata (page title, Notion URL).

---

## 4. Component Design

### 4.1 Notion Source

**Responsibility:** turn a Notion workspace subtree into a list of in-memory
documents.

- Notion publishes **no official Rust SDK**. We keep the already-working
  hand-rolled `reqwest` client. `notion-client` (community crate) is a
  fallback if hand-rolling becomes costly — kept behind our own trait so the
  choice is reversible.
- API: `GET /v1/blocks/{block_id}/children?page_size=100`,
  header `Notion-Version: 2022-06-28`, `Authorization: Bearer <token>`.
- **Pagination:** loop while `has_more == true`, passing `start_cursor =
  next_cursor`.
- **Recursion:** any block with `has_children == true` is fetched again with
  its own block id as parent. Guard against cycles with a visited-set and a
  max-depth cap.
- **Rate limits:** Notion allows ~3 requests/sec. Add a small concurrency cap
  and retry-with-backoff on HTTP 429 / 5xx.
- **Text extraction:** map block types to plain text by concatenating
  `rich_text[].plain_text`. MVP supports: `paragraph`, `heading_1..3`,
  `bulleted_list_item`, `numbered_list_item`, `to_do`, `toggle`, `quote`,
  `callout`, `code`. Unknown block types are skipped (logged), not fatal.

**Output type (conceptual):**

```rust
struct SourceDoc {
    page_id: String,
    title: String,
    url: String,            // Notion page URL for citation
    blocks: Vec<TextBlock>, // ordered
}
struct TextBlock { text: String, is_heading: bool }
```

Heading blocks (`heading_1..3`) are flagged on `TextBlock` so the chunker can
treat them as soft boundaries (see §4.2). The richer **heading_path**
(stack of enclosing headings, for "Page › Section" citation) is deferred to
Phase 2 — see §9.

### 4.2 Chunking

**Responsibility:** split a `SourceDoc` into embeddable chunks that respect the
embedder's input limit and keep semantic coherence.

- `intfloat/multilingual-e5-small` has a **512-token** max sequence. We target
  **~256–384 tokens** per chunk with **~15% overlap** to preserve context
  across boundaries.
- Strategy (MVP): **structure-aware greedy packing**
  1. Treat heading blocks (`heading_1..3`) as soft boundaries — prefer to emit
     a chunk *before* crossing into the next section when the running chunk is
     non-trivial.
  2. Greedily concatenate blocks until the next block would exceed the target
     size; emit a chunk; carry an overlap tail into the next chunk.
  3. Never split inside a code block; oversized single blocks are hard-split on
     character boundaries as a fallback.
- Token counting: a cheap approximation for MVP (chars/≈3 for CJK-heavy text,
  chars/≈4 for Latin). Exact tokenization is a Phase-2 refinement.
- Each chunk carries forward metadata: `page_id`, `title`, `url`, and a stable
  `chunk_id` (`{page_id}#{ordinal}`). Section-level citation context
  (`heading_path`) is deferred to Phase 2 — see §9.

The `Chunker` is a trait so we can later swap in token-exact or
semantic/recursive splitters without touching the rest of the pipeline.

### 4.3 Embedding

**Responsibility:** turn text into 384-dim vectors.

- Library: **`fastembed`** (fastembed-rs).
- Model: **`intfloat/multilingual-e5-small`** — chosen because notes mix
  English and Chinese; this model is multilingual and small/fast for local use.
- **Critical detail — E5 prefixes.** This model family is trained with
  asymmetric prefixes and *will underperform without them*:
  - Indexing a chunk: embed `"passage: " + chunk_text`
  - Embedding a query: embed `"query: " + query_text`
- Output: 384-dim vectors. We **L2-normalize** so cosine similarity == dot
  product, simplifying the vector store distance config.
- Model weights download on first run and are cached; document this so the
  first `ingest` isn't mistaken for a hang.
- Batch embed chunks (fastembed supports batching) for throughput.

The `Embedder` trait exposes `embed_passages(&[String])` and
`embed_query(&str)` so the prefix logic lives in one place and the model is
swappable.

### 4.4 Vector Store (LanceDB)

**Responsibility:** persist chunks + vectors and answer top-k queries, all
embedded in-process from a local directory.

- Library: **`lancedb`** (+ `arrow`), pointed at a local path (e.g.
  `./data/lancedb`). No server process — satisfies the "run everything at
  once" requirement.
- **Schema** (one Arrow table, e.g. `notes`):

  | column        | type                | notes                          |
  |---------------|---------------------|--------------------------------|
  | `chunk_id`    | Utf8 (primary)      | `{page_id}#{ordinal}`          |
  | `page_id`     | Utf8                | for delete/replace by page     |
  | `title`       | Utf8                | citation                       |
  | `url`         | Utf8                | citation                       |
  | `text`        | Utf8                | raw chunk text (no prefix)     |
  | `vector`      | FixedSizeList<f32,384> | L2-normalized embedding     |

  (A `heading_path` column for section-level citation is deferred to Phase 2 —
  see §9.)

- **Distance:** cosine (or dot, since vectors are normalized).
- **Indexing:** MVP uses brute-force / flat scan — fine for the expected
  thousands of chunks. An ANN index (IVF_PQ) is a Phase-2 toggle once the
  corpus grows.
- **Upsert semantics:** MVP does *delete-by-`page_id` then insert* to make
  re-ingest idempotent without true delta sync.

`VectorStore` trait: `upsert(rows)`, `search(query_vec, k) -> Vec<Hit>`,
`delete_page(page_id)`. LanceDB is one impl; an in-memory impl is handy for
fast unit tests.

### 4.5 Hybrid Retrieval (dense + lexical)

**Responsibility:** glue embedding, the vector store, and a lexical index into
a single query call shared by the MCP server and the eval harness. **Hybrid is
an MVP requirement, not a later phase.**

Two retrievers run per query and their ranked lists are fused:

- **Dense (semantic):** `Embedder::embed_query` (`query:` prefix) →
  `VectorStore::search` → top-N by cosine. Catches paraphrase and cross-lingual
  matches (an English question hitting a Chinese note).
- **Lexical (keyword, BM25):** a `LexicalIndex` over the raw chunk `text` →
  top-N by BM25. Catches exact terms the embedding blurs: proper nouns, code
  identifiers, ids, rare Chinese terms, exact phrases.
- **Fusion:** **Reciprocal Rank Fusion (RRF)**,
  `score(d) = Σ_i 1/(k + rank_i(d))` with `k ≈ 60`. RRF needs no score
  normalization across the two very different score scales, is parameter-light,
  and is robust — the right default for MVP. The fused top-k is returned.

```
retrieve(query, k):
    d     = VectorStore.search(embed_query(query), N)   # dense ranked list
    l     = LexicalIndex.search(query, N)                # BM25 ranked list
    fused = rrf_merge(d, l, k_rrf = 60)
    return fused[..k] -> { text, title, url, score }
```

- **Lexical index choice:** `tantivy` (embedded, file-based, no server — fits
  the single-process constraint) with a **CJK-aware tokenizer**
  (`tantivy-jieba`) so Chinese — which has no whitespace — is segmented into
  terms instead of one giant token. Fallback: a small in-memory BM25 +
  `jieba-rs` if Tantivy integration proves heavy. Both sit behind a
  `LexicalIndex` trait so the choice is reversible.
- The lexical index is built during `ingest` from the **same chunks** as the
  vector store, so the two legs stay consistent.
- **No cross-encoder reranking in MVP** — that is a distinct stage-2 step on
  top of fusion, deferred to §9. Returning citation metadata lets OpenClaw and
  the faithfulness judge attribute claims to source pages.

### 4.6 MCP Server

**Responsibility:** expose retrieval to OpenClaw as an MCP tool.

- Library: **`rmcp`** (the official Rust MCP SDK).
- **Transport: stdio.** OpenClaw spawns our binary as a child process and
  speaks MCP over stdin/stdout. This is the simplest, no-port, no-auth option
  and matches OpenClaw's stdio transport.
- **Tool exposed:**

  ```
  name: search_notes
  description: Search the user's personal Notion notes for relevant passages.
  input:  { "query": string, "top_k": integer (default 5, max 20) }
  output: list of { text, title, url, score }
  ```

- **OpenClaw registration** (run by the user once the binary builds):

  ```bash
  openclaw mcp set notion-rag \
    '{"command":"/abs/path/to/rag_personal","args":["serve-mcp"],
      "env":{"NOTION_TOKEN":"...", "RAG_DB_PATH":"/abs/path/data/lancedb"}}'
  openclaw mcp list      # verify it shows up
  ```

  OpenClaw also supports SSE / streamable-HTTP transports; those are a
  later-phase option if remote access is ever needed. For MVP, stdio only.

- **Auth (future phase, not built in MVP).** stdio needs none — only a local
  process that can already exec the binary and read the DB can use the server,
  and OpenClaw "registration" is just local config, not a network endpoint.
  Auth becomes relevant only with a remote transport: gate it with a shared
  bearer token — the server reads `RAG_MCP_TOKEN` and rejects any request whose
  `Authorization: Bearer …` header doesn't match; OpenClaw is then registered
  with `{"url":"…","headers":{"Authorization":"Bearer <token>"}}`, bound to
  localhost. Tracked in §9.

- The MCP server **only reads** the store; ingestion stays a separate `ingest`
  command so a long crawl never blocks the agent.

---

## 5. Trait / Module Layout

Designed so every external dependency sits behind a trait — the MVP picks one
implementation, future phases swap freely.

```
src/
  main.rs          // CLI dispatch (clap)
  config.rs        // env + file config, secrets
  source/
    mod.rs         // trait Source { fn fetch(&self) -> Vec<SourceDoc> }
    notion.rs      // NotionSource (reqwest)
  chunk/
    mod.rs         // trait Chunker { fn chunk(&self, doc) -> Vec<Chunk> }
    structure.rs   // StructureChunker (MVP)
  embed/
    mod.rs         // trait Embedder { embed_passages / embed_query }
    fastembed.rs   // E5SmallEmbedder (prefix logic here)
  store/
    mod.rs         // trait VectorStore { upsert / search / delete_page }
    lancedb.rs     // LanceStore (dense vectors)
    memory.rs      // InMemoryStore (tests)
  lexical/
    mod.rs         // trait LexicalIndex { index(chunks) / search(query) }
    tantivy.rs     // TantivyIndex (BM25, tantivy-jieba CJK tokenizer)
  retrieve.rs      // HybridRetriever + RetrievalMode {Dense,Lexical,Hybrid};
                   //   RRF fusion, stable chunk_id tiebreak (reproducible eval)
  mcp/
    server.rs      // rmcp stdio server, search_notes tool
  eval/
    mod.rs         // gold-set loading, metrics
    recall.rs      // Recall@5
    faithfulness.rs// LLM-as-judge
  pipeline.rs      // ingest() orchestration
```

**Key principle:** the *data contracts* (`SourceDoc`, `Chunk`, `Hit`) are
stable; implementations behind traits are not. This is what makes the future
phases cheap.

---

## 6. CLI Surface

`clap`-based subcommands:

| Command              | Purpose                                                    |
|----------------------|------------------------------------------------------------|
| `ingest`             | Notion → chunk → embed → LanceDB (full re-ingest).         |
| `query "<text>"`     | Debug: print top-k hits for a query (no MCP).              |
| `serve-mcp`          | Run the stdio MCP server for OpenClaw.                     |
| `eval recall`        | Recall@5 × {dense, lexical, hybrid} against the gold set.  |
| `eval faithfulness`  | RAG vs no-RAG faithfulness via LLM judge.                  |

`query` exists purely so retrieval can be sanity-checked independently of
OpenClaw — important for fast iteration during the week.

---

## 7. Configuration & Secrets

- Secrets via environment (`.env` already present): `NOTION_TOKEN`, and for
  evaluation an LLM key (e.g. `OPENAI_API_KEY` / `ANTHROPIC_API_KEY`).
- Non-secret config in a small `config.toml` with env overrides; sane
  defaults so MVP runs with near-zero config:
  - `root_page_ids` — a **list** of Notion page ids to crawl (each crawled
    recursively); default = the id currently in `main.rs`.
  - `db_path`, `chunk_target_tokens`, `chunk_overlap`, `top_k`.
  - `model` / provider + api key for the eval answerer & judge (defaults to
    OpenClaw's model; swappable here, no code change).
- Re-enable `dotenvy` (currently commented in `Cargo.toml`) so `.env` loads
  automatically instead of relying on the shell.
- `.env`, `data/`, model cache are git-ignored.

---

## 8. Evaluation

Two metrics, deliberately separating *retrieval quality* from *end-to-end
answer quality*.

### 8.1 Recall@5 (retrieval accuracy)

- **Gold set:** a hand-authored JSONL file, ~20–40 entries. Relevance is
  **chunk-level, identified by an answer snippet** (not by chunk id):

  ```json
  {"q": "What did I decide about the LanceDB schema?",
   "relevant_page_ids": ["<notion_page_id>"],
   "answer_span": "one row, columns chunk_id page_id title url ... vector"}
  ```

  `answer_span` is the exact answer-bearing sentence/phrase copied from the
  note. This is preferred over chunk ids (which shift whenever chunking is
  tuned) and over page-only relevance (which over-counts: any 1 of a long
  page's many chunks would falsely score as a hit — significant because the
  e5-small 512-token limit splits long pages into many chunks).
  Authoring tip: while reading a note to write the question, copy the sentence
  that answers it — near-zero extra effort.

- **Metric:** for each query, retrieve top-5; it counts as a hit if **any**
  returned chunk is from a `relevant_page_id` **and** its text contains /
  substantially overlaps `answer_span` (case-insensitive substring for MVP;
  normalized token overlap as a refinement).
  `Recall@5 = hits / total_queries`.
  - **Fallback:** if `answer_span` authoring runs short on Day 6, an entry may
    omit it and fall back to page-level relevance (hit iff a top-5 chunk's
    page is relevant). Report how many entries used the looser rule.
  - If multiple relevant pages exist per query, also report the stricter
    per-query recall = relevant items found in top-5 / total relevant,
    averaged. MVP reports the simple hit-rate; the stricter form is a one-line
    extension.

- **Report — three numbers, one gold set.** Recall@5 is computed
  independently for **dense-only** (vector search), **lexical-only** (BM25),
  and **hybrid** (RRF-fused), by running the *same* gold set through each
  `RetrievalMode` (see §5). The report is a 3-row table; the headline is the
  hybrid row, the other two show which leg earns the score and whether fusion
  ever *regresses* vs. dense alone.
  - Record the fusion params used (`N` candidates per leg, RRF `k`) in the
    report — runs aren't comparable otherwise. Do **not** tune them against
    this gold set (20–40 Q ⇒ overfitting); fix from priors (`k≈60`).
  - **Caveat:** the hit rule is lexical (chunk contains `answer_span`), which
    is sympathetic to the BM25 leg. A hybrid gain that does **not** also show
    up in §8.2 faithfulness may be this artifact, not a real end-task win —
    read the two metrics together.

- **Why this metric:** it isolates retrieval quality (chunking + dense
  embedding + lexical index + fusion) *before* any answering LLM is involved —
  the part of the system we control most directly. The dense/lexical/hybrid
  split further isolates *which* retrieval component is responsible.

### 8.2 LLM-as-judge faithfulness (end-to-end)

- **What it measures:** whether answers are *grounded in the retrieved notes*
  (no hallucination / fabrication), and whether RAG improves this vs. a no-RAG
  baseline.

- **Procedure** per gold question:
  1. **No-RAG answer:** ask the answering LLM the question with no context.
  2. **RAG answer:** retrieve top-5, inject as context, ask the same question.
  3. **Judge:** a separate LLM-judge prompt scores each answer's faithfulness
     on a 1–5 scale, *given the retrieved notes as the source of truth*, plus a
     short rationale. The judge sees the question, the candidate answer, and
     the retrieved passages; it is instructed to penalize unsupported claims.
  4. Report mean faithfulness for RAG vs. no-RAG, and the delta.

- **Answerer model:** defaults to the same model OpenClaw is configured with,
  so the eval reflects the agent the author actually uses. The model is **not
  hard-coded** — it is a `config.toml` field (`model` / provider + api key) so
  it can be swapped without code changes.
- **Controls / honesty:** fixed judge prompt and temperature 0; the judge
  should be a *different* model from the answerer where possible. If budget
  forces the same model for both, mitigate with a strict scoring rubric +
  temperature 0, and treat only the **RAG − no-RAG delta** (not the absolute
  score) as the headline result. Rationales are logged so scores are
  auditable.

- Both eval commands write a timestamped report (JSON + human summary) under
  `eval_runs/` so progress across the week is comparable.

---

## 9. Extensibility / Future Phases

The MVP intentionally hard-codes the simple choice behind each trait. Future
phases plug in without reworking the data flow:

Tick the boxes as phases land: `[ ]` = todo, `[x]` = done. *Italic* = touch
points (the modules a phase changes).

### Phase 2
- [ ] **`heading_path` citation context** — track the stack of enclosing headings on each `TextBlock`, propagate onto `Chunk`, persist as a `heading_path` Utf8 column in LanceDB (joined with ` › `), and surface it on `Hit` and the `search_notes` MCP output so answers can cite "Page › Section" — *source/notion.rs, chunk/, store/lancedb.rs, retrieve.rs, mcp/server.rs*
- [ ] **Incremental sync** — use Notion `last_edited_time`, delete+reinsert only changed pages — *Source, pipeline*
- [ ] **Token-exact chunking** + semantic/recursive splitter — *Chunker only*
- [ ] **ANN index** (LanceDB IVF_PQ) when corpus is large — *store/lancedb.rs*

### Phase 3
- [ ] **Cross-encoder reranker** over the fused top-N (fastembed supports rerankers) — *retrieve.rs*
- [ ] **Simple MCP auth** — shared bearer token validated server-side; only needed once a remote (SSE/HTTP) transport is used (stdio MVP needs none) — *mcp/, config*
- [ ] **More sources** (Markdown export, Obsidian, Google Docs) — *new Source impls*
- [ ] **Remote MCP** (SSE / streamable-HTTP) for multi-device OpenClaw — *mcp/ only*
- [ ] **Richer Notion blocks** (tables, databases, image OCR) — *source/notion.rs*

### Phase 4
- [ ] **Continuous eval** in CI on each ingest; regression gates on Recall@5 — *eval/*

Because `SourceDoc` / `Chunk` / `Hit` are the only cross-module contracts, each
row above is a localized change.

---

## 10. Risks & Resolved Decisions

| Risk | Mitigation |
|------|------------|
| Forgetting E5 `query:`/`passage:` prefixes silently tanks quality | Prefix logic centralized in `Embedder`; covered by a unit test asserting prefixes are applied |
| First-run model download looks like a hang | Log "downloading model…" + size; document in README |
| Notion rate limits on large workspaces | Backoff + concurrency cap; MVP scoped to a root page subtree |
| LanceDB / Arrow API churn between versions | Pin versions in `Cargo.toml`; isolate in `store/lancedb.rs` |
| Small/biased gold set makes metrics noisy | Treat absolute numbers cautiously; emphasize RAG−noRAG delta; grow gold set in P4 |
| Token approximation under/over-fills chunks | Conservative target (≤384 of 512); upgrade to exact tokenizer in P2 |
| Chinese has no whitespace → naive BM25 tokenizer kills lexical recall | CJK-aware tokenizer (`tantivy-jieba` / `jieba-rs`); EN+ZH retrieval smoke test |
| Hybrid adds scope to a 1-week MVP | Day-4 fallback: ship vector-only for the Day-5 OpenClaw demo, finish hybrid before Day 6 so Recall@5 measures the hybrid system |
| Hybrid can *regress* vs dense (fusion dilution) | The dense/lexical/hybrid Recall@5 ablation (§8.1) makes a regression visible; stable `chunk_id` tiebreak keeps the three runs reproducible |

**Resolved decisions (2026-05-19):**

1. **Crawl roots:** a configurable **list** of root page ids
   (`config.toml: root_page_ids`), each crawled recursively. Default = the id
   currently in `main.rs`.
2. **Eval answerer:** the **same model OpenClaw uses**, set via `config.toml`
   so it is swappable without code changes (author is open to changing it).
   Judge ideally a different model — see §8.2 controls.
3. **Recall@5 relevance:** **chunk-level via `answer_span` snippet** (see
   §8.1), with page-level relevance as a documented fallback if gold-set
   authoring runs short on Day 6.
