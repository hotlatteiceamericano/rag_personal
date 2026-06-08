# rag_personal

A single-binary Rust pipeline that ingests Notion notes, embeds them with a
multilingual model, and stores the results in a local LanceDB for retrieval.

It exposes three task commands:

- **`ingest`** — crawl your Notion workspace and build the local index. Prompts
  for your Notion integration token if `NOTION_TOKEN` isn't in your environment.
- **`serve-mcp`** — expose retrieval as an MCP tool over stdio so an MCP-aware
  agent (e.g. OpenClaw) can call it. The same retrieval is also reachable
  ad-hoc via the `query` subcommand for shell debugging.
- **`eval`** — measure retrieval quality (Recall@5 today, faithfulness later)
  against a hand-authored gold set.

## Prerequisites

- **Rust 1.91 or newer** (required by `lancedb 0.30`). Install via
  [rustup](https://rustup.rs/).
- **`protoc`** (Protocol Buffers compiler) — a build-time dependency of
  LanceDB.
  ```
  brew install protobuf      # macOS
  apt install protobuf-compiler   # Debian/Ubuntu
  ```
- **`NOTION_TOKEN`** in your environment (or in a `.env` file at the project
  root). Create an internal integration in Notion and share the root page(s)
  you want indexed with that integration.
- Optional: **`RAG_DB_PATH`** — where the LanceDB directory lives. Defaults to
  `./data/lancedb`.

## Ingest

Run the full ingest pipeline:

```
cargo run -- ingest
```

What it does, in order:

1. Crawls Notion starting from the configured root page IDs, recursively
   following nested blocks and child pages.
2. Extracts plain text from each block and assembles per-page `SourceDoc`s.
3. Chunks each `SourceDoc` into ≤384-token chunks with heading-aware
   boundaries.
4. Embeds each chunk with `intfloat/multilingual-e5-small` (384-dim,
   L2-normalized, with the `passage:` prefix the E5 family requires).
5. Upserts the rows into a local LanceDB table (`notes`) using
   `merge_insert` keyed by `chunk_id`, with orphan cleanup scoped to the
   re-ingested pages.

### First run

The first invocation downloads the embedding model weights (~120 MB) into the
fastembed cache directory. This is **one-time** and the run will appear to
pause for 30–60 seconds while it downloads — that is not a hang.

### Expected output

You should see log lines roughly like:

```
INFO rag_personal::pipeline: fetched 12 docs
INFO rag_personal::pipeline: chunked 12 docs → 87 chunks (chars min/median/max: 142/612/1481)
INFO rag_personal::pipeline: embedded 87 chunks (384-dim)
INFO rag_personal::pipeline: upserted 87 rows to vector store
```

After the run completes, `./data/lancedb/notes.lance/` exists and contains
the persisted Arrow data. Re-running `cargo run -- ingest` is idempotent:
existing chunks are updated in place, and any chunks that no longer appear
in a re-ingested page are deleted.

### Adjusting log verbosity

Use the standard `RUST_LOG` env var:

```
RUST_LOG=debug cargo run -- ingest    # noisier
RUST_LOG=warn cargo run -- ingest     # quieter
```

## Query

Ad-hoc retrieval from the shell. Same code path the MCP server uses, exposed
as a CLI for debugging and exploration.

```
cargo run -- query "<your question>"
cargo run -- query "<your question>" --top-k 10 --mode hybrid
```

Flags:

- `--top-k <N>` (default `5`) — how many hits to return.
- `--mode <dense|lexical|hybrid>` (default `hybrid`) — which retriever to use.
  Useful for spot-checking whether a miss is a dense-only or lexical-only
  problem.

Each hit is printed with its score, page title, Notion URL, and a 240-char
preview of the chunk text.

## Serve MCP

Run an MCP server over stdio. An MCP-aware agent (OpenClaw, Claude Desktop,
etc.) launches this binary as a child process and calls the tool over
stdin/stdout.

```
cargo run -- serve-mcp
```

The server exposes one tool:

- **`search_notes(query: string, top_k: integer)`** — runs the same hybrid
  retriever as the `query` command and returns a JSON list of
  `{ text, title, url, score }` hits.

Stdout is reserved for the MCP protocol; all logs go to stderr. Register the
binary with your agent once it's built, e.g. for OpenClaw:

```
openclaw mcp set notion-rag '{"command":"/abs/path/to/rag_personal", \
  "args":["serve-mcp"], "env":{"NOTION_TOKEN":"...", \
  "RAG_DB_PATH":"/abs/path/data/lancedb"}}'
openclaw mcp list      # verify it shows up
```

## Eval

Measure retrieval quality against a hand-authored gold set at
`eval/gold.jsonl`. One JSON object per line, schema:

```
{"q": "what is the goal for X?",
 "relevant_page_ids": ["<notion_page_id>"],
 "answer_span": "exact phrase from the answering sentence"}
```

`answer_span` is optional — when present, a hit requires the retrieved chunk
to contain that phrase (chunk-level relevance). When absent, page-level
relevance is used instead and the entry is marked as using the looser rule.

### Recall@5

```
cargo run -- eval recall
cargo run -- eval recall --gold path/to/gold.jsonl --top-k 5
```

Runs every gold question through the **dense**, **lexical**, and **hybrid**
retrievers and prints a 3-row Markdown table with:

- **Hits / Total / Recall@5** — the headline metric per mode.
- **PageMiss** — the relevant page wasn't in the top-K at all.
- **SpanMiss** — the page was in the top-K but no chunk contained the
  `answer_span` (only fires when `answer_span` is set).

The dense/lexical/hybrid split makes it visible when fusion *regresses* vs.
either leg alone, which is the main risk RRF can introduce.

## Inspect

Read-only look at what's currently in the LanceDB store. Useful for verifying
that an ingest run actually landed, sanity-checking chunk boundaries, or
piping rows to another tool.

### Show table stats

```
cargo run -- inspect --stats
```

Prints the total row count and the number of unique pages:

```
Total rows:   87
Unique pages: 12
```

### Browse rows

By default, scans up to 10 rows and prints a short text preview per chunk:

```
cargo run -- inspect              # first 10 rows
cargo run -- inspect --limit 25   # first 25 rows
```

Expected output:

```
[1] chunk_id=ab12cd34-0  page=Project notes  url=https://www.notion.so/...
    Some short preview of the chunk text, truncated at 200 characters…

[2] chunk_id=ab12cd34-1  page=Project notes  url=https://www.notion.so/...
    Next chunk's preview…

Showing 10 row(s).
```

### Filter by page

Restrict the scan to a single Notion page (the `page_id` column matches the
Notion page UUID):

```
cargo run -- inspect --page-id 1234abcd-5678-...
```

### JSON output

Pipe-friendly output for `jq` or further processing:

```
cargo run -- inspect --limit 3 --json | jq
cargo run -- inspect --stats --json
```

`--json` works with both row scans and `--stats`.

## Environment

### Memory considerations during embedding

The embedding step (`embedder.embed_passages`) is the most memory-intensive
stage of the pipeline. On low-RAM machines (~8 GB) it can be OOM-killed if
all chunks are sent in a single call.

The cost is **not** the output vectors — 1,000 chunks × 384 dims × 4 bytes
is only ~1.5 MB. The cost is the **ONNX Runtime activations during the
forward pass** of the E5 transformer:

- Hidden states per layer: `[batch, seq_len, 384]`
- Attention scores per layer: `[batch, heads, seq_len, seq_len]` — quadratic
  in sequence length, so the 512-token max dominates.

For a batch of 256 at seq_len 512, peak activation memory can reach 1–2 GB
on top of the ~120 MB model weights and OS baseline. On an 8 GB machine
that is enough to trip the OOM killer.

**Mitigation**: process embeddings in bounded batches (e.g. 32 chunks at a
time) so peak activation memory stays well under the available RAM.
Pathologically large chunks (tens of thousands of characters) also inflate
tokenizer buffers and should be capped at the chunking stage.
