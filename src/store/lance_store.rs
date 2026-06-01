use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, anyhow};
use arrow::array::{
    Array, ArrayRef, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator,
    StringArray,
};
use arrow::datatypes::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::Connection;
use lancedb::DistanceType;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::table::Table;

use super::{EmbeddedChunk, Hit, StoredRow, VectorStore};

const TABLE_NAME: &str = "notes";
const DIMENSION: i32 = 384;

pub struct LanceStore {
    table: Table,
    schema: Arc<Schema>,
}

impl LanceStore {
    pub async fn connect(db_path: &Path) -> anyhow::Result<Self> {
        let uri = db_path
            .to_str()
            .ok_or_else(|| anyhow!("db_path is not valid UTF-8"))?;
        let conn: Connection = lancedb::connect(uri)
            .execute()
            .await
            .context("opening LanceDB connection")?;

        let schema = notes_schema();
        let table = match conn.open_table(TABLE_NAME).execute().await {
            Ok(t) => t,
            Err(_) => conn
                .create_empty_table(TABLE_NAME, schema.clone())
                .execute()
                .await
                .context("creating LanceDB table")?,
        };

        Ok(Self { table, schema })
    }

    pub async fn scan(
        &self,
        limit: usize,
        page_id: Option<&str>,
    ) -> anyhow::Result<Vec<StoredRow>> {
        let mut q = self.table.query().limit(limit);
        if let Some(pid) = page_id {
            let escaped = pid.replace('\'', "''");
            q = q.only_if(format!("page_id = '{}'", escaped));
        }

        let stream = q.execute().await.context("scanning LanceDB table")?;
        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .context("collecting scan stream")?;

        let mut rows = Vec::new();
        for batch in batches {
            let chunk_ids = downcast_string(&batch, "chunk_id")?;
            let page_ids = downcast_string(&batch, "page_id")?;
            let titles = downcast_string(&batch, "title")?;
            let urls = downcast_string(&batch, "url")?;
            let texts = downcast_string(&batch, "text")?;

            for i in 0..batch.num_rows() {
                rows.push(StoredRow {
                    chunk_id: chunk_ids.value(i).to_string(),
                    page_id: page_ids.value(i).to_string(),
                    title: titles.value(i).to_string(),
                    url: urls.value(i).to_string(),
                    text: texts.value(i).to_string(),
                });
            }
        }
        Ok(rows)
    }

    pub async fn row_count(&self) -> anyhow::Result<usize> {
        self.table
            .count_rows(None)
            .await
            .context("counting LanceDB rows")
    }

    pub async fn page_count(&self) -> anyhow::Result<usize> {
        let stream = self
            .table
            .query()
            .select(lancedb::query::Select::Columns(vec!["page_id".into()]))
            .execute()
            .await
            .context("scanning page_id column")?;

        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .context("collecting page_id stream")?;

        let mut pages: BTreeSet<String> = BTreeSet::new();
        for batch in batches {
            let page_ids = downcast_string(&batch, "page_id")?;
            for i in 0..batch.num_rows() {
                pages.insert(page_ids.value(i).to_string());
            }
        }
        Ok(pages.len())
    }

    fn build_batch(&self, rows: &[EmbeddedChunk]) -> anyhow::Result<RecordBatch> {
        let chunk_ids = StringArray::from_iter_values(rows.iter().map(|r| r.chunk_id.as_str()));
        let page_ids = StringArray::from_iter_values(rows.iter().map(|r| r.page_id.as_str()));
        let titles = StringArray::from_iter_values(rows.iter().map(|r| r.title.as_str()));
        let urls = StringArray::from_iter_values(rows.iter().map(|r| r.url.as_str()));
        let texts = StringArray::from_iter_values(rows.iter().map(|r| r.text.as_str()));

        let flat: Vec<f32> = rows
            .iter()
            .flat_map(|r| r.vector.iter().copied())
            .collect();
        let inner = Float32Array::from(flat);
        let item_field = Arc::new(Field::new("item", DataType::Float32, true));
        let vectors = FixedSizeListArray::new(item_field, DIMENSION, Arc::new(inner), None);

        let columns: Vec<ArrayRef> = vec![
            Arc::new(chunk_ids),
            Arc::new(page_ids),
            Arc::new(titles),
            Arc::new(urls),
            Arc::new(texts),
            Arc::new(vectors),
        ];
        RecordBatch::try_new(self.schema.clone(), columns).context("building Arrow RecordBatch")
    }
}

