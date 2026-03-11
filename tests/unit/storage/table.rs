use anyhow::Result;
use bendclaw::storage::sql::SqlVal;
use bendclaw::storage::table::DatabendTable;
use bendclaw::storage::table::RowMapper;
use bendclaw::storage::table::Where;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

#[derive(Clone)]
struct TextMapper;

impl RowMapper for TextMapper {
    type Entity = String;

    fn columns(&self) -> &str {
        "value"
    }

    fn parse(&self, row: &serde_json::Value) -> bendclaw::base::Result<Self::Entity> {
        Ok(row[0].as_str().unwrap_or_default().to_string())
    }
}

#[tokio::test]
async fn table_get_builds_scoped_select_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, database| {
        assert_eq!(database, None);
        assert_eq!(sql, "SELECT value FROM demo WHERE id = 'row-1' LIMIT 1");
        Ok(paged_rows(&[&["value-1"]], None, None))
    });
    let table = DatabendTable::new(fake.pool(), "demo", TextMapper);

    let value = table
        .get(&[Where("id", SqlVal::Str("row-1"))])
        .await?
        .expect("row should exist");

    assert_eq!(value, "value-1");
    assert_eq!(fake.calls(), vec![FakeDatabendCall::Query {
        sql: "SELECT value FROM demo WHERE id = 'row-1' LIMIT 1".to_string(),
        database: None,
    }]);
    Ok(())
}

#[tokio::test]
async fn table_insert_batch_skips_empty_rows() -> Result<()> {
    let fake = FakeDatabend::new(|_sql, _database| {
        panic!("empty batch should not issue any query");
    });
    let table = DatabendTable::new(fake.pool(), "demo", TextMapper);

    table.insert_batch(&["value"], &[]).await?;

    assert!(fake.calls().is_empty());
    Ok(())
}
