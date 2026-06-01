# rag_personal

A single-binary Rust pipeline that ingests Notion notes, embeds them with a
multilingual model, and stores the results in a local LanceDB for retrieval.

## Internal concepts

`SourceDoc` is the internal unit of a documentation from a source. A Notion
page, a Google Doc page are all converted to a `SourceDoc`.

A `SourceDoc` then being chunked into different units for embedding.

todo: I want a diagram showing what are the input and output between each
component (from Notion source to internal struct, to chunker, to embedder etc)

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
