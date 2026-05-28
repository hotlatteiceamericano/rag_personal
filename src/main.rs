use clap::{Parser, Subcommand};
use rag_personal::{
    chunk::{Chunk, Chunker, structure::StructureChunker},
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

            let chunker = StructureChunker::new(config.chunk_target_tokens);
            let chunks: Vec<Chunk> = docs.iter().flat_map(|d| chunker.chunk(d)).collect();

            let (min, median, max) = chunk_char_stats(&chunks);
            info!(
                "chunked {} docs → {} chunks (chars min/median/max: {}/{}/{})",
                docs.len(),
                chunks.len(),
                min,
                median,
                max,
            );
        }
    }

    Ok(())
}

fn chunk_char_stats(chunks: &[Chunk]) -> (usize, usize, usize) {
    if chunks.is_empty() {
        return (0, 0, 0);
    }
    let mut lens: Vec<usize> = chunks.iter().map(|c| c.text.chars().count()).collect();
    lens.sort_unstable();
    (lens[0], lens[lens.len() / 2], lens[lens.len() - 1])
}
