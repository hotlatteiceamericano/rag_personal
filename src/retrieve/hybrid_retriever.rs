use std::collections::HashMap;

use crate::embed::Embedder;
use crate::lexical::LexicalIndex;
use crate::store::{Hit, VectorStore};

use super::{DenseRetriever, LexicalRetriever, Retriever};

const DEFAULT_N_PER_LEG: usize = 50;
const DEFAULT_RRF_K: f32 = 60.0;

pub struct HybridRetriever<'a, E: Embedder, S: VectorStore, L: LexicalIndex> {
    dense: DenseRetriever<'a, E, S>,
    lexical: LexicalRetriever<'a, L>,
    n_per_leg: usize,
    rrf_k: f32,
}

impl<'a, E: Embedder, S: VectorStore, L: LexicalIndex> HybridRetriever<'a, E, S, L> {
    pub fn new(embedder: &'a E, store: &'a S, lexical: &'a L) -> Self {
        Self {
            dense: DenseRetriever::new(embedder, store),
            lexical: LexicalRetriever::new(lexical),
            n_per_leg: DEFAULT_N_PER_LEG,
            rrf_k: DEFAULT_RRF_K,
        }
    }
}

#[async_trait::async_trait]
impl<E: Embedder + Sync, S: VectorStore, L: LexicalIndex> Retriever
    for HybridRetriever<'_, E, S, L>
{
    async fn retrieve(&self, query: &str, top_k: usize) -> anyhow::Result<Vec<Hit>> {
        let dense_hits = self.dense.retrieve(query, self.n_per_leg).await?;
        let lexical_hits = self.lexical.retrieve(query, self.n_per_leg).await?;
        Ok(rrf_fuse(vec![dense_hits, lexical_hits], self.rrf_k, top_k))
    }
}

/// combining the result from both lexical and dense
/// higher the rank higher the priority
/// specifically: 1 / (RRF_K + rank) points from each list
fn rrf_fuse(lists: Vec<Vec<Hit>>, rrf_k: f32, top_k: usize) -> Vec<Hit> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    let mut keep: HashMap<String, Hit> = HashMap::new();

    for list in lists {
        for (index, hit) in list.into_iter().enumerate() {
            let rank = (index + 1) as f32;
            *scores.entry(hit.chunk_id.clone()).or_insert(0.0) += 1.0 / (rrf_k + rank);
            keep.entry(hit.chunk_id.clone()).or_insert(hit);
        }
    }

    let mut fused: Vec<Hit> = keep
        .into_iter()
        .map(|(id, mut h)| {
            h.score = scores[&id];
            h
        })
        .collect();

    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
    });

    fused.truncate(top_k);
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(chunk_id: &str) -> Hit {
        Hit {
            chunk_id: chunk_id.to_string(),
            page_id: "p".to_string(),
            title: "t".to_string(),
            url: "u".to_string(),
            text: "x".to_string(),
            score: 0.0,
        }
    }

    #[test]
    fn doc_in_both_lists_outranks_doc_in_one() {
        let dense = vec![hit("a"), hit("b"), hit("c")];
        let lexical = vec![hit("d"), hit("a"), hit("e")];

        let fused = rrf_fuse(vec![dense, lexical], 60.0, 5);
        assert_eq!(fused[0].chunk_id, "a", "a is in both, should rank first");
    }

    #[test]
    fn rrf_score_matches_formula() {
        // a: rank 1 in list1 + rank 2 in list2 => 1/61 + 1/62
        let dense = vec![hit("a"), hit("b")];
        let lexical = vec![hit("b"), hit("a")];

        let fused = rrf_fuse(vec![dense, lexical], 60.0, 2);
        let a = fused.iter().find(|h| h.chunk_id == "a").unwrap();
        let expected = 1.0 / 61.0 + 1.0 / 62.0;
        assert!(
            (a.score - expected).abs() < 1e-6,
            "got {}, want {}",
            a.score,
            expected
        );
    }

    #[test]
    fn tiebreak_is_stable_on_chunk_id() {
        // x and y appear at identical ranks → equal fused scores → tiebreak by chunk_id asc
        let l1 = vec![hit("y"), hit("x")];
        let l2 = vec![hit("y"), hit("x")];

        let fused = rrf_fuse(vec![l1, l2], 60.0, 2);
        assert_eq!(fused[0].chunk_id, "y", "y wins rank 1 in both");
        // For a true tie test:
        let m1 = vec![hit("zeta"), hit("alpha")];
        let m2 = vec![hit("alpha"), hit("zeta")];
        let fused = rrf_fuse(vec![m1, m2], 60.0, 2);
        // both got rank1+rank2 → identical score → alpha < zeta lexicographically wins
        assert_eq!(fused[0].chunk_id, "alpha");
        assert_eq!(fused[1].chunk_id, "zeta");
    }

    #[test]
    fn top_k_truncates() {
        let l1: Vec<Hit> = (0..10).map(|i| hit(&format!("c{i}"))).collect();
        let fused = rrf_fuse(vec![l1], 60.0, 3);
        assert_eq!(fused.len(), 3);
    }
}
