use std::result;

use async_trait::async_trait;

pub mod notion;

// internal type to abstract from different sources
pub struct Document {
    pub id: String,
    pub text: String,
}

#[async_trait]
pub trait Source {
    async fn fetch(&self) -> anyhow::Result<Vec<Document>>;
}