#[async_trait::async_trait]
impl VectorStore for LanceStore {
    async fn upsert(&self, rows: Vec<EmbeddedChunk>) -> anyhow::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        for row in &rows {
            if row.vector.len() != DIMENSION as usize {
                return Err(anyhow!(
                    "chunk {} vector has {} dims, expected {DIMENSION}",
                    row.chunk_id,
                    row.vector.len()
                ));
            }
        }

        let page_ids: BTreeSet<&str> = rows.iter().map(|r| r.page_id.as_str()).collect();
        let page_filter = format!(
            "page_id IN ({})",
            page_ids
                .iter()
                .map(|id| format!("'{}'", id.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let batch = self.build_batch(&rows)?;
        let reader = RecordBatchIterator::new(vec![Ok(batch)], self.schema.clone());

        let mut mi = self.table.merge_insert(&["chunk_id"]);
        mi.when_matched_update_all(None)
            .when_not_matched_insert_all()
            .when_not_matched_by_source_delete(Some(page_filter));
        mi.execute(Box::new(reader))
            .await
            .context("merge_insert into LanceDB")?;
        Ok(())
    }

    async fn search(&self, query_vec: &[f32], k: usize) -> anyhow::Result<Vec<Hit>> {
        let stream = self
            .table
            .query()
            .nearest_to(query_vec)
            .context("LanceDB query: nearest_to rejected vector")?
            .distance_type(DistanceType::Cosine)
            .limit(k)
            .execute()
            .await
            .context("executing LanceDB vector query")?;

        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .context("collecting LanceDB result stream")?;

        let mut hits = Vec::with_capacity(k);
        for batch in batches {
            let chunk_ids = downcast_string(&batch, "chunk_id")?;
            let page_ids = downcast_string(&batch, "page_id")?;
            let titles = downcast_string(&batch, "title")?;
            let urls = downcast_string(&batch, "url")?;
            let texts = downcast_string(&batch, "text")?;
            let distances = downcast_f32(&batch, "_distance")?;

            for i in 0..batch.num_rows() {
                hits.push(Hit {
                    chunk_id: chunk_ids.value(i).to_string(),
                    page_id: page_ids.value(i).to_string(),
                    title: titles.value(i).to_string(),
                    url: urls.value(i).to_string(),
                    text: texts.value(i).to_string(),
                    score: 1.0 - distances.value(i),
                });
            }
        }
        Ok(hits)
    }

    async fn delete_page(&self, page_id: &str) -> anyhow::Result<()> {
        let escaped = page_id.replace('\'', "''");
        let predicate = format!("page_id = '{}'", escaped);
        self.table
            .delete(predicate.as_str())
            .await
            .context("LanceDB delete by page_id")?;
        Ok(())
    }
}

fn notes_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("chunk_id", DataType::Utf8, false),
        Field::new("page_id", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("url", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                DIMENSION,
            ),
            false,
        ),
    ]))
}

fn downcast_string<'a>(batch: &'a RecordBatch, name: &str) -> anyhow::Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .ok_or_else(|| anyhow!("missing column {name}"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| anyhow!("column {name} is not Utf8"))
}

fn downcast_f32<'a>(batch: &'a RecordBatch, name: &str) -> anyhow::Result<&'a Float32Array> {
    batch
        .column_by_name(name)
        .ok_or_else(|| anyhow!("missing column {name}"))?
        .as_any()
        .downcast_ref::<Float32Array>()
        .ok_or_else(|| anyhow!("column {name} is not Float32"))
}
