use forum_reader_lib::test_exports::{open_test_pool, schema_table_names};

#[tokio::test]
async fn schema_creates_expected_tables() {
    let pool = open_test_pool().await.unwrap();
    let tables = schema_table_names(&pool).await.unwrap();
    for required in ["categories", "forums", "threads"] {
        assert!(
            tables.iter().any(|t| t == required),
            "missing table {} (have: {:?})",
            required,
            tables
        );
    }
}
