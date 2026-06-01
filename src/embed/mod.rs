pub mod fastembed_embedder;

pub trait Embedder {
    fn embed_passages(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
    fn embed_query(&self, text: &str) -> anyhow::Result<Vec<f32>>;
    fn dimension(&self) -> usize;
}

pub(crate) fn passage_input(text: &str) -> String {
    format!("passage: {text}")
}

pub(crate) fn query_input(text: &str) -> String {
    format!("query: {text}")
}

#[cfg(test)]
mod tests {
    use super::{passage_input, query_input};

    #[test]
    fn passage_input_applies_passage_prefix() {
        assert_eq!(passage_input("hello"), "passage: hello");
    }

    #[test]
    fn query_input_applies_query_prefix() {
        assert_eq!(query_input("what is RAG?"), "query: what is RAG?");
    }
}
