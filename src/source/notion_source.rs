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
    fn with_base_url(
        client: reqwest::Client,
        token: String,
        base_url: String,
        page_ids: Vec<String>,
    ) -> Self {
        NotionSource {
            client,
            token,
            page_ids,
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

    // Fully paginates one parent's children into a flat Vec, preserving order.
    async fn fetch_block_children_pagination(&self, block_id: &str) -> anyhow::Result<Vec<Block>> {
        let mut out: Vec<Block> = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let resp = self
                .fetch_block_children(block_id, cursor.as_deref())
                .await
                .with_context(|| format!("fetch block children for {block_id}"))?;
            out.extend(resp.results);
            if !resp.has_more {
                break;
            }
            cursor = resp.next_cursor;
        }
        Ok(out)
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
}

#[async_trait]
impl Source for NotionSource {
    // refactor and abstract each functionalties:
    // 1. iterate initial root page ids in a queue
    // 2. fetch and iterate notion blocks:
    //   1. fetch child page id and push them to the queue
    //   2. convert notion blocks to internal blocks
    //   3. for other children, fetch their descedent blocks for further iteration
    async fn fetch(&self) -> anyhow::Result<Vec<SourceDoc>> {
        let mut queue = self.page_ids.clone();
        let mut docs: Vec<SourceDoc> = Vec::new();

        while let Some(page_id) = queue.pop() {
            let meta = self.fetch_page_meta(&page_id).await?;

            let mut text_blocks: Vec<TextBlock> = Vec::new();
            let mut notion_blocks: Vec<Block> = Vec::new();

            // Seed with the page's top-level children, reversed so the first
            // child ends up on top of the stack (pre-order DFS).
            for b in self
                .fetch_block_children_pagination(&page_id)
                .await?
                .into_iter()
                .rev()
            {
                notion_blocks.push(b);
            }

            while let Some(block) = notion_blocks.pop() {
                // child_page becomes its own SourceDoc; don't fold into parent.
                if matches!(&block.body, BlockBody::Known(KnownBlock::ChildPage { .. })) {
                    queue.push(block.id);
                    continue;
                }

                if let Some(tb) = NotionSource::block_to_text_block(&block) {
                    text_blocks.push(tb);
                }

                if block.has_children {
                    for c in self
                        .fetch_block_children_pagination(&block.id)
                        .await?
                        .into_iter()
                        .rev()
                    {
                        notion_blocks.push(c);
                    }
                }
            }

            docs.push(SourceDoc {
                page_id,
                title: meta.title,
                url: meta.url,
                blocks: text_blocks,
            });
        }

        Ok(docs)
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
    use wiremock::matchers::{
        bearer_token, header, method, path, query_param, query_param_is_missing,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_source(server_uri: String, page_ids: Vec<String>) -> NotionSource {
        NotionSource::with_base_url(
            reqwest::Client::new(),
            "test-token".to_string(),
            server_uri,
            page_ids,
        )
    }

    async fn mock_page_meta(server: &MockServer, page_id: &str, title: &str, url: &str) {
        Mock::given(method("GET"))
            .and(path(format!("/v1/pages/{page_id}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": url,
                "properties": {
                    "title": { "type": "title", "title": [{ "plain_text": title }] }
                }
            })))
            .mount(server)
            .await;
    }

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
            vec![],
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

    #[tokio::test]
    async fn fetch_returns_source_doc_for_single_page() {
        let server = MockServer::start().await;

        mock_page_meta(&server, "page-A", "Page A", "https://notion.so/page-A").await;

        Mock::given(method("GET"))
            .and(path("/v1/blocks/page-A/children"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "has_more": false,
                "next_cursor": null,
                "results": [
                    {
                        "id": "b1",
                        "has_children": false,
                        "type": "heading_1",
                        "heading_1": { "rich_text": [{ "plain_text": "Title" }] }
                    },
                    {
                        "id": "b2",
                        "has_children": false,
                        "type": "paragraph",
                        "paragraph": { "rich_text": [{ "plain_text": "hello" }] }
                    }
                ]
            })))
            .mount(&server)
            .await;

        let source = test_source(server.uri(), vec!["page-A".to_string()]);
        let docs = source.fetch().await.unwrap();

        assert_eq!(docs.len(), 1);
        let doc = &docs[0];
        assert_eq!(doc.page_id, "page-A");
        assert_eq!(doc.title, "Page A");
        assert_eq!(doc.url, "https://notion.so/page-A");
        assert_eq!(doc.blocks.len(), 2);
        assert!(doc.blocks[0].kind.is_heading());
        assert_eq!(doc.blocks[0].text, "Title");
        assert_eq!(doc.blocks[1].text, "hello");
        assert!(matches!(doc.blocks[1].kind, BlockKind::Paragraph));
    }

    #[tokio::test]
    async fn fetch_recursively_follows_child_pages() {
        let server = MockServer::start().await;

        // Parent page: one paragraph + one child_page block pointing at "child-B"
        mock_page_meta(&server, "page-A", "Page A", "https://notion.so/page-A").await;
        Mock::given(method("GET"))
            .and(path("/v1/blocks/page-A/children"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "has_more": false,
                "next_cursor": null,
                "results": [
                    {
                        "id": "b1",
                        "has_children": false,
                        "type": "paragraph",
                        "paragraph": { "rich_text": [{ "plain_text": "parent body" }] }
                    },
                    {
                        "id": "child-B",
                        "has_children": true,
                        "type": "child_page",
                        "child_page": { "title": "Child B" }
                    }
                ]
            })))
            .mount(&server)
            .await;

        // Child page
        mock_page_meta(&server, "child-B", "Child B", "https://notion.so/child-B").await;
        Mock::given(method("GET"))
            .and(path("/v1/blocks/child-B/children"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "has_more": false,
                "next_cursor": null,
                "results": [
                    {
                        "id": "b2",
                        "has_children": false,
                        "type": "paragraph",
                        "paragraph": { "rich_text": [{ "plain_text": "child body" }] }
                    }
                ]
            })))
            .mount(&server)
            .await;

        let source = test_source(server.uri(), vec!["page-A".to_string()]);
        let docs = source.fetch().await.unwrap();

        assert_eq!(docs.len(), 2);

        let parent = docs.iter().find(|d| d.page_id == "page-A").unwrap();
        let child = docs.iter().find(|d| d.page_id == "child-B").unwrap();

        // child_page block must NOT appear in the parent's blocks
        assert_eq!(parent.blocks.len(), 1);
        assert_eq!(parent.blocks[0].text, "parent body");

        assert_eq!(child.title, "Child B");
        assert_eq!(child.blocks.len(), 1);
        assert_eq!(child.blocks[0].text, "child body");
    }

    #[tokio::test]
    async fn fetch_handles_pagination() {
        let server = MockServer::start().await;

        mock_page_meta(&server, "page-A", "Page A", "https://notion.so/page-A").await;

        // First call: no start_cursor, has_more=true, next_cursor="cursor-2"
        Mock::given(method("GET"))
            .and(path("/v1/blocks/page-A/children"))
            .and(query_param_is_missing("start_cursor"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "has_more": true,
                "next_cursor": "cursor-2",
                "results": [
                    {
                        "id": "b1",
                        "has_children": false,
                        "type": "paragraph",
                        "paragraph": { "rich_text": [{ "plain_text": "page 1" }] }
                    }
                ]
            })))
            .mount(&server)
            .await;

        // Second call: start_cursor=cursor-2, has_more=false
        Mock::given(method("GET"))
            .and(path("/v1/blocks/page-A/children"))
            .and(query_param("start_cursor", "cursor-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "has_more": false,
                "next_cursor": null,
                "results": [
                    {
                        "id": "b2",
                        "has_children": false,
                        "type": "paragraph",
                        "paragraph": { "rich_text": [{ "plain_text": "page 2" }] }
                    }
                ]
            })))
            .mount(&server)
            .await;

        let source = test_source(server.uri(), vec!["page-A".to_string()]);
        let docs = source.fetch().await.unwrap();

        assert_eq!(docs.len(), 1);
        let blocks = &docs[0].blocks;
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "page 1");
        assert_eq!(blocks[1].text, "page 2");
    }

    #[tokio::test]
    async fn fetch_folds_nested_bullets_into_parent_doc() {
        let server = MockServer::start().await;

        mock_page_meta(&server, "page-A", "Page A", "https://notion.so/page-A").await;

        // Top-level: a bulleted_list_item with has_children=true, followed
        // by a paragraph sibling.
        Mock::given(method("GET"))
            .and(path("/v1/blocks/page-A/children"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "has_more": false,
                "next_cursor": null,
                "results": [
                    {
                        "id": "bullet-parent",
                        "has_children": true,
                        "type": "bulleted_list_item",
                        "bulleted_list_item": {
                            "rich_text": [{ "plain_text": "parent bullet" }]
                        }
                    },
                    {
                        "id": "p-after",
                        "has_children": false,
                        "type": "paragraph",
                        "paragraph": { "rich_text": [{ "plain_text": "after" }] }
                    }
                ]
            })))
            .mount(&server)
            .await;

        // Nested children of bullet-parent.
        Mock::given(method("GET"))
            .and(path("/v1/blocks/bullet-parent/children"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "has_more": false,
                "next_cursor": null,
                "results": [
                    {
                        "id": "bullet-child",
                        "has_children": false,
                        "type": "bulleted_list_item",
                        "bulleted_list_item": {
                            "rich_text": [{ "plain_text": "nested bullet" }]
                        }
                    }
                ]
            })))
            .mount(&server)
            .await;

        let source = test_source(server.uri(), vec!["page-A".to_string()]);
        let docs = source.fetch().await.unwrap();

        assert_eq!(docs.len(), 1);
        let blocks = &docs[0].blocks;
        assert_eq!(blocks.len(), 3);
        // Pre-order DFS: parent → nested child → next sibling.
        assert_eq!(blocks[0].text, "parent bullet");
        assert!(matches!(blocks[0].kind, BlockKind::BulletedListItem));
        assert_eq!(blocks[1].text, "nested bullet");
        assert!(matches!(blocks[1].kind, BlockKind::BulletedListItem));
        assert_eq!(blocks[2].text, "after");
        assert!(matches!(blocks[2].kind, BlockKind::Paragraph));
    }
}
