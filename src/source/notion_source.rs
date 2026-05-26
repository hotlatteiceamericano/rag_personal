use std::vec;

use anyhow::Context;
use async_trait::async_trait;
use tracing::warn;

use crate::source::dto::notion::{
    Block, BlockBody, BlockListResponse, KnownBlock, PageProperty, PageResponse, join_rich,
};
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
                KnownBlock::ChildPage { .. } => return None,
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

    fn extract_child_page_ids(resp: &BlockListResponse) -> Vec<String> {
        resp.results
            .iter()
            .filter_map(|block| match &block.body {
                BlockBody::Known(KnownBlock::ChildPage { .. }) => Some(block.id.clone()),
                _ => None,
            })
            .collect()
    }
}

#[async_trait]
impl Source for NotionSource {
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
                queue.extend(NotionSource::extract_child_page_ids(&resp));

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
        }

        anyhow::Ok(docs)
    }
}

#[derive(Debug)]
pub struct PageMeta {
    pub title: String,
    pub url: String,
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
