use std::sync::Arc;

use crate::lexical::LexicalIndex;
use crate::store::Hit;

use super::Retriever;

pub struct LexicalRetriever<L: LexicalIndex> {
    index: Arc<L>,
}

impl<L: LexicalIndex> LexicalRetriever<L> {
    pub fn new(index: Arc<L>) -> Self {
        Self { index }
    }
}

#[async_trait::async_trait]
impl<L: LexicalIndex + Send + Sync> Retriever for LexicalRetriever<L> {
    async fn retrieve(&self, query: &str, top_k: usize) -> anyhow::Result<Vec<Hit>> {
        self.index.search(query, top_k)
    }
}
