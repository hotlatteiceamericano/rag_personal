use std::result;

use async_trait::async_trait;
use serde::Deserialize;

pub mod notion;

// internal type to abstract from different sources
#[derive(Debug)]
pub struct SourceDoc {
    pub page_id: String,
    pub title: String,
    pub url: String,
    pub blocks: Vec<TextBlock>,
}

#[derive(Debug)]
pub struct TextBlock {
    pub heading_path: Vec<String>,
    pub text: String,
    pub kind: BlockKind,
}

#[derive(Debug)]
pub enum BlockKind {
    Heading(u8),
    Paragraph,
    ListItem,
    Quote,
    Code,
    BulletedListItem,
    NumberedListItem,
    ToDo,
    Toggle,
}

#[async_trait]
pub trait Source {
    async fn fetch(&self) -> anyhow::Result<Vec<SourceDoc>>;
}
