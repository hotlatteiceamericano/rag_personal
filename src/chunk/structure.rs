use crate::chunk::{Chunk, Chunker};
use crate::source::{BlockKind, SourceDoc};

pub struct StructureChunker {
    target_tokens: usize,
}

impl StructureChunker {
    pub fn new(target_tokens: usize) -> Self {
        Self { target_tokens }
    }
}

impl Chunker for StructureChunker {
    // fill the buffer with doc.block.text
    // when full, flush the buffer and convert it to chunks
    fn chunk(&self, doc: &SourceDoc) -> Vec<Chunk> {
        let mut chunks: Vec<Chunk> = Vec::new();
        let mut buffer = String::new();
        let mut ordinal: usize = 0;

        // using closure to capture external doc, any better way?
        let make_chunk = |text: String, ord: usize| Chunk {
            chunk_id: format!("{}#{}", doc.page_id, ord),
            page_id: doc.page_id.clone(),
            title: doc.title.clone(),
            url: doc.url.clone(),
            text,
        };

        for block in &doc.blocks {
            // flush immediately when seeing headings
            if block.kind.is_heading() && !buffer.is_empty() {
                chunks.push(make_chunk(std::mem::take(&mut buffer), ordinal));
                ordinal += 1;
            }

            let block_tokens = approx_tokens(&block.text);

            // first handle oversize chunk
            if block_tokens > self.target_tokens && !matches!(block.kind, BlockKind::Code) {
                if !buffer.is_empty() {
                    chunks.push(make_chunk(std::mem::take(&mut buffer), ordinal));
                    ordinal += 1;
                }
                for piece in hard_split(&block.text, self.target_tokens) {
                    chunks.push(make_chunk(piece, ordinal));
                    ordinal += 1;
                }
                continue;
            }

            // second handle current buffer + current block exceed size
            if !buffer.is_empty() && approx_tokens(&buffer) + block_tokens > self.target_tokens {
                chunks.push(make_chunk(std::mem::take(&mut buffer), ordinal));
                ordinal += 1;
            }

            if !buffer.is_empty() {
                buffer.push('\n');
            }

            // fill the buffer
            buffer.push_str(&block.text);
        }

        if !buffer.is_empty() {
            chunks.push(make_chunk(buffer, ordinal));
        }

        chunks
    }
}

fn approx_tokens(text: &str) -> usize {
    let divisor = if text.chars().any(is_cjk) { 3 } else { 4 };
    text.chars().count().div_ceil(divisor)
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF)
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
