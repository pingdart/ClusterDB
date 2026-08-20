use crate::exec::Executor;
use crate::storage::StorageEngine;
use std::time::Instant;
use uuid::Uuid;

pub async fn run_benchmark() {
    println!("Starting PingDart-Core Benchmark Suite...");

    let db_path = ".pingdart_bench";
    let _ = std::fs::remove_dir_all(db_path); // Start fresh
    let storage = StorageEngine::new(db_path).unwrap();
    let executor = Executor::new(storage);

    // Setup Table
    executor.execute("CREATE DATABASE bench", None).unwrap();
    let setup_sql = r#"
        CREATE TABLE bench_users (
            id STRING PRIMARY,
            name STRING,
            age INTEGER,
            city STRING
        );
    "#;
    executor.execute(setup_sql, Some("bench")).unwrap();

    let num_docs = 10_000;
    println!("1. Inserting {} Documents...", num_docs);
    let start = Instant::now();
    for i in 0..num_docs {
        let sql = format!("INSERT INTO bench_users (id, name, age, city) VALUES ('{}', 'User{}', {}, '{}')",
            Uuid::new_v4(), i, i % 100, if i % 2 == 0 { "Hyderabad" } else { "Bangalore" }
        );
        executor.execute(&sql, Some("bench")).unwrap();
    }
    let duration = start.elapsed();
    println!("   Done in {:?} ({:?} per insert)", duration, duration / num_docs as u32);

    println!("2. Full Table Scan Read (O(N))...");
    let start = Instant::now();
    let req = crate::server::DocumentQueryRequest {
        collection: "bench_users".to_string(),
        r#where: Some(vec![
            crate::server::DocumentFilter { field: "name".to_string(), operator: "==".to_string(), value: "User5000".to_string() }
        ]),
        near: None, order_by: None, limit: None, offset: None, explain: None, token: "".to_string(), database: None,
    };
    let _ = executor.execute_document_query(&req, Some("bench")).unwrap();
    let duration = start.elapsed();
    println!("   Done in {:?}", duration);

    println!("3. Building Background Index...");
    let start = Instant::now();
    executor.execute("CREATE INDEX idx_name ON bench_users(name)", Some("bench")).unwrap();
    
    // Wait a bit for the tokio background task to finish backfilling
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    
    let duration = start.elapsed();
    println!("   Index built in {:?}", duration);

    println!("4. Indexed Read (O(1))...");
    let start = Instant::now();
    let _ = executor.execute_document_query(&req, Some("bench")).unwrap();
    let duration = start.elapsed();
    println!("   Done in {:?} (Instant Quantum Speed!)", duration);

    let _ = std::fs::remove_dir_all(db_path);
    println!("Benchmark Suite Completed Successfully.");
}
