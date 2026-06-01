use clap::{Parser, Subcommand};
use rag_personal::{
    chunk::structure::StructureChunker,
    config::Config,
    embed::fastembed_embedder::E5SmallEmbedder,
    pipeline,
    source::notion_source::NotionSource,
    store::lance_store::LanceStore,
};
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

    let config = Config::load()?;
    let cli = Cli::parse();

    match cli.command {
        Command::Ingest => {
            let client = reqwest::Client::new();
            let source = NotionSource::new(client, config.notion_token, config.root_page_ids);
            let chunker = StructureChunker::new(config.chunk_target_tokens);
            let embedder = E5SmallEmbedder::new()?;
            let store = LanceStore::connect(&config.db_path).await?;

            pipeline::ingest(&source, &chunker, &embedder, &store).await?;
        }
    }

    Ok(())
}
