use std::{env, vec};

use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

use crate::source::{Source, SourceDoc};

pub struct NotionSource {
    client: reqwest::Client,
    token: String,
    page_ids: Vec<String>,
}

impl NotionSource {
    pub fn new(client: reqwest::Client, token: String, page_ids: Vec<String>) -> Self {
        NotionSource {
            client,
            token,
            page_ids,
        }
    }
}

#[async_trait]
impl Source for NotionSource {
    async fn fetch(&self) -> anyhow::Result<Vec<SourceDoc>> {
        let url = format!(
            "https://api.notion.com/v1/blocks/{}/children?page_size=100",
            "4f596d0962ca45b7bf5b3937364a8374" // todo: replace and iterate all page ids
        );

        let resp: BlockListResponse = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header("Notion-Version", "2022-06-28")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        info!("{:?}", resp);

        anyhow::Ok(vec![])
    }
}

#[derive(Deserialize, Debug)]
struct BlockListResponse {
    has_more: bool,
    results: Vec<Block>,
    next_cursor: Option<String>,
}

#[derive(Deserialize, Debug)]
struct Block {
    id: String,
    has_children: bool,
    #[serde(flatten)]
    body: BlockBody,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum BlockBody {
    Known(KnownBlock),
    Unknown(serde_json::Value),
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum KnownBlock {
    Paragraph { paragraph: RichTextHolder },
    Heading1 { heading_1: RichTextHolder },
    Heading2 { heading_2: RichTextHolder },
    Heading3 { heading_3: RichTextHolder },
    BulletedListItem { bulleted_list_item: RichTextHolder },
    NumberedListItem { numbered_list_item: RichTextHolder },
    ToDo { to_do: RichTextHolder },
    Toggle { toggle: RichTextHolder },
    Quote { quote: RichTextHolder },
    Callout { callout: RichTextHolder },
    Code { code: CodeBody },
}

#[derive(Deserialize, Debug)]
struct RichTextHolder {
    rich_text: Vec<RichText>,
}
#[derive(Deserialize, Debug)]
struct RichText {
    plain_text: String,
}
#[derive(Deserialize, Debug)]
struct CodeBody {
    rich_text: Vec<RichText>,
    language: String,
}
