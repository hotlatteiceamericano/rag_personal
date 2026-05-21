use std::{collections::VecDeque, vec};

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
    base_url: String,
}

impl NotionSource {
    pub fn new(client: reqwest::Client, token: String, page_ids: Vec<String>) -> Self {
        NotionSource {
            client,
            token,
            page_ids,
            base_url: "https://api.notion.com".to_string(),
        }
    }

    #[cfg(test)]
    fn with_base_url(client: reqwest::Client, token: String, base_url: String) -> Self {
        NotionSource {
            client,
            token,
            page_ids: vec![],
            base_url,
        }
    }

    pub async fn fetch_block_children(&self, block_id: &str) -> anyhow::Result<BlockListResponse> {
        // todo: consider wrap the API call in another function
        // for better error handling and exponential retry
        let url = format!("{}/v1/blocks/{}/children", self.base_url, block_id);

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

        Ok(resp)
    }
}

#[async_trait]
impl Source for NotionSource {
    // todo:
    // 1. put root page ids to a queue
    // 2. call block children api for each root page ids
    // 3. get the page info intself and convert it to a SourceDoc
    // 4. then gets all the child page from this page, and push them into the queue
    async fn fetch(&self) -> anyhow::Result<Vec<SourceDoc>> {
        let mut queue = self.page_ids.clone();
        let mut source_docs: Vec<SourceDoc> = Vec::new();
        while let Some(page_id) = queue.pop() {
            // warn here if not able to get block children
            let resp = self.fetch_block_children(&page_id).await?;

            // source_docs.extend(get_source_docs(&resp));
            //
            // let child_page_ids = get_child_page_ids(&resp);
            //
            // queue.extend(child_page_ids);
        }

        anyhow::Ok(vec![])
    }
}

fn get_child_page_ids(resp: &BlockListResponse) -> Vec<String> {
    todo!()
}

fn get_source_docs(resp: &BlockListResponse) -> Vec<SourceDoc> {
    todo!()
}

// model for api.notion.com/v1/blocks/{block_id}/children API
// contains list of Block in its `results` field
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct BlockListResponse {
    has_more: bool,
    results: Vec<Block>,
    next_cursor: Option<String>,
}

// each "block" in a page can be a Paragraph, Heading, BulletItem and more
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct Block {
    id: String,
    has_children: bool,
    #[serde(flatten)]
    body: BlockBody,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum BlockBody {
    Known(KnownBlock),
    Unknown(serde_json::Value),
}

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct RichTextHolder {
    rich_text: Vec<RichText>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct RichText {
    plain_text: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct CodeBody {
    rich_text: Vec<RichText>,
    language: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{bearer_token, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetch_block_children_parses_response() {
        let server = MockServer::start().await;

        let body = serde_json::json!({
            "has_more": false,
            "next_cursor": null,
            "results": [{
                "id": "block-1",
                "has_children": false,
                "type": "paragraph",
                "paragraph": {
                    "rich_text": [{ "plain_text": "hello world" }]
                }
            }]
        });

        Mock::given(method("GET"))
            .and(path("/v1/blocks/page-123/children"))
            .and(bearer_token("test-token"))
            .and(header("Notion-Version", "2022-06-28"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let source = NotionSource::with_base_url(
            reqwest::Client::new(),
            "test-token".to_string(),
            server.uri(),
        );

        let resp = source.fetch_block_children("page-123").await.unwrap();

        assert!(!resp.has_more);
        assert!(resp.next_cursor.is_none());
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].id, "block-1");
        assert!(!resp.results[0].has_children);
        match &resp.results[0].body {
            BlockBody::Known(KnownBlock::Paragraph { paragraph }) => {
                assert_eq!(paragraph.rich_text[0].plain_text, "hello world");
            }
            other => panic!("expected Paragraph, got {:?}", other),
        }
    }
}
