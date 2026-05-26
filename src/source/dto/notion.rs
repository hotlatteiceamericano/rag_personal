use std::collections::HashMap;

use serde::Deserialize;

// model for api.notion.com/v1/pages/{page_id} API
#[derive(Deserialize, Debug)]
pub struct PageResponse {
    pub url: String,
    pub properties: HashMap<String, PageProperty>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PageProperty {
    Title {
        title: Vec<RichText>,
    },
    #[serde(other)]
    Other,
}

// model for api.notion.com/v1/blocks/{block_id}/children API
// contains list of Block in its `results` field
#[derive(Deserialize, Debug)]
pub struct BlockListResponse {
    pub has_more: bool,
    pub results: Vec<Block>,
    pub next_cursor: Option<String>,
}

// each "block" in a page can be a Paragraph, Heading, BulletItem and more
#[derive(Deserialize, Debug)]
pub struct Block {
    pub id: String,
    #[allow(dead_code)] // read only in tests
    pub has_children: bool,
    #[serde(flatten)]
    pub body: BlockBody,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum BlockBody {
    Known(KnownBlock),
    Unknown(serde_json::Value),
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KnownBlock {
    Paragraph { paragraph: RichTextHolder },
    #[serde(rename = "heading_1")]
    Heading1 { heading_1: RichTextHolder },
    #[serde(rename = "heading_2")]
    Heading2 { heading_2: RichTextHolder },
    #[serde(rename = "heading_3")]
    Heading3 { heading_3: RichTextHolder },
    BulletedListItem { bulleted_list_item: RichTextHolder },
    NumberedListItem { numbered_list_item: RichTextHolder },
    ToDo { to_do: RichTextHolder },
    Toggle { toggle: RichTextHolder },
    Quote { quote: RichTextHolder },
    Code { code: CodeBody },
    ChildPage { child_page: ChildPageBody },
}

#[derive(Deserialize, Debug)]
pub struct RichTextHolder {
    pub rich_text: Vec<RichText>,
}

#[derive(Deserialize, Debug)]
pub struct RichText {
    pub plain_text: String,
}

#[derive(Deserialize, Debug)]
pub struct CodeBody {
    pub rich_text: Vec<RichText>,
    // pub language: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct ChildPageBody {
    pub title: String,
}

pub fn join_rich(rt: &[RichText]) -> String {
    rt.iter().map(|r| r.plain_text.as_str()).collect()
}
