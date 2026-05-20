use std::{env, vec};

use anyhow::Context;
use async_trait::async_trait;
use serde_json::Value;
use tracing::info;

use crate::source::{Document, Source};

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
    async fn fetch(&self) -> anyhow::Result<Vec<Document>> {
        let token = env::var("NOTION_TOKEN").context("NOTION_TOKEN not set")?;
        let url = format!(
            "https://api.notion.com/v1/blocks/{}/children?page_size=100",
            "4f596d0962ca45b7bf5b3937364a8374"
        );

        let resp: Value = reqwest::Client::new()
            .get(&url)
            .bearer_auth(&token)
            .header("Notion-Version", "2022-06-28")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        info!("{}", serde_json::to_string_pretty(&resp)?);

        anyhow::Ok(vec![])
    }
}
