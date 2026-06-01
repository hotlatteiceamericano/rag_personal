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
    Inspect {
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        stats: bool,
        #[arg(long)]
        page_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
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
        Command::Inspect {
            limit,
            stats,
            page_id,
            json,
        } => {
            let store = LanceStore::connect(&config.db_path).await?;

            if stats {
                let rows = store.row_count().await?;
                let pages = store.page_count().await?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({ "rows": rows, "pages": pages })
                    );
                } else {
                    println!("Total rows:   {rows}");
                    println!("Unique pages: {pages}");
                }
                return Ok(());
            }

            let rows = store.scan(limit, page_id.as_deref()).await?;

            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                for (i, r) in rows.iter().enumerate() {
                    let preview: String = r.text.chars().take(200).collect();
                    let ellipsis = if r.text.chars().count() > 200 { "…" } else { "" };
                    println!(
                        "[{}] chunk_id={}  page={}  url={}",
                        i + 1,
                        r.chunk_id,
                        r.title,
                        r.url,
                    );
                    println!("    {preview}{ellipsis}");
                    println!();
                }
                println!("Showing {} row(s).", rows.len());
            }
        }
    }

    Ok(())
}
