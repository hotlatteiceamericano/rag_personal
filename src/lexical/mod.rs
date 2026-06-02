pub mod tantivy_index;

use crate::chunk::Chunk;
use crate::store::Hit;

pub trait LexicalIndex: Send + Sync {
    fn upsert(&self, chunks: &[Chunk]) -> anyhow::Result<()>;
    fn search(&self, query: &str, k: usize) -> anyhow::Result<Vec<Hit>>;
}
