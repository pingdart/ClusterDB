pub mod checkpoint;
pub mod mapping;
pub mod source;
pub mod target;
pub mod validator;
pub mod job;

pub use checkpoint::MigrationCheckpoint;
pub use mapping::{EmbeddingConfig, MigrationMapping, SchemaPolicy};
pub use source::{CsvSource, JsonSource, MigrationSource, MongoJsonSource, MySQLDumpSource, PostgresDumpSource};
pub use target::{FileExportTarget, MigrationTarget, PingDartTarget};
pub use validator::{MigrationValidator, ValidationReport};
pub use job::{MigrationConfig, MigrationJob, MigrationManager, MigrationProgress, MigrationStatus};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageEngine;
    use std::collections::HashMap;
    use std::io::Write;

    #[test]
    fn test_csv_source_and_mapping() -> anyhow::Result<()> {
        let path = std::env::temp_dir().join(format!("test_csv_{}.csv", uuid::Uuid::new_v4()));
        {
            let mut file = std::fs::File::create(&path)?;
            writeln!(file, "id,full_name,email,score")?;
            writeln!(file, "101,\"Alice Smith\",alice@test.com,98")?;
            writeln!(file, "102,\"Bob Jones\",bob@test.com,85")?;
            file.flush()?;
        }

        let mut source = CsvSource::new(&path, None)?;
        let batch = source.next_batch(10)?;
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].get("full_name").unwrap().as_str().unwrap(), "Alice Smith");

        let mut mapping = MigrationMapping::default();
        mapping.column_renames.insert("full_name".to_string(), "name".to_string());
        mapping.embedding_configs.push(EmbeddingConfig {
            source_column: "name".to_string(),
            vector_column: "embedding".to_string(),
            dimensions: 16,
        });

        let transformed = mapping.transform_record(&batch[0], None, None)?;
        assert_eq!(transformed.get("name").unwrap(), "Alice Smith");
        assert!(transformed.contains_key("embedding"));
        assert!(transformed.get("embedding").unwrap().starts_with('['));

        let _ = std::fs::remove_file(path);
        Ok(())
    }

    #[test]
    fn test_mongo_extended_json_normalization() {
        let mut raw = HashMap::new();
        let mut oid_map = serde_json::Map::new();
        oid_map.insert("$oid".to_string(), serde_json::Value::String("64b0f001".to_string()));
        raw.insert("_id".to_string(), serde_json::Value::Object(oid_map));

        let mut date_map = serde_json::Map::new();
        date_map.insert("$date".to_string(), serde_json::Value::String("2026-09-11T00:00:00Z".to_string()));
        raw.insert("created_at".to_string(), serde_json::Value::Object(date_map));

        let norm = MongoJsonSource::normalize_mongo_record(raw);
        assert_eq!(norm.get("id").unwrap().as_str().unwrap(), "64b0f001");
        assert_eq!(norm.get("created_at").unwrap().as_str().unwrap(), "2026-09-11T00:00:00Z");
    }

    #[test]
    fn test_mysql_dump_parsing() -> anyhow::Result<()> {
        let path = std::env::temp_dir().join(format!("test_mysql_{}.sql", uuid::Uuid::new_v4()));
        {
            let mut file = std::fs::File::create(&path)?;
            writeln!(file, "-- MySQL dump")?;
            writeln!(file, "INSERT INTO `users` (`id`, `username`, `email`) VALUES (1, 'alice', 'alice@mysql.com'), (2, 'bob', 'bob@mysql.com');")?;
            file.flush()?;
        }

        let mut source = MySQLDumpSource::new(&path)?;
        let batch = source.next_batch(10)?;
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].get("username").unwrap().as_str().unwrap(), "alice");
        assert_eq!(batch[1].get("email").unwrap().as_str().unwrap(), "bob@mysql.com");

        let _ = std::fs::remove_file(path);
        Ok(())
    }

    #[test]
    fn test_checkpoint_persistence() -> anyhow::Result<()> {
        let storage = StorageEngine::new_temp()?;

        let mut cp = MigrationCheckpoint::new("job-alpha-123".to_string());
        cp.batch_number = 42;
        cp.records_committed = 21000;
        cp.source_offset = 1048576;
        cp.save(&storage)?;

        let loaded = MigrationCheckpoint::load(&storage, "job-alpha-123")?.expect("Checkpoint should exist");
        assert_eq!(loaded.batch_number, 42);
        assert_eq!(loaded.records_committed, 21000);
        assert_eq!(loaded.source_offset, 1048576);

        Ok(())
    }

    #[test]
    fn test_migration_validator() -> anyhow::Result<()> {
        let storage = StorageEngine::new_temp()?;

        let row1 = crate::exec::RowData {
            _id: "1".to_string(),
            columns: [("id".to_string(), "1".to_string()), ("name".to_string(), "Alice".to_string())].into_iter().collect(),
            ..Default::default()
        };
        let row2 = crate::exec::RowData {
            _id: "2".to_string(),
            columns: [("id".to_string(), "2".to_string()), ("name".to_string(), "Bob".to_string())].into_iter().collect(),
            ..Default::default()
        };

        storage.set("test_table", "1", &row1)?;
        storage.set("test_table", "2", &row2)?;
        storage.flush()?;

        let report = MigrationValidator::validate("job-v-1", &storage, "test_table", 2, "", Some("id"), &["name".to_string()])?;
        assert!(report.is_valid);
        assert_eq!(report.source_rows, 2);
        assert_eq!(report.target_rows, 2);
        assert_eq!(report.null_violations_count, 0);
        assert_eq!(report.pk_violations_count, 0);

        Ok(())
    }

    #[test]
    fn test_firebase_and_json_source() -> anyhow::Result<()> {
        let path = std::env::temp_dir().join(format!("test_firebase_{}.json", uuid::Uuid::new_v4()));
        {
            let mut file = std::fs::File::create(&path)?;
            writeln!(file, r#"{{
                "user_abc": {{ "name": "Charlie", "email": "charlie@firebase.com", "created_at": {{ "_seconds": 1728000000, "_nanoseconds": 0 }} }},
                "user_xyz": {{ "name": "Dana", "email": "dana@firebase.com", "created_at": {{ "_seconds": 1728000100, "_nanoseconds": 0 }} }}
            }}"#)?;
            file.flush()?;
        }

        let mut source = JsonSource::new(&path)?;
        let batch = source.next_batch(10)?;
        assert_eq!(batch.len(), 2);
        
        let charlie = batch.iter().find(|r| r.get("id").and_then(|v| v.as_str()) == Some("user_abc")).unwrap();
        assert_eq!(charlie.get("name").unwrap().as_str().unwrap(), "Charlie");
        assert_eq!(charlie.get("created_at").unwrap().as_i64().unwrap(), 1728000000);

        let _ = std::fs::remove_file(path);
        Ok(())
    }
}
