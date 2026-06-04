use tracing::info;

use crate::chunk::{Chunk, Chunker};
use crate::embed::Embedder;
use crate::lexical::LexicalIndex;
use crate::source::Source;
use crate::store::{EmbeddedChunk, VectorStore};

pub async fn ingest(
    source: &impl Source,
    chunker: &impl Chunker,
    embedder: &impl Embedder,
    store: &impl VectorStore,
    lexical: &impl LexicalIndex,
) -> anyhow::Result<()> {
    let docs = source.fetch().await?;
    info!("fetched {} docs", docs.len());

    let chunks: Vec<Chunk> = docs.iter().flat_map(|d| chunker.chunk(d)).collect();
    let (min, median, max) = chunk_char_stats(&chunks);
    info!(
        "chunked {} docs → {} chunks (chars min/median/max: {}/{}/{})",
        docs.len(),
        chunks.len(),
        min,
        median,
        max,
    );

    if chunks.is_empty() {
        info!("nothing to embed, skipping upsert");
        return Ok(());
    }

    lexical.upsert(&chunks)?;
    info!("indexed {} chunks into lexical store", chunks.len());

    const EMBED_BATCH: usize = 32;
    let mut vectors: Vec<Vec<f32>> = Vec::with_capacity(chunks.len());
    for batch in chunks.chunks(EMBED_BATCH) {
        let texts: Vec<String> = batch.iter().map(|c| c.text.clone()).collect();
        let batch_vecs = embedder.embed_passages(&texts)?;
        vectors.extend(batch_vecs);
    }
    info!(
        "embedded {} chunks ({}-dim)",
        vectors.len(),
        embedder.dimension()
    );

    let rows: Vec<EmbeddedChunk> = chunks
        .into_iter()
        .zip(vectors)
        .map(|(c, vector)| EmbeddedChunk {
            chunk_id: c.chunk_id,
            page_id: c.page_id,
            title: c.title,
            url: c.url,
            text: c.text,
            vector,
        })
        .collect();

    let upserted = rows.len();
    store.upsert(rows).await?;
    info!("upserted {upserted} rows to vector store");
    Ok(())
}

fn chunk_char_stats(chunks: &[Chunk]) -> (usize, usize, usize) {
    if chunks.is_empty() {
        return (0, 0, 0);
    }
    let mut lens: Vec<usize> = chunks.iter().map(|c| c.text.chars().count()).collect();
    lens.sort_unstable();
    (lens[0], lens[lens.len() / 2], lens[lens.len() - 1])
}
