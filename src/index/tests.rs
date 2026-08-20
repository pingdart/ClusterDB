#[cfg(test)]
mod tests {
    use crate::exec::Executor;
    use crate::storage::StorageEngine;

    #[tokio::test]
    async fn test_index_creation_and_query_planner() {
        let db_path = ".test_db";
        let _ = std::fs::remove_dir_all(db_path);
        
        let storage = StorageEngine::new(db_path).unwrap();
        let executor = Executor::new(storage.clone());

        // Setup test table and insert data
        executor.execute("CREATE DATABASE testdb", None).unwrap();
        executor.execute(r#"
            CREATE TABLE users (
                id STRING PRIMARY KEY,
                name STRING,
                age INTEGER
            );
        "#, Some("testdb")).unwrap();
        
        executor.execute("INSERT INTO users (id, name, age) VALUES ('u1', 'Alice', 30)", Some("testdb")).unwrap();
        executor.execute("INSERT INTO users (id, name, age) VALUES ('u2', 'Bob', 25)", Some("testdb")).unwrap();

        // Check EXPLAIN before index (should use CollectionScan)
        let explain_pre = executor.execute("EXPLAIN SELECT * FROM users WHERE name='Alice'", Some("testdb")).unwrap();
        assert!(explain_pre.contains("CollectionScan"), "Expected CollectionScan, got: {}", explain_pre);

        // Create Index
        executor.execute("CREATE INDEX idx_name ON users(name)", Some("testdb")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await; // wait for backfill

        // Check EXPLAIN after index (should use Single Secondary Index)
        let explain_post = executor.execute("EXPLAIN SELECT * FROM users WHERE name='Alice'", Some("testdb")).unwrap();
        assert!(explain_post.contains("name"));

        // Execute actual query and verify it hits index stats
        let req = crate::server::DocumentQueryRequest {
            collection: "users".to_string(),
            r#where: Some(vec![
                crate::server::DocumentFilter { field: "name".to_string(), operator: "==".to_string(), value: "Alice".to_string() }
            ]),
            near: None, order_by: None, limit: None, offset: None, explain: None, token: "".to_string(), database: None,
        };
        let _res = executor.execute_document_query(&req, Some("testdb")).unwrap();

        // Verify stats incremented
        let stats = executor.execute("SHOW INDEX STATS", Some("testdb")).unwrap();
        assert!(stats.contains("testdb_users_name"));

        let _ = std::fs::remove_dir_all(db_path);
    }
}
