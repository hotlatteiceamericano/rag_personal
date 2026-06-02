use clap::ValueEnum;

use crate::embed::Embedder;
use crate::store::{Hit, VectorStore};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RetrievalMode {
    Dense,
    Lexical,
    Hybrid,
}

#[async_trait::async_trait]
pub trait Retriever: Send + Sync {
    async fn retrieve(&self, query: &str, top_k: usize) -> anyhow::Result<Vec<Hit>>;
}

pub struct DenseRetriever<'a, E: Embedder, S: VectorStore> {
    embedder: &'a E,
    store: &'a S,
}

impl<'a, E: Embedder, S: VectorStore> DenseRetriever<'a, E, S> {
    pub fn new(embedder: &'a E, store: &'a S) -> Self {
        Self { embedder, store }
    }
}

#[async_trait::async_trait]
impl<E: Embedder + Sync, S: VectorStore> Retriever for DenseRetriever<'_, E, S> {
    async fn retrieve(&self, query: &str, top_k: usize) -> anyhow::Result<Vec<Hit>> {
        let qv = self.embedder.embed_query(query)?;
        self.store.search(&qv, top_k).await
    }
}
