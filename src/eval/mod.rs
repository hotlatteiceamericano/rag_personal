pub mod recall;

use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use serde::Deserialize;

/// One entry in eval/gold.jsonl. See design §8.1.
///
/// `answer_span` is optional: when present, a hit requires the retrieved chunk's
/// text to contain it (chunk-level relevance). When absent, we fall back to
/// page-level relevance and the report counts this entry as "loose-rule".
#[derive(Debug, Deserialize)]
pub struct GoldEntry {
    pub q: String,
    pub relevant_page_ids: Vec<String>,
    #[serde(default)]
    pub answer_span: Option<String>,
}

/// Parse a JSONL file (one GoldEntry per line). Blank lines are skipped so the
/// file can be edited by hand without breaking the parser.
pub fn load_gold(path: &Path) -> anyhow::Result<Vec<GoldEntry>> {
    let file = File::open(path)
        .map_err(|e| anyhow::anyhow!("failed to open gold set at {}: {e}", path.display()))?;
    let reader = BufReader::new(file);

    let mut entries = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: GoldEntry = serde_json::from_str(trimmed)
            .map_err(|e| anyhow::anyhow!("gold.jsonl line {}: {e}", i + 1))?;
        entries.push(entry);
    }
    Ok(entries)
}
