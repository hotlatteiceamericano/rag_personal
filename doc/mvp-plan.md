# MVP Plan — 7-Day Schedule


This is the day-by-day delivery schedule for the one-week MVP. Each day lists a
**deliverable**, **acceptance criteria** (how you know it's done), and a
**risk/note**. Days are sized for ~½–1 focused day each, leaving Day 7 as a
buffer.

Tick the boxes as you go: `[ ]` = todo, `[x]` = done.

---

## Assumptions & current state

**Done already**
- [x] `cargo` project (Rust 2024) with `tokio`, `reqwest`, `serde`, `serde_json`,
  `anyhow`.
- [x] `src/main.rs` successfully calls
  `GET /v1/blocks/{id}/children?page_size=100` and prints the JSON response.
- [x] `.env` holds `NOTION_TOKEN`.

**Fixed stack** (do not revisit during MVP): fastembed-rs
`intfloat/multilingual-e5-small` (dense) **+ `tantivy` BM25 (lexical), fused
with RRF**, LanceDB embedded, `rmcp` over stdio, single binary.

---

## Day 1 — Project skeleton + Notion client

**Deliverable**
- [ ] Add deps: `clap`, `fastembed`, `lancedb`, `arrow`, `rmcp`, `thiserror`,
  `tantivy`, `tantivy-jieba` (lexical/BM25 + CJK tokenizer), re-enable
  `dotenvy`. Pin versions.
- [x] Restructure into the module layout from design §5 (empty traits + stubs).
- [x] `clap` CLI dispatch with subcommands: `ingest`.
- [ ] `config.rs`: load `.env` + defaults (root page ids, db path, model, top_k).
- [ ] `source/notion.rs`: move the working call into a `Source` impl with
  **pagination** (`has_more` / `next_cursor`) and **recursion** into
  `has_children` blocks (visited-set + max depth).

**Acceptance criteria**
- [ ] `cargo run -- ingest` reaches the Notion source and prints the *count* of
  blocks fetched recursively from the root page (more than the flat
  `page_size=100` you get today).
- [ ] `cargo run -- --help` shows all four subcommands.

**Risk/note:** Notion ~3 req/s — add backoff on 429/5xx now, not later.

---

## Day 2 — Text extraction + chunking

**Deliverable**
- [ ] Block → plain text for: `paragraph`, `heading_1..3`, `bulleted_list_item`,
  `numbered_list_item`, `to_do`, `toggle`, `quote`, `callout`, `code`.
  Build `SourceDoc { page_id, title, url, blocks[] }` where each `TextBlock`
  carries an `is_heading` flag so the chunker can use heading boundaries as
  soft splits. (Full `heading_path` citation context is deferred to Phase 2.)
- [ ] `chunk/structure.rs`: structure-aware greedy packing, target ≤384 tokens,
  ~15% overlap, never split inside `code`, char-split oversized blocks.
- [ ] Stable `chunk_id = {page_id}#{ordinal}`; metadata carried onto each chunk.

**Acceptance criteria**
- [ ] `cargo run -- ingest` prints N documents → M chunks with size stats
  (min/median/max chars) and no chunk exceeds the cap.
- [ ] Spot-check: a known note's text appears intact across its chunks; Chinese
  text is not mangled (UTF-8 boundaries respected).

**Risk/note:** char-based token approximation only — acceptable for MVP, flag
as P2 upgrade.

---

## Day 3 — Embedding + LanceDB store

**Deliverable**
- [ ] `embed/fastembed.rs`: `E5SmallEmbedder` with **`passage:` / `query:`
  prefixes** centralized here; L2-normalize output; batch embedding.
- [ ] `store/lancedb.rs`: create/open table with the design §4.4 schema; `upsert`
  (delete-by-`page_id` then insert); `search(vec, k)` cosine.
- [ ] `store/memory.rs`: in-memory impl for tests.

**Acceptance criteria**
- [ ] First run logs the model download; subsequent runs use cache.
- [ ] After `ingest`, `./data/lancedb` exists and a quick count query returns the
  expected number of chunk rows.
- [ ] Unit test asserts the embedder applies the correct prefix and returns
  384-dim, L2-normalized vectors.

**Risk/note:** pin `lancedb`/`arrow` versions; isolate all Arrow code here.

---

## Day 4 — Hybrid retrieval + end-to-end `ingest`/`query`

**Deliverable**
- [ ] `lexical/tantivy.rs`: `LexicalIndex` (BM25) over chunk `text`, built during
  ingest, using a **CJK-aware tokenizer** (`tantivy-jieba`) so Chinese
  segments into terms.
- [ ] `retrieve.rs`: `HybridRetriever` = dense (`query:` prefix → vector search)
  **+** lexical (BM25) **+ RRF fusion** (`k ≈ 60`) →
  `Hit { text, title, url, score }`. Expose a
  `RetrievalMode { Dense, Lexical, Hybrid }` selector and a stable `chunk_id`
  tiebreak so the eval can ablate the three legs reproducibly (design §8.1).
- [ ] `pipeline.rs`: wire full `ingest` (Notion → chunk → embed → **vector store +
  lexical index**).
- [ ] `query "<text>"` subcommand prints fused ranked hits with scores +
  citations.

**Acceptance criteria**
- [ ] Full `cargo run -- ingest` populates **both** the vector store and the
  lexical index; re-running does **not** duplicate rows (idempotent).
- [ ] `cargo run -- query "<English question>"` **and** `query "<中文问题>"` each
  return the expected note in the top results (proves both legs work across
  languages).
- [ ] A keyword-only query (exact proper noun / code identifier) ranks the right
  chunk via the lexical leg even where dense alone would miss it.

**Risk/note:** the heaviest day — hybrid adds the lexical index + CJK
tokenizer + fusion on top of integration. **Fallback if overrunning:** ship
*vector-only* retrieval for the Day-5 OpenClaw demo, then finish the lexical
leg + RRF before Day 6 so Recall@5 measures the hybrid system (hybrid is an
MVP requirement, so it lands before the metric, not after).

---

## Day 5 — MCP server + OpenClaw integration

**Deliverable**
- [ ] `mcp/server.rs`: `rmcp` stdio server exposing `search_notes(query, top_k)`
  → JSON list of hits (reuses `Retriever`, read-only).
- [ ] `serve-mcp` subcommand runs it.
- [ ] Register with OpenClaw and verify a real agent call.

**Acceptance criteria**
- [ ] `openclaw mcp set notion-rag '{"command":"<abs path>","args":["serve-mcp"],
  "env":{...}}'` then `openclaw mcp list` shows the server.
- [ ] Asking OpenClaw a question about your notes triggers `search_notes` and the
  answer reflects retrieved content.

**Risk/note:** stdio server must keep stdout clean for the MCP protocol — send
all logs to stderr.

---

## Day 6 — Recall@5 eval harness

**Deliverable**
- [ ] Hand-author `eval/gold.jsonl` (~20–40 Q), each entry
  `{q, relevant_page_ids, answer_span}` where `answer_span` is the
  answer-bearing sentence copied from the note (design §8.1).
- [ ] `eval/recall.rs`: for each Q, top-5 retrieve; **hit iff** a top-5 chunk is
  from a relevant page **and** its text contains/overlaps `answer_span`
  (case-insensitive substring for MVP). Entries with no `answer_span` fall
  back to page-level relevance. Run the gold set through **all three
  `RetrievalMode`s** (Dense, Lexical, Hybrid).
- [ ] Timestamped report under `eval_runs/`: a **3-row Recall@5 table**
  (dense | lexical | hybrid), the recorded fusion params (`N`, RRF `k`), the
  count of fallback entries, and per-query pass/fail per mode.

**Acceptance criteria**
- [ ] `cargo run -- eval recall` outputs the **dense / lexical / hybrid** Recall@5
  numbers (not one number) plus a miss table, so a hybrid regression vs dense
  is visible.
- [ ] Re-running is deterministic for the same index (stable tiebreak).

**Risk/note:** keep questions natural and answerable from one known page;
small set is noisy — that's expected, note it in the report.

---

## Day 7 — LLM-as-judge faithfulness + buffer/polish

**Deliverable**
- [ ] `eval/faithfulness.rs`: per Q produce no-RAG and RAG answers using the
  **answerer model from `config.toml`** (default = the model OpenClaw uses,
  swappable without code change); a fixed judge prompt (temp 0, ideally a
  different model) scores faithfulness 1–5 with rationale, given retrieved
  notes as source of truth. Report mean RAG vs no-RAG and the delta.
- [ ] README quickstart (ingest → serve-mcp → register → eval).
- [ ] Buffer: absorb slippage from Days 1–6; polish logging/errors.

**Acceptance criteria**
- [ ] `cargo run -- eval faithfulness` prints mean faithfulness for RAG and
  no-RAG and the **delta**, with rationales logged to `eval_runs/`.
- [ ] A new user can follow the README from clone to a working OpenClaw tool.

**Risk/note:** if Day 4 slipped, faithfulness eval is the first thing to defer
to post-MVP — Recall@5 (Day 6) is the higher-priority metric.

---

## Daily priority order (if time is short)

If the week compresses, protect deliverables in this order:

1. Day 4 — end-to-end **hybrid** ingest + query (the core pipeline;
   vector-only is the acceptable fallback to unblock Day 5, with hybrid
   completed before Day 6).
2. Day 5 — MCP server + OpenClaw (the actual goal: usable by the agent).
3. Day 6 — Recall@5 (objective retrieval signal).
4. Day 7 — faithfulness (nice-to-have for MVP, schedule into Phase 2 if cut).

---

## Post-MVP backlog (Phase 2+)

Pulled from design §9 — do **not** start these during the week:

- [ ] `heading_path` citation context — propagate the enclosing-heading stack from source → chunk → store schema → `Hit` → `search_notes` output so answers can cite "Page › Section".
- [ ] Incremental sync via Notion `last_edited_time`.
- [ ] Token-exact / semantic chunking.
- [ ] LanceDB ANN index (IVF_PQ).
- [ ] Cross-encoder reranker over the fused top-N.
- [ ] Simple MCP auth (shared bearer token) — only once a remote transport is used.
- [ ] Additional sources (Markdown/Obsidian export).
- [ ] Remote MCP transport (SSE / streamable-HTTP).
- [ ] Richer Notion blocks (tables, databases, image OCR).
- [ ] Continuous eval with Recall@5 regression gates.
