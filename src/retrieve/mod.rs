pub mod dense_retriever;
pub mod hybrid_retriever;
pub mod lexical_retriever;

pub use dense_retriever::DenseRetriever;
pub use hybrid_retriever::HybridRetriever;
pub use lexical_retriever::LexicalRetriever;

use clap::ValueEnum;

use crate::store::Hit;

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
