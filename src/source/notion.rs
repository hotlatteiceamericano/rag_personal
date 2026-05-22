use std::{collections::HashMap, vec};

use anyhow::Context;
use async_trait::async_trait;
use serde::Deserialize;
use tracing::warn;

use crate::source::{BlockKind, Source, SourceDoc, TextBlock};

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

    // todo: consider wrap the API call in another function
    // for better error handling and exponential retry
    async fn fetch_block_children(
        &self,
        block_id: &str,
        cursor: Option<&str>,
    ) -> anyhow::Result<BlockListResponse> {
        let url = format!("{}/v1/blocks/{}/children", self.base_url, block_id);

        let mut req = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header("Notion-Version", "2022-06-28")
            .query(&[("page_size", "100")]);

        if let Some(c) = cursor {
            req = req.query(&[("start_cursor", c)]);
        }

        let resp: BlockListResponse = req.send().await?.error_for_status()?.json().await?;

        Ok(resp)
    }

    pub async fn fetch_page_meta(&self, page_id: &str) -> anyhow::Result<PageMeta> {
        let url = format!("{}/v1/pages/{}", self.base_url, page_id);

        let resp: PageResponse = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header("Notion-Version", "2022-06-28")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // Notion stores the title under whichever property has type "title"
        // (named "title" on plain pages, sometimes "Name" on database pages).
        let title = resp
            .properties
            .values()
            .find_map(|p| match p {
                PageProperty::Title { title } => Some(join_rich(title)),
                _ => None,
            })
            .unwrap_or_default();

        Ok(PageMeta {
            title,
            url: resp.url,
        })
    }

    fn extract_text_blocks(resp: &BlockListResponse) -> Vec<TextBlock> {
        resp.results
            .iter()
            .filter_map(Self::block_to_text_block)
            .collect()
    }

    fn block_to_text_block(b: &Block) -> Option<TextBlock> {
        let (text, kind) = match &b.body {
            BlockBody::Known(k) => match k {
                KnownBlock::Paragraph { paragraph } => {
                    (join_rich(&paragraph.rich_text), BlockKind::Paragraph)
                }
                KnownBlock::Heading1 { heading_1 } => {
                    (join_rich(&heading_1.rich_text), BlockKind::Heading(1))
                }
                KnownBlock::Heading2 { heading_2 } => {
                    (join_rich(&heading_2.rich_text), BlockKind::Heading(2))
                }
                KnownBlock::Heading3 { heading_3 } => {
                    (join_rich(&heading_3.rich_text), BlockKind::Heading(3))
                }
                KnownBlock::BulletedListItem { bulleted_list_item } => (
                    join_rich(&bulleted_list_item.rich_text),
                    BlockKind::BulletedListItem,
                ),
                KnownBlock::NumberedListItem { numbered_list_item } => (
                    join_rich(&numbered_list_item.rich_text),
                    BlockKind::NumberedListItem,
                ),
                KnownBlock::ToDo { to_do } => (join_rich(&to_do.rich_text), BlockKind::ToDo),
                KnownBlock::Toggle { toggle } => (join_rich(&toggle.rich_text), BlockKind::Toggle),
                KnownBlock::Quote { quote } => (join_rich(&quote.rich_text), BlockKind::Quote),
                KnownBlock::Code { code } => (join_rich(&code.rich_text), BlockKind::Code),
            },
            BlockBody::Unknown(v) => {
                warn!(id = &b.id, "skipping unknow block type, value: {v}");
                return None;
            }
        };

        if text.is_empty() {
            return None;
        }

        Some(TextBlock {
            heading_path: vec![],
            text,
            kind,
        })
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
        let mut docs: Vec<SourceDoc> = Vec::new();
        while let Some(page_id) = queue.pop() {
            let meta = self.fetch_page_meta(&page_id).await?;

            let mut blocks: Vec<TextBlock> = Vec::new();
            let mut cursor: Option<String> = None;

            loop {
                let resp = self
                    .fetch_block_children(&page_id, cursor.as_deref())
                    .await
                    .with_context(|| format!("fetch block children for {page_id}"))?;

                blocks.extend(NotionSource::extract_text_blocks(&resp));

                if !resp.has_more {
                    break;
                }

                cursor = resp.next_cursor;
            }

            docs.push(SourceDoc {
                page_id,
                title: meta.title,
                url: meta.url,
                blocks,
            });

            // todo: recursively handle children
            // let child_page_ids = get_child_page_ids(&resp);
            //
            // queue.extend(child_page_ids);
        }

        anyhow::Ok(docs)
    }
}

// model for api.notion.com/v1/pages/{page_id} API
#[derive(Debug)]
pub struct PageMeta {
    pub title: String,
    pub url: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct PageResponse {
    url: String,
    properties: HashMap<String, PageProperty>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PageProperty {
    Title {
        title: Vec<RichText>,
    },
    #[serde(other)]
    Other,
}

fn join_rich(rt: &[RichText]) -> String {
    rt.iter().map(|r| r.plain_text.as_str()).collect()
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

        let resp = source.fetch_block_children("page-123", None).await.unwrap();

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
