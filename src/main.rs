use std::env;

use anyhow::Context;
use clap::{Parser, Subcommand};
use rag_personal::source::{Source, notion::NotionSource};
use serde_json::Value;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "rag_personal", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Ingest,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Ingest => {
            let source = NotionSource::new();
            let docs = source.fetch().await?;
            info!(count = docs.len(), "fetched documents");
        }
    }

    Ok(())
}
