use clap::{Parser, Subcommand};
use rag_personal::{
    config::Config,
    source::{Source, notion_source::NotionSource},
};
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

    let client = reqwest::Client::new();
    let config = Config::load()?;

    let cli = Cli::parse();
    match cli.command {
        Command::Ingest => {
            let source = NotionSource::new(client, config.notion_token, config.root_page_ids);
            let docs = source.fetch().await?;
            info!("fetched docs: {:#?}", docs);
        }
    }

    Ok(())
}
