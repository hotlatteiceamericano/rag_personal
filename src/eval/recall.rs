use std::fmt;

use crate::{
    eval::GoldEntry,
    retrieve::{RetrievalMode, Retriever},
    store::Hit,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitOutcome {
    Hit,
    PageMiss,
    SpanMiss,
}

pub struct QueryOutcome {
    pub query: String,
    pub outcome: HitOutcome,
    pub top_hits: Vec<Hit>,
    pub used_loose_rule: bool,
}

pub struct ModeResult {
    pub mode: RetrievalMode,
    pub total: usize,
    pub hits: usize,
    pub per_query: Vec<QueryOutcome>,
}

impl ModeResult {
    pub fn recall(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.hits as f32 / self.total as f32
        }
    }

    pub fn count(&self, target: HitOutcome) -> usize {
        self.per_query
            .iter()
            .filter(|q| q.outcome == target)
            .count()
    }
}

fn evaluate(entry: &GoldEntry, hits: &[Hit]) -> HitOutcome {
    let page_found = hits
        .iter()
        .any(|h| entry.relevant_page_ids.contains(&h.page_id));

    match &entry.answer_span {
        Some(span) => {
            let span_lower = span.to_lowercase();
            let span_found_on_relevant_page = hits.iter().any(|h| {
                entry.relevant_page_ids.contains(&h.page_id)
                    && h.text.to_lowercase().contains(&span_lower)
            });

            if span_found_on_relevant_page {
                HitOutcome::Hit
            } else if page_found {
                HitOutcome::SpanMiss
            } else {
                HitOutcome::PageMiss
            }
        }
        None => {
            if page_found {
                HitOutcome::Hit
            } else {
                HitOutcome::PageMiss
            }
        }
    }
}

// todo: implement eval method on each retriever
pub async fn run_mode(
    retriever: &dyn Retriever,
    mode: RetrievalMode,
    gold: &[GoldEntry],
    top_k: usize,
) -> anyhow::Result<ModeResult> {
    let mut per_query = Vec::with_capacity(gold.len());
    let mut hits = 0;
    for entry in gold {
        let top = retriever.retrieve(&entry.q, top_k).await?;

        let outcome = evaluate(entry, &top);
        if matches!(outcome, HitOutcome::Hit) {
            hits += 1;
        }

        per_query.push(QueryOutcome {
            query: entry.q.clone(),
            outcome,
            top_hits: top,
            used_loose_rule: entry.answer_span.is_none(),
        });
    }
    Ok(ModeResult {
        mode,
        total: gold.len(),
        hits,
        per_query,
    })
}

struct ModeName<'a>(&'a RetrievalMode);

impl fmt::Display for ModeName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            RetrievalMode::Dense => "Dense",
            RetrievalMode::Lexical => "Lexical",
            RetrievalMode::Hybrid => "Hybrid",
        })
    }
}

pub fn render_table(results: &[ModeResult]) -> String {
    let mut s = String::new();
    s.push_str("| Mode    | Hits | Total | Recall@5 | PageMiss | SpanMiss |\n");
    s.push_str("|---------|------|-------|----------|----------|----------|\n");
    for r in results {
        s.push_str(&format!(
            "| {:<7} | {:>4} | {:>5} | {:>8.3} | {:>8} | {:>8} |\n",
            ModeName(&r.mode),
            r.hits,
            r.total,
            r.recall(),
            r.count(HitOutcome::PageMiss),
            r.count(HitOutcome::SpanMiss),
        ));
    }
    s
}
