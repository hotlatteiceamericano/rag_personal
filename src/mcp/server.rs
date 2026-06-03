use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

use crate::retrieve::Retriever;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchNotesArgs {
    /// The natural-language search query.
    pub query: String,
    /// Number of results to return (clamped to 1..=20, default 5).
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    5
}

#[derive(Debug, Serialize)]
struct HitJson {
    title: String,
    url: String,
    text: String,
    score: f32,
}

#[derive(Clone)]
pub struct RagServer {
    retriever: Arc<dyn Retriever>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl RagServer {
    pub fn new(retriever: Arc<dyn Retriever>) -> Self {
        Self {
            retriever,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Search the user's personal Notion notes for relevant passages. \
                       Returns a JSON list of hits, each with title, url, text, and relevance score."
    )]
    async fn search_notes(
        &self,
        Parameters(args): Parameters<SearchNotesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let top_k = args.top_k.clamp(1, 20);
        tracing::info!(top_k, query = %args.query, "search_notes request");
        let hits = self
            .retriever
            .retrieve(&args.query, top_k)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let results: Vec<HitJson> = hits
            .into_iter()
            .map(|h| HitJson {
                title: h.title,
                url: h.url,
                text: h.text,
                score: h.score,
            })
            .collect();

        let json = serde_json::to_string(&results)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

#[tool_handler]
impl ServerHandler for RagServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Search and retrieve relevant passages from the user's personal Notion notes."
                    .to_string(),
            )
    }
}
