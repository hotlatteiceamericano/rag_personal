use async_trait::async_trait;

mod dto;
pub mod notion_source;

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

impl BlockKind {
    pub fn is_heading(&self) -> bool {
        matches!(self, BlockKind::Heading(_))
    }
}

#[async_trait]
pub trait Source {
    async fn fetch(&self) -> anyhow::Result<Vec<SourceDoc>>;
}
