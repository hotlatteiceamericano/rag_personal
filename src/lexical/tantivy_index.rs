use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, anyhow};
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::{Index, IndexReader, TantivyDocument, Term, doc};

use super::LexicalIndex;
use crate::chunk::Chunk;
use crate::store::Hit;

const TOKENIZER_NAME: &str = "cjk";
const WRITER_HEAP_BYTES: usize = 50_000_000;

pub struct TantivyIndex {
    index: Index,
    reader: IndexReader,
    fields: Fields,
}

#[derive(Clone, Copy)]
struct Fields {
    chunk_id: Field,
    page_id: Field,
    title: Field,
    url: Field,
    text: Field,
}

impl TantivyIndex {
    pub fn open_or_create(path: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(path).context("creating tantivy dir")?;
        let (schema, fields) = build_schema();
        let dir = MmapDirectory::open(path).context("opening tantivy mmap dir")?;
        let index = Index::open_or_create(dir, schema).context("open_or_create tantivy index")?;
        Self::from_index(index, fields)
    }

    fn from_index(index: Index, fields: Fields) -> anyhow::Result<Self> {
        index
            .tokenizers()
            .register(TOKENIZER_NAME, tantivy_jieba::JiebaTokenizer::new());
        let reader = index.reader().context("building tantivy reader")?;
        Ok(Self {
            index,
            reader,
            fields,
        })
    }
}

impl LexicalIndex for TantivyIndex {
    fn upsert(&self, chunks: &[Chunk]) -> anyhow::Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        let mut writer = self
            .index
            .writer(WRITER_HEAP_BYTES)
            .context("creating tantivy writer")?;

        let mut seen_pages: BTreeSet<&str> = BTreeSet::new();
        for c in chunks {
            if seen_pages.insert(c.page_id.as_str()) {
                writer.delete_term(Term::from_field_text(self.fields.page_id, &c.page_id));
            }
        }

        for c in chunks {
            writer
                .add_document(doc!(
                    self.fields.chunk_id => c.chunk_id.as_str(),
                    self.fields.page_id  => c.page_id.as_str(),
                    self.fields.title    => c.title.as_str(),
                    self.fields.url      => c.url.as_str(),
                    self.fields.text     => c.text.as_str(),
                ))
                .context("tantivy add_document")?;
        }

        writer.commit().context("tantivy commit")?;
        self.reader.reload().context("tantivy reader reload")?;
        Ok(())
    }

    fn search(&self, query: &str, k: usize) -> anyhow::Result<Vec<Hit>> {
        let searcher = self.reader.searcher();
        let qp = QueryParser::for_index(&self.index, vec![self.fields.text]);
        let (q, errors) = qp.parse_query_lenient(query);
        if !errors.is_empty() {
            tracing::debug!(?errors, query, "tantivy lenient parse: ignored tokens");
        }
        let top = searcher
            .search(&q, &TopDocs::with_limit(k).order_by_score())
            .context("tantivy search")?;

        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr).context("loading doc")?;
            hits.push(Hit {
                chunk_id: text_field(&doc, self.fields.chunk_id, "chunk_id")?,
                page_id: text_field(&doc, self.fields.page_id, "page_id")?,
                title: text_field(&doc, self.fields.title, "title")?,
                url: text_field(&doc, self.fields.url, "url")?,
                text: text_field(&doc, self.fields.text, "text")?,
                score,
            });
        }
        Ok(hits)
    }
}

fn build_schema() -> (Schema, Fields) {
    let mut sb = Schema::builder();
    let text_opts = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(TOKENIZER_NAME)
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();

    let chunk_id = sb.add_text_field("chunk_id", STRING | STORED);
    let page_id = sb.add_text_field("page_id", STRING | STORED);
    let title = sb.add_text_field("title", STORED);
    let url = sb.add_text_field("url", STORED);
    let text = sb.add_text_field("text", text_opts);

    (
        sb.build(),
        Fields {
            chunk_id,
            page_id,
            title,
            url,
            text,
        },
    )
}

fn text_field(doc: &TantivyDocument, field: Field, name: &str) -> anyhow::Result<String> {
    doc.get_first(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("missing stored text field: {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &str, page: &str, text: &str) -> Chunk {
        Chunk {
            chunk_id: id.to_string(),
            page_id: page.to_string(),
            title: format!("title-{page}"),
            url: format!("https://example/{page}"),
            text: text.to_string(),
        }
    }

    fn in_ram_index() -> TantivyIndex {
        let (schema, fields) = build_schema();
        let index = Index::create_in_ram(schema);
        TantivyIndex::from_index(index, fields).unwrap()
    }

    #[test]
    fn bm25_finds_english_match() {
        let idx = in_ram_index();
        idx.upsert(&[
            chunk(
                "a#0",
                "a",
                "RAG pipelines combine retrieval with generation.",
            ),
            chunk("b#0", "b", "The cat sat on the mat."),
        ])
        .unwrap();

        let hits = idx.search("retrieval generation", 5).unwrap();
        assert!(!hits.is_empty(), "expected at least one hit");
        assert_eq!(hits[0].chunk_id, "a#0", "RAG chunk should rank first");
    }

    #[test]
    fn bm25_finds_chinese_match_via_jieba() {
        let idx = in_ram_index();
        idx.upsert(&[
            chunk("zh#0", "zh", "將外部知識注入大型語言模型"),
            chunk(
                "en#0",
                "en",
                "Reciprocal rank fusion blends two ranked lists.",
            ),
        ])
        .unwrap();

        let hits = idx.search("外部知識", 5).unwrap();
        assert!(
            !hits.is_empty(),
            "expected Chinese tokenizer to segment 外部知識"
        );
        assert_eq!(hits[0].chunk_id, "zh#0");
    }

    #[test]
    fn delete_by_page_replaces_old_chunks() {
        let idx = in_ram_index();
        idx.upsert(&[chunk("p#0", "p", "alpha bravo charlie")])
            .unwrap();
        // re-ingest the same page with different text — old row must be evicted
        idx.upsert(&[chunk("p#0", "p", "delta echo foxtrot")])
            .unwrap();

        let stale = idx.search("alpha", 5).unwrap();
        assert!(stale.is_empty(), "old text should be deleted by page_id");
        let fresh = idx.search("delta", 5).unwrap();
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].chunk_id, "p#0");
    }
}
