use crate::chunk::chunk_builder::{ChunkBuilder, approx_tokens};
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
    fn chunk(&self, doc: &SourceDoc) -> Vec<Chunk> {
        let mut builder = ChunkBuilder::new(doc, self.target_tokens);

        for block in &doc.blocks {
            if block.kind.is_heading() {
                builder.flush();
            }

            let block_tokens = approx_tokens(&block.text);

            if block_tokens > self.target_tokens && !matches!(block.kind, BlockKind::Code) {
                builder.flush();
                builder.push_hard_split(&block.text);
                continue;
            }

            // flush when current buffer + this block would exceed target
            if builder.buffer_tokens() + block_tokens > self.target_tokens {
                builder.flush();
            }

            builder.append(&block.text);
        }

        builder.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{BlockKind, SourceDoc, TextBlock};

    fn doc(blocks: Vec<TextBlock>) -> SourceDoc {
        SourceDoc {
            page_id: "page-1".to_string(),
            title: "Test Page".to_string(),
            url: "https://notion.so/page-1".to_string(),
            blocks,
        }
    }

    fn paragraph(text: &str) -> TextBlock {
        TextBlock {
            heading_path: vec![],
            text: text.to_string(),
            kind: BlockKind::Paragraph,
        }
    }

    fn heading(level: u8, text: &str) -> TextBlock {
        TextBlock {
            heading_path: vec![],
            text: text.to_string(),
            kind: BlockKind::Heading(level),
        }
    }

    fn code(text: &str) -> TextBlock {
        TextBlock {
            heading_path: vec![],
            text: text.to_string(),
            kind: BlockKind::Code,
        }
    }

    #[test]
    fn single_paragraph_produces_one_chunk() {
        let d = doc(vec![paragraph("hello world")]);
        let chunks = StructureChunker::new(384).chunk(&d);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_id, "page-1#0");
        assert_eq!(chunks[0].page_id, "page-1");
        assert_eq!(chunks[0].title, "Test Page");
        assert_eq!(chunks[0].url, "https://notion.so/page-1");
        assert_eq!(chunks[0].text, "hello world");
    }

    #[test]
    fn multiple_paragraphs_split_when_target_exceeded() {
        // each token can roughly have 4 chars
        // 30 words should take less then 10 tokens
        // resulting each chunk contains only each paragraph
        let d = doc(vec![
            paragraph(&"a".repeat(30)),
            paragraph(&"b".repeat(30)),
            paragraph(&"c".repeat(30)),
        ]);
        let chunks = StructureChunker::new(10).chunk(&d);

        assert_eq!(chunks.len(), 3);
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.chunk_id, format!("page-1#{i}"));
        }
    }

    #[test]
    fn heading_forces_flush() {
        // Plenty of room — only the heading triggers the split.
        let d = doc(vec![
            paragraph("intro para"),
            heading(2, "Section A"),
            paragraph("section body"),
        ]);
        let chunks = StructureChunker::new(384).chunk(&d);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "intro para");
        assert_eq!(chunks[1].text, "Section A\nsection body");
    }

    #[test]
    fn no_overlap_between_chunks() {
        // Forces an overflow flush between the two paragraphs; assert the
        // a's and b's land in different chunks with no carry-over.
        let d = doc(vec![paragraph(&"a".repeat(30)), paragraph(&"b".repeat(30))]);
        let chunks = StructureChunker::new(10).chunk(&d);

        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.chars().all(|c| c == 'a'));
        assert!(chunks[1].text.chars().all(|c| c == 'b'));
    }

    #[test]
    fn cjk_text_preserves_utf8() {
        // 6 chars × 20 = 120 CJK chars. CJK divisor 3 ⇒ ≈40 tokens.
        // Target 10 tokens ⇒ hard_split target_chars = 30 ⇒ 4 pieces.
        let cjk = "中文測試文字".repeat(20);
        let d = doc(vec![paragraph(&cjk)]);
        let chunks = StructureChunker::new(10).chunk(&d);

        assert!(chunks.len() > 1, "expected hard-split");
        let joined: String = chunks.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(joined, cjk, "hard_split must not lose or garble CJK chars");
    }

    #[test]
    fn oversized_paragraph_is_hard_split() {
        let big = "x".repeat(150); // ≈38 Latin tokens, target 10 ⇒ multi-piece
        let d = doc(vec![paragraph(&big)]);
        let chunks = StructureChunker::new(10).chunk(&d);

        assert!(chunks.len() > 1);
        let joined: String = chunks.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(joined, big);
    }

    #[test]
    fn oversized_code_block_emitted_whole() {
        let big = "fn main() {}\n".repeat(50);
        let d = doc(vec![code(&big)]);
        let chunks = StructureChunker::new(10).chunk(&d);

        assert_eq!(chunks.len(), 1, "code block must not be split");
        assert_eq!(chunks[0].text, big);
    }
}
