use std::env;

use anyhow::Context;
use serde_json::Value;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    println!("{}", serde_json::to_string_pretty(&resp)?);

    Ok(())
}
