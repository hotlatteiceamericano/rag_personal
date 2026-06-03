use std::sync::Arc;

use crate::embed::Embedder;
use crate::store::{Hit, VectorStore};

use super::Retriever;

pub struct DenseRetriever<E: Embedder, S: VectorStore> {
    embedder: Arc<E>,
    store: Arc<S>,
}

impl<E: Embedder, S: VectorStore> DenseRetriever<E, S> {
    pub fn new(embedder: Arc<E>, store: Arc<S>) -> Self {
        Self { embedder, store }
    }
}

#[async_trait::async_trait]
impl<E: Embedder + Send + Sync, S: VectorStore + Send + Sync> Retriever for DenseRetriever<E, S> {
    async fn retrieve(&self, query: &str, top_k: usize) -> anyhow::Result<Vec<Hit>> {
        let qv = self.embedder.embed_query(query)?;
        self.store.search(&qv, top_k).await
    }
}
