use crate::lexical::LexicalIndex;
use crate::store::Hit;

use super::Retriever;

pub struct LexicalRetriever<'a, L: LexicalIndex> {
    index: &'a L,
}

impl<'a, L: LexicalIndex> LexicalRetriever<'a, L> {
    pub fn new(index: &'a L) -> Self {
        Self { index }
    }
}

#[async_trait::async_trait]
impl<L: LexicalIndex> Retriever for LexicalRetriever<'_, L> {
    async fn retrieve(&self, query: &str, top_k: usize) -> anyhow::Result<Vec<Hit>> {
        self.index.search(query, top_k)
    }
}
