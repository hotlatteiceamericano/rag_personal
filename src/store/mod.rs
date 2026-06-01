pub mod lance_store;

#[derive(Debug, Clone)]
pub struct EmbeddedChunk {
    pub chunk_id: String,
    pub page_id: String,
    pub title: String,
    pub url: String,
    pub text: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub chunk_id: String,
    pub page_id: String,
    pub title: String,
    pub url: String,
    pub text: String,
    pub score: f32,
}

#[async_trait::async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert(&self, rows: Vec<EmbeddedChunk>) -> anyhow::Result<()>;
    async fn search(&self, query_vec: &[f32], k: usize) -> anyhow::Result<Vec<Hit>>;
    async fn delete_page(&self, page_id: &str) -> anyhow::Result<()>;
}
