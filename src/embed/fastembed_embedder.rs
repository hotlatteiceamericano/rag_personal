use std::sync::Mutex;

use anyhow::Context;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use super::{Embedder, passage_input, query_input};

const DIMENSION: usize = 384;

pub struct E5SmallEmbedder {
    model: Mutex<TextEmbedding>,
}

impl E5SmallEmbedder {
    pub fn new() -> anyhow::Result<Self> {
        let model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::MultilingualE5Small))
            .context("failed to initialize multilingual-e5-small")?;
        Ok(Self {
            model: Mutex::new(model),
        })
    }
}

impl Embedder for E5SmallEmbedder {
    fn embed_passages(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let prefixed: Vec<String> = texts.iter().map(|t| passage_input(t)).collect();
        let mut model = self.model.lock().expect("embedder mutex poisoned");

        model.embed(prefixed, None)
    }

    fn embed_query(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let prefixed = vec![query_input(text)];
        let mut model = self.model.lock().expect("embedder mutex poisoned");
        let mut out = model.embed(prefixed, None)?;

        Ok(out.pop().expect("one input yields one embedding"))
    }

    fn dimension(&self) -> usize {
        DIMENSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l2_norm(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    #[test]
    #[ignore = "downloads ~120MB model on first run; run with `cargo test -- --ignored`"]
    fn embeds_passage_with_correct_dim_and_norm() {
        let embedder = E5SmallEmbedder::new().expect("init model");
        let vectors = embedder
            .embed_passages(&["hello world".to_string()])
            .expect("embed");
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].len(), DIMENSION);
        let n = l2_norm(&vectors[0]);
        assert!((n - 1.0).abs() < 1e-3, "expected L2 norm ~1.0, got {n}");
    }

    #[test]
    #[ignore = "downloads ~120MB model on first run; run with `cargo test -- --ignored`"]
    fn embeds_query_with_correct_dim_and_norm() {
        let embedder = E5SmallEmbedder::new().expect("init model");
        let v = embedder.embed_query("what is RAG?").expect("embed");
        assert_eq!(v.len(), DIMENSION);
        let n = l2_norm(&v);
        assert!((n - 1.0).abs() < 1e-3, "expected L2 norm ~1.0, got {n}");
    }
}
