use std::env;

use anyhow::Context;

pub struct Config {
    pub notion_token: String,
    pub root_page_ids: Vec<String>,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let notion_token = env::var("NOTION_TOKEN").context("NOTION_TOKEN not set")?;
        let root_page_ids = vec!["4f596d0962ca45b7bf5b3937364a8374".to_string()];

        Ok(Self {
            notion_token,
            root_page_ids,
        })
    }
}
