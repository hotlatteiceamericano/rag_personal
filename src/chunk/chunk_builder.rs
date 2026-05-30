use crate::{chunk::Chunk, source::SourceDoc};

pub struct ChunkBuilder<'a> {
    doc: &'a SourceDoc,
    target_tokens: usize,
    chunks: Vec<Chunk>,
    buffer: String,
    ordinal: usize,
}

impl<'a> ChunkBuilder<'a> {
    pub fn new(doc: &'a SourceDoc, target_tokens: usize) -> Self {
        Self {
            doc,
            target_tokens,
            chunks: Vec::new(),
            buffer: String::new(),
            ordinal: 0,
        }
    }

    pub fn buffer_tokens(&self) -> usize {
        approx_tokens(&self.buffer)
    }

    pub fn append(&mut self, text: &str) {
        if !self.buffer.is_empty() {
            self.buffer.push('\n');
        }
        self.buffer.push_str(text);
    }

    pub fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.buffer);
        let chunk = self.make_chunk(text);
        self.chunks.push(chunk);
    }

    pub fn push_hard_split(&mut self, text: &str) {
        for piece in hard_split(text, self.target_tokens) {
            let chunk = self.make_chunk(piece);
            self.chunks.push(chunk);
        }
    }

    pub fn finish(mut self) -> Vec<Chunk> {
        self.flush();
        self.chunks
    }

    fn make_chunk(&mut self, text: String) -> Chunk {
        let chunk = Chunk {
            chunk_id: format!("{}#{}", self.doc.page_id, self.ordinal),
            page_id: self.doc.page_id.clone(),
            title: self.doc.title.clone(),
            url: self.doc.url.clone(),
            text,
        };
        self.ordinal += 1;
        chunk
    }
}

pub fn approx_tokens(text: &str) -> usize {
    let divisor = if text.chars().any(is_cjk) { 3 } else { 4 };
    text.chars().count().div_ceil(divisor)
}

fn hard_split(text: &str, target_tokens: usize) -> Vec<String> {
    let divisor = if text.chars().any(is_cjk) { 3 } else { 4 };
    let target_chars = target_tokens * divisor;

    let mut pieces: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_chars: usize = 0;

    for c in text.chars() {
        if current_chars >= target_chars {
            pieces.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        current.push(c);
        current_chars += 1;
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF)
}
