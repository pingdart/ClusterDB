use crate::backup::proto::*;
use crate::exec::{ColumnDef, RowData, TableSchema};
use crate::planner::stats::{ColumnStats, HistogramBucket, HistogramValue, TableStats};
use crate::security::encryption::CryptoEngine;
use crate::storage::StorageEngine;
use anyhow::{anyhow, Context, Result};
use prost::Message;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::time::Instant;

pub const MAGIC_BYTES: &[u8; 12] = b"PINGDART_PB\x01";
pub const FLAG_COMPRESSED_ZSTD: u8 = 0x01;
pub const FLAG_ENCRYPTED_AES256: u8 = 0x02;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportReport {
    pub scope: String,
    pub database: String,
    pub file_path: String,
    pub uncompressed_bytes: u64,
    pub compressed_bytes: u64,
    pub compression_ratio: f64,
    pub is_encrypted: bool,
    pub checksum_sha256: String,
    pub tables_count: usize,
    pub rows_count: usize,
    pub vectors_count: usize,
    pub duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportReport {
    pub source_file: String,
    pub scope: String,
    pub database: String,
    pub tables_restored: usize,
    pub rows_restored: usize,
    pub vectors_restored: usize,
    pub checksum_verified: bool,
    pub duration_ms: f64,
    pub warnings: Vec<String>,
}

pub struct PbPackageManager;

impl PbPackageManager {
    /// Exports a database or full cluster to a production-grade .pb file
    pub fn export_pb(
        storage: &StorageEngine,
        scope: &str, // "database" or "cluster"
        database: &str,
        dest_path: &str,
        encrypted: bool,
        passphrase: Option<&str>,
    ) -> Result<ExportReport> {
        let start = Instant::now();
        let db_prefix = database;
        let prefix = format!("{}_", db_prefix);

        let mut schemas = Vec::new();
        let mut row_chunks = Vec::new();
        let mut indexes = Vec::new();
        let vector_chunks = Vec::new();
        let mut stats = Vec::new();

        let mut total_rows = 0;
        let total_vectors = 0;
        let mut tables_count = 0;

        let all_tables = storage.get_keys_by_prefix("_tables", "").unwrap_or_default();
        for full_table in &all_tables {
            let is_match = if scope == "cluster" {
                true
            } else {
                full_table.starts_with(&prefix) || full_table == db_prefix
            };

            if !is_match {
                continue;
            }

            tables_count += 1;

            // 1. Export Schema
            if let Ok(Some(s)) = storage.get::<TableSchema>("_schemas", full_table) {
                schemas.push(PbTableSchema {
                    name: full_table.clone(),
                    columns: s.columns.iter().map(|c| PbColumnDef {
                        name: c.name.clone(),
                        data_type: c.data_type.clone(),
                        is_primary: c.is_primary,
                        is_unique: c.is_unique,
                        is_not_null: c.is_not_null,
                        auto_increment: c.auto_increment,
                        default_value: c.default_value.clone(),
                    }).collect(),
                });
            }

            // 2. Export Rows in 10,000-row chunks for streaming memory efficiency
            if let Ok(rows) = storage.get_all_with_keys::<RowData>(full_table) {
                total_rows += rows.len();
                const CHUNK_SIZE: usize = 10_000;
                for (chunk_idx, chunk) in rows.chunks(CHUNK_SIZE).enumerate() {
                    let pb_rows = chunk.iter().map(|(k, r)| PbRowItem {
                        id: k.clone(),
                        columns: r.columns.clone(),
                    }).collect();

                    row_chunks.push(PbRowChunk {
                        table_name: full_table.clone(),
                        chunk_index: chunk_idx as u32,
                        rows: pb_rows,
                    });
                }
            }

            // 3. Export Secondary Indexes
            if let Ok(Some(idx_list)) = storage.get::<Vec<Vec<String>>>("_indexes", full_table) {
                for f in idx_list {
                    indexes.push(PbIndexDef {
                        table_name: full_table.clone(),
                        fields: f,
                    });
                }
            }

            // 4. Export Table Stats
            if let Ok(Some(st)) = storage.get::<TableStats>("_system_table_stats", full_table) {
                stats.push(Self::convert_table_stats_to_pb(&st));
            }
        }

        // Users, Roles, Tenants if cluster export or security tables exist
        let users: Vec<PbUser> = storage.get_all::<crate::auth::User>("_users")
            .unwrap_or_default()
            .into_iter()
            .map(|u| PbUser {
                username: u.username,
                password_hash: u.password_hash,
                allowed_ip: u.allowed_ip,
                roles: u.roles.iter().map(|r| format!("{:?}", r)).collect(),
            }).collect();

        let roles: Vec<PbRole> = storage.get_all::<crate::security::permissions::RoleDefinition>("_roles")
            .unwrap_or_default()
            .into_iter()
            .map(|r| PbRole {
                name: r.name,
                permissions: r.permissions.iter().map(|p| format!("{:?}", p)).collect(),
            }).collect();

        let api_keys: Vec<PbApiKey> = storage.get_all::<crate::security::api_key::ApiKey>("_api_keys")
            .unwrap_or_default()
            .into_iter()
            .map(|k| PbApiKey {
                id: k.key_id,
                key_hash: k.key_hash,
                user_or_service: k.name,
                roles: k.roles,
                is_revoked: k.revoked,
                created_at: k.created_at,
                expires_at: k.expires_at,
            }).collect();

        let tenants: Vec<PbTenant> = storage.get_all::<crate::tenant::Tenant>("_tenants")
            .unwrap_or_default()
            .into_iter()
            .map(|t| PbTenant {
                id: t.id,
                name: t.name,
                max_storage_bytes: t.max_storage_bytes,
                max_vectors: t.max_vectors as u64,
                max_qps: t.max_qps as u64,
            }).collect();

        let header = PbPackageHeader {
            magic: "PINGDART_PB".to_string(),
            version: 1,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            scope: scope.to_string(),
            database: database.to_string(),
            is_encrypted: encrypted,
            compression: "zstd".to_string(),
            checksum_sha256: String::new(), // computed after compression/encryption
            total_rows: total_rows as u64,
            total_vectors: total_vectors as u64,
        };

        let envelope = PbEnvelope {
            header: Some(header),
            schemas,
            row_chunks,
            vector_chunks,
            indexes,
            graph_nodes: Vec::new(),
            graph_edges: Vec::new(),
            memories: Vec::new(),
            users,
            roles,
            api_keys,
            tenants,
            stats,
        };

        // 1. Protobuf Serialize
        let uncompressed_bytes = envelope.encode_to_vec();
        let uncompressed_len = uncompressed_bytes.len() as u64;

        // 2. ZSTD Compress
        let compressed_bytes = zstd::encode_all(&uncompressed_bytes[..], 3)
            .context("Failed to compress backup package with ZSTD")?;
        let compressed_len = compressed_bytes.len() as u64;

        // 3. AES-256-GCM Encrypt (if requested)
        let mut flags = FLAG_COMPRESSED_ZSTD;
        let payload = if encrypted {
            flags |= FLAG_ENCRYPTED_AES256;
            let key = passphrase.unwrap_or("PINGDART_ENTERPRISE_CLUSTER_MASTER_SECRET_2026");
            let crypto = CryptoEngine::new(key);
            crypto.encrypt(&compressed_bytes)
                .context("Failed to encrypt backup package with AES-256-GCM")?
        } else {
            compressed_bytes
        };

        // 4. SHA-256 Checksum calculation over payload body
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let checksum_bytes = hasher.finalize();
        let checksum_hex = hex::encode(checksum_bytes);

        // 5. Build Container Binary: [ Magic (12) || Flags (1) || Checksum (32) || Payload (N) ]
        let mut container = Vec::with_capacity(12 + 1 + 32 + payload.len());
        container.extend_from_slice(MAGIC_BYTES);
        container.push(flags);
        container.extend_from_slice(&checksum_bytes);
        container.extend_from_slice(&payload);

        // 6. Write to destination file
        if let Some(parent) = Path::new(dest_path).parent() {
            if !parent.as_os_str().is_empty() {
                let _ = fs::create_dir_all(parent);
            }
        }
        fs::write(dest_path, &container)
            .with_context(|| format!("Failed to write backup package to '{}'", dest_path))?;

        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        let ratio = if compressed_len > 0 {
            uncompressed_len as f64 / compressed_len as f64
        } else {
            1.0
        };

        Ok(ExportReport {
            scope: scope.to_string(),
            database: database.to_string(),
            file_path: dest_path.to_string(),
            uncompressed_bytes: uncompressed_len,
            compressed_bytes: compressed_len,
            compression_ratio: ratio,
            is_encrypted: encrypted,
            checksum_sha256: checksum_hex,
            tables_count,
            rows_count: total_rows,
            vectors_count: total_vectors,
            duration_ms,
        })
    }

    /// Imports a .pb backup package, strictly verifying checksum, decrypting, decompressing, and restoring
    pub fn import_pb(
        storage: &StorageEngine,
        src_path: &str,
        passphrase: Option<&str>,
        target_db: Option<&str>,
    ) -> Result<ImportReport> {
        let start = Instant::now();

        let file_bytes = fs::read(src_path)
            .with_context(|| format!("Failed to read backup package from '{}'", src_path))?;

        if file_bytes.len() < 12 + 1 + 32 {
            return Err(anyhow!("Invalid PingDart .pb file: file is too small to contain header"));
        }

        // 1. Verify Magic Bytes
        if &file_bytes[0..12] != MAGIC_BYTES {
            return Err(anyhow!("Invalid PingDart .pb file: magic header mismatch (expected PINGDART_PB)"));
        }

        let flags = file_bytes[12];
        let is_compressed = (flags & FLAG_COMPRESSED_ZSTD) != 0;
        let is_encrypted = (flags & FLAG_ENCRYPTED_AES256) != 0;

        let expected_checksum = &file_bytes[13..45];
        let payload = &file_bytes[45..];

        // 2. Strict SHA-256 Checksum Verification
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let actual_checksum = hasher.finalize();

        if actual_checksum.as_slice() != expected_checksum {
            return Err(anyhow!(
                "Checksum verification failed: .pb backup file is corrupted or has been tampered with"
            ));
        }

        // 3. AES-256-GCM Decrypt (if encrypted)
        let unencrypted_bytes = if is_encrypted {
            let key = passphrase.unwrap_or("PINGDART_ENTERPRISE_CLUSTER_MASTER_SECRET_2026");
            let crypto = CryptoEngine::new(key);
            crypto.decrypt(payload)
                .context("AES-256-GCM decryption failed: invalid passphrase or tampered ciphertext")?
        } else {
            payload.to_vec()
        };

        // 4. ZSTD Decompress (if compressed)
        let protobuf_bytes = if is_compressed {
            zstd::decode_all(&unencrypted_bytes[..])
                .context("Failed to decompress backup payload with ZSTD")?
        } else {
            unencrypted_bytes
        };

        // 5. Protobuf Decode
        let envelope = PbEnvelope::decode(&protobuf_bytes[..])
            .context("Failed to deserialize Protobuf PbEnvelope")?;

        let header = envelope.header.ok_or_else(|| anyhow!("Missing package header in .pb envelope"))?;
        let active_db = target_db.unwrap_or(&header.database);

        let mut tables_restored = 0;
        let mut rows_restored = 0;
        let vectors_restored = 0;
        let warnings = Vec::new();

        // 6. Restore Schemas
        for schema in envelope.schemas {
            let full_table_name = if target_db.is_some() {
                // If importing into custom database, rewrite prefix
                let parts: Vec<&str> = schema.name.splitn(2, '_').collect();
                let clean_name = if parts.len() == 2 { parts[1] } else { &schema.name };
                format!("{}_{}", active_db, clean_name)
            } else {
                schema.name.clone()
            };

            let table_schema = TableSchema {
                name: full_table_name.clone(),
                columns: schema.columns.into_iter().map(|c| ColumnDef {
                    name: c.name,
                    data_type: c.data_type,
                    is_primary: c.is_primary,
                    auto_increment: c.auto_increment,
                    is_unique: c.is_unique,
                    is_not_null: c.is_not_null,
                    default_value: c.default_value,
                    generated_from: None,
                    using_model: None,
                }).collect(),
            };

            storage.set("_schemas", &full_table_name, &table_schema)?;
            storage.set("_tables", &full_table_name, &true)?;
            tables_restored += 1;
        }

        // 7. Restore Row Chunks
        for chunk in envelope.row_chunks {
            let target_table = if target_db.is_some() {
                let parts: Vec<&str> = chunk.table_name.splitn(2, '_').collect();
                let clean_name = if parts.len() == 2 { parts[1] } else { &chunk.table_name };
                format!("{}_{}", active_db, clean_name)
            } else {
                chunk.table_name.clone()
            };

            for item in chunk.rows {
                let row = RowData {
                    _id: item.id.clone(),
                    columns: item.columns,
                    ..Default::default()
                };
                storage.set(&target_table, &item.id, &row)?;
                rows_restored += 1;
            }
        }

        // 8. Restore Secondary Indexes
        for idx in envelope.indexes {
            let target_table = if target_db.is_some() {
                let parts: Vec<&str> = idx.table_name.splitn(2, '_').collect();
                let clean_name = if parts.len() == 2 { parts[1] } else { &idx.table_name };
                format!("{}_{}", active_db, clean_name)
            } else {
                idx.table_name.clone()
            };

            let mut existing: Vec<Vec<String>> = storage.get("_indexes", &target_table)?.unwrap_or_default();
            if !existing.contains(&idx.fields) {
                existing.push(idx.fields);
                storage.set("_indexes", &target_table, &existing)?;
            }
        }

        // 9. Restore Security Credentials & Multi-Tenancy (if cluster-level export)
        if header.scope == "cluster" {
            for user in envelope.users {
                let mut roles = Vec::new();
                for r_str in &user.roles {
                    if r_str == "MainAdmin" {
                        roles.push(crate::auth::Role::MainAdmin);
                    } else if r_str.starts_with("DatabaseAdmin(") {
                        let db = r_str.trim_start_matches("DatabaseAdmin(").trim_end_matches(')');
                        roles.push(crate::auth::Role::DatabaseAdmin(db.to_string()));
                    } else if r_str.starts_with("ReadWrite(") {
                        let db = r_str.trim_start_matches("ReadWrite(").trim_end_matches(')');
                        roles.push(crate::auth::Role::ReadWrite(db.to_string()));
                    } else if r_str.starts_with("ReadOnly(") {
                        let db = r_str.trim_start_matches("ReadOnly(").trim_end_matches(')');
                        roles.push(crate::auth::Role::ReadOnly(db.to_string()));
                    }
                }
                let u = crate::auth::User {
                    username: user.username.clone(),
                    password_hash: user.password_hash,
                    token: None,
                    allowed_ip: user.allowed_ip,
                    roles,
                    rbac_roles: Vec::new(),
                    assigned_tenants: vec!["default".to_string()],
                    is_service_account: false,
                };
                storage.set("_users", &user.username, &u)?;
            }

            for role in envelope.roles {
                let mut permissions = std::collections::HashSet::new();
                for p in &role.permissions {
                    match p.as_str() {
                        "DatabaseCreate" => { permissions.insert(crate::security::permissions::Permission::DatabaseCreate); }
                        "DatabaseDrop" => { permissions.insert(crate::security::permissions::Permission::DatabaseDrop); }
                        "DatabaseRead" => { permissions.insert(crate::security::permissions::Permission::DatabaseRead); }
                        "DatabaseWrite" => { permissions.insert(crate::security::permissions::Permission::DatabaseWrite); }
                        "TableCreate" => { permissions.insert(crate::security::permissions::Permission::TableCreate); }
                        "TableDrop" => { permissions.insert(crate::security::permissions::Permission::TableDrop); }
                        "TableRead" => { permissions.insert(crate::security::permissions::Permission::TableRead); }
                        "TableWrite" => { permissions.insert(crate::security::permissions::Permission::TableWrite); }
                        "VectorSearch" => { permissions.insert(crate::security::permissions::Permission::VectorSearch); }
                        "VectorInsert" => { permissions.insert(crate::security::permissions::Permission::VectorInsert); }
                        "GraphRead" => { permissions.insert(crate::security::permissions::Permission::GraphRead); }
                        "GraphWrite" => { permissions.insert(crate::security::permissions::Permission::GraphWrite); }
                        "MemoryRecall" => { permissions.insert(crate::security::permissions::Permission::MemoryRecall); }
                        "MemoryWrite" => { permissions.insert(crate::security::permissions::Permission::MemoryWrite); }
                        "BackupCreate" => { permissions.insert(crate::security::permissions::Permission::BackupCreate); }
                        "BackupRestore" => { permissions.insert(crate::security::permissions::Permission::BackupRestore); }
                        "PITR" => { permissions.insert(crate::security::permissions::Permission::PITR); }
                        "TenantAdmin" => { permissions.insert(crate::security::permissions::Permission::TenantAdmin); }
                        "UserAdmin" => { permissions.insert(crate::security::permissions::Permission::UserAdmin); }
                        "AuditRead" => { permissions.insert(crate::security::permissions::Permission::AuditRead); }
                        _ => {}
                    }
                }
                let r = crate::security::permissions::RoleDefinition {
                    name: role.name.clone(),
                    description: "Restored from cluster .pb snapshot".to_string(),
                    permissions,
                    scope: crate::security::permissions::RoleScope::Global,
                };
                storage.set("_roles", &role.name, &r)?;
            }

            for key in envelope.api_keys {
                let k = crate::security::api_key::ApiKey {
                    key_id: key.id.clone(),
                    key_hash: key.key_hash,
                    name: key.user_or_service,
                    prefix: "pd_live_...".to_string(),
                    roles: key.roles,
                    tenant_id: Some("default".to_string()),
                    created_at: key.created_at,
                    expires_at: key.expires_at,
                    last_used_at: None,
                    revoked: key.is_revoked,
                };
                storage.set("_api_keys", &key.id, &k)?;
            }

            for tenant in envelope.tenants {
                let t = crate::tenant::Tenant {
                    id: tenant.id.clone(),
                    name: tenant.name,
                    max_storage_bytes: tenant.max_storage_bytes,
                    max_vectors: tenant.max_vectors as usize,
                    max_qps: tenant.max_qps as u32,
                    current_storage_bytes: 0,
                    current_vectors: 0,
                    rls_enabled: false,
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64,
                };
                storage.set("_tenants", &tenant.id, &t)?;
            }
        }

        // 10. Restore CBO Catalog Statistics
        for st in envelope.stats {
            let target_table = if target_db.is_some() {
                let parts: Vec<&str> = st.table_name.splitn(2, '_').collect();
                let clean_name = if parts.len() == 2 { parts[1] } else { &st.table_name };
                format!("{}_{}", active_db, clean_name)
            } else {
                st.table_name.clone()
            };

            let table_stats = Self::convert_pb_to_table_stats(&st, &target_table);
            storage.set("_system_table_stats", &target_table, &table_stats)?;
        }

        storage.flush()?;

        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

        Ok(ImportReport {
            source_file: src_path.to_string(),
            scope: header.scope,
            database: active_db.to_string(),
            tables_restored,
            rows_restored,
            vectors_restored,
            checksum_verified: true,
            duration_ms,
            warnings,
        })
    }

    fn convert_table_stats_to_pb(stats: &TableStats) -> PbTableStats {
        let mut columns = Vec::new();
        for (col_name, col) in &stats.columns {
            let buckets = col.buckets.iter().map(|b| PbHistogramBucket {
                lower_bound: format!("{:?}", b.lower_bound),
                upper_bound: format!("{:?}", b.upper_bound),
                row_count: b.row_count as u64,
                distinct_count: b.distinct_count as u64,
            }).collect();

            columns.push(PbColumnStats {
                column_name: col_name.clone(),
                null_count: col.null_count as u64,
                ndv: col.ndv as u64,
                min_val: col.min_val.as_ref().map(|v| format!("{:?}", v)),
                max_val: col.max_val.as_ref().map(|v| format!("{:?}", v)),
                buckets,
            });
        }

        PbTableStats {
            table_name: stats.table_name.clone(),
            total_rows: stats.total_rows as u64,
            analyzed_rows: stats.analyzed_rows as u64,
            modification_counter: stats.modification_counter as u64,
            last_analyzed: stats.last_analyzed_epoch_secs,
            columns,
        }
    }

    fn convert_pb_to_table_stats(pb: &PbTableStats, target_table_name: &str) -> TableStats {
        let mut stats = TableStats::new(target_table_name);
        stats.total_rows = pb.total_rows as usize;
        stats.analyzed_rows = pb.analyzed_rows as usize;
        stats.modification_counter = pb.modification_counter as usize;
        stats.last_analyzed_epoch_secs = pb.last_analyzed;

        for col in &pb.columns {
            let buckets = col.buckets.iter().map(|b| HistogramBucket {
                lower_bound: HistogramValue::from_str_auto(&b.lower_bound),
                upper_bound: HistogramValue::from_str_auto(&b.upper_bound),
                row_count: b.row_count as usize,
                distinct_count: b.distinct_count as usize,
            }).collect();

            stats.columns.insert(col.column_name.clone(), ColumnStats {
                column_name: col.column_name.clone(),
                null_count: col.null_count as usize,
                ndv: col.ndv as usize,
                min_val: col.min_val.as_ref().map(|s| HistogramValue::from_str_auto(s)),
                max_val: col.max_val.as_ref().map(|s| HistogramValue::from_str_auto(s)),
                buckets,
            });
        }

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_pb_package_roundtrip_with_zstd_and_encryption() {
        let storage = StorageEngine::new_temp().unwrap();

        // Seed schema
        let schema = TableSchema {
            name: "default_users".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".to_string(),
                    data_type: "INT".to_string(),
                    is_primary: true,
                    auto_increment: false,
                    is_unique: true,
                    is_not_null: true,
                    default_value: None,
                    generated_from: None,
                    using_model: None,
                },
                ColumnDef {
                    name: "email".to_string(),
                    data_type: "TEXT".to_string(),
                    is_primary: false,
                    auto_increment: false,
                    is_unique: false,
                    is_not_null: false,
                    default_value: None,
                    generated_from: None,
                    using_model: None,
                },
            ],
        };
        storage.set("_schemas", "default_users", &schema).unwrap();
        storage.set("_tables", "default_users", &true).unwrap();

        // Seed 100 rows
        for i in 0..100 {
            let mut cols = HashMap::new();
            cols.insert("id".to_string(), i.to_string());
            cols.insert("email".to_string(), format!("user_{}@pingdart.ai", i));
            let row = RowData {
                _id: i.to_string(),
                columns: cols,
                ..Default::default()
            };
            storage.set("default_users", &i.to_string(), &row).unwrap();
        }

        let export_path = "test_artifacts_backup.pb";

        // Export with ZSTD and AES-256-GCM encryption
        let export_report = PbPackageManager::export_pb(
            &storage,
            "database",
            "default",
            export_path,
            true,
            Some("test_passphrase_secret"),
        ).unwrap();

        assert_eq!(export_report.tables_count, 1);
        assert_eq!(export_report.rows_count, 100);
        assert!(export_report.compression_ratio > 1.0);
        assert!(export_report.is_encrypted);
        assert!(!export_report.checksum_sha256.is_empty());

        // Create a new fresh database and import into it
        let restore_storage = StorageEngine::new_temp().unwrap();
        let import_report = PbPackageManager::import_pb(
            &restore_storage,
            export_path,
            Some("test_passphrase_secret"),
            None,
        ).unwrap();

        assert_eq!(import_report.tables_restored, 1);
        assert_eq!(import_report.rows_restored, 100);
        assert!(import_report.checksum_verified);

        // Verify rows exist in restored database
        let restored_row: Option<RowData> = restore_storage.get("default_users", "42").unwrap();
        assert!(restored_row.is_some());
        assert_eq!(restored_row.unwrap().columns.get("email").unwrap(), "user_42@pingdart.ai");

        // Clean up temporary test file
        let _ = fs::remove_file(export_path);
    }

    #[test]
    fn test_pb_tampering_fails_checksum_verification() {
        let storage = StorageEngine::new_temp().unwrap();
        let export_path = "test_tampered.pb";

        let _ = PbPackageManager::export_pb(
            &storage,
            "database",
            "default",
            export_path,
            false,
            None,
        ).unwrap();

        // Tamper with file: flip a byte in the payload
        let mut file_bytes = fs::read(export_path).unwrap();
        let last_idx = file_bytes.len() - 1;
        file_bytes[last_idx] ^= 0xFF;
        fs::write(export_path, &file_bytes).unwrap();

        // Attempt import -> must fail with checksum verification error
        let restore_storage = StorageEngine::new_temp().unwrap();
        let res = PbPackageManager::import_pb(
            &restore_storage,
            export_path,
            None,
            None,
        );

        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Checksum verification failed"));

        let _ = fs::remove_file(export_path);
    }
}
