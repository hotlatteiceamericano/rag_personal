use crate::source::SourceDoc;

mod chunk_builder;
pub mod structure;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub chunk_id: String,
    pub page_id: String,
    pub title: String,
    pub url: String,
    pub text: String,
}

pub trait Chunker {
    fn chunk(&self, doc: &SourceDoc) -> Vec<Chunk>;
}
