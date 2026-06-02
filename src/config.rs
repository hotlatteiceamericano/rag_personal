use std::{env, path::PathBuf};

pub struct Config {
    pub notion_token: Option<String>,
    pub root_page_ids: Vec<String>,
    pub chunk_target_tokens: usize,
    pub db_path: PathBuf,
    pub lexical_path: PathBuf,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let notion_token = env::var("NOTION_TOKEN").ok();
        let root_page_ids = vec![
            "3108458c27988048b5b8eef713e581cc".to_string(),
            "8419cd72c7c54e698bb293c770030357".to_string(),
        ];
        let db_path = env::var("RAG_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data/lancedb"));
        let lexical_path = env::var("RAG_LEXICAL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data/tantivy"));

        Ok(Self {
            notion_token,
            root_page_ids,
            chunk_target_tokens: 384,
            db_path,
            lexical_path,
        })
    }
}
