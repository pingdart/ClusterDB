use crate::embedding::EmbeddingManager;
use crate::exec::RowData;
use crate::migration::checkpoint::MigrationCheckpoint;
use crate::migration::mapping::{MigrationMapping, SchemaPolicy};
use crate::migration::source::{CsvSource, JsonSource, MigrationSource, MongoJsonSource, MySQLDumpSource, PostgresDumpSource};
use crate::migration::target::{MigrationTarget, PingDartTarget};
use crate::migration::validator::{MigrationValidator, ValidationReport};
use crate::observability::DistributedTracer;
use crate::storage::StorageEngine;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationConfig {
    pub batch_size: usize,
    pub workers: usize,
    pub checkpoint_interval_batches: usize,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            batch_size: 5_000,
            workers: 4,
            checkpoint_interval_batches: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MigrationProgress {
    pub records_processed: usize,
    pub records_failed: usize,
    pub bytes_processed: u64,
    pub start_time: u64,
    pub end_time: Option<u64>,
    pub current_batch: u64,
    pub qps: f64,
    pub validation: Option<ValidationReport>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationJob {
    pub job_id: String,
    pub source_type: String,
    pub source_path: String,
    pub target_table: String,
    pub mapping: MigrationMapping,
    pub config: MigrationConfig,
    pub status: MigrationStatus,
    pub progress: MigrationProgress,
    pub checkpoint: MigrationCheckpoint,
}

pub struct MigrationManager {
    storage: StorageEngine,
    jobs: Arc<RwLock<HashMap<String, Arc<RwLock<MigrationJob>>>>>,
}

impl MigrationManager {
    pub fn new(storage: StorageEngine) -> Self {
        Self {
            storage,
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn start_job(
        &self,
        source_type: String,
        source_path: String,
        target_table: String,
        mapping: MigrationMapping,
        config: MigrationConfig,
        embedding_mgr: Arc<EmbeddingManager>,
        tracer: Option<Arc<DistributedTracer>>,
    ) -> Result<String> {
        let job_id = uuid::Uuid::new_v4().to_string();
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        let checkpoint = MigrationCheckpoint::new(job_id.clone());
        let mut progress = MigrationProgress::default();
        progress.start_time = now;

        let job = MigrationJob {
            job_id: job_id.clone(),
            source_type: source_type.clone(),
            source_path: source_path.clone(),
            target_table: target_table.clone(),
            mapping: mapping.clone(),
            config: config.clone(),
            status: MigrationStatus::Pending,
            progress,
            checkpoint,
        };

        // Persist initial job metadata
        self.storage.set("_system_migrations", &job_id, &job)?;
        self.storage.flush()?;

        let job_arc = Arc::new(RwLock::new(job));
        {
            let mut w = self.jobs.write().unwrap();
            w.insert(job_id.clone(), Arc::clone(&job_arc));
        }

        // Spawn background worker thread
        let storage_clone = self.storage.clone();
        let job_id_clone = job_id.clone();
        let thread_name = format!("mig-{}", &job_id[..8.min(job_id.len())]);
        std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                if let Err(e) = Self::run_migration_loop(
                    job_id_clone,
                    job_arc,
                    storage_clone,
                    embedding_mgr,
                    tracer,
                ) {
                    error!("Migration job failed: {}", e);
                }
            })?;

        Ok(job_id)
    }

    pub fn resume_job(
        &self,
        job_id: &str,
        embedding_mgr: Arc<EmbeddingManager>,
        tracer: Option<Arc<DistributedTracer>>,
    ) -> Result<String> {
        let mut saved_job: MigrationJob = self
            .storage
            .get("_system_migrations", job_id)?
            .ok_or_else(|| anyhow!("Migration job '{}' not found", job_id))?;

        if let Some(cp) = MigrationCheckpoint::load(&self.storage, job_id)? {
            saved_job.checkpoint = cp;
        }

        saved_job.status = MigrationStatus::Pending;
        self.storage.set("_system_migrations", job_id, &saved_job)?;
        self.storage.flush()?;

        let job_arc = Arc::new(RwLock::new(saved_job));
        {
            let mut w = self.jobs.write().unwrap();
            w.insert(job_id.to_string(), Arc::clone(&job_arc));
        }

        let storage_clone = self.storage.clone();
        let job_id_clone = job_id.to_string();
        let thread_name = format!("mig-res-{}", &job_id[..8.min(job_id.len())]);
        std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                if let Err(e) = Self::run_migration_loop(
                    job_id_clone,
                    job_arc,
                    storage_clone,
                    embedding_mgr,
                    tracer,
                ) {
                    error!("Resumed migration job failed: {}", e);
                }
            })?;

        Ok(job_id.to_string())
    }

    pub fn abort_job(&self, job_id: &str) -> Result<()> {
        let mut found = false;
        {
            let r = self.jobs.read().unwrap();
            if let Some(j) = r.get(job_id) {
                let mut job = j.write().unwrap();
                job.status = MigrationStatus::Aborted;
                found = true;
            }
        }
        if found {
            if let Some(mut persistent_job) = self.storage.get::<MigrationJob>("_system_migrations", job_id)? {
                persistent_job.status = MigrationStatus::Aborted;
                self.storage.set("_system_migrations", job_id, &persistent_job)?;
                self.storage.flush()?;
            }
            Ok(())
        } else {
            Err(anyhow!("Migration job '{}' not found", job_id))
        }
    }

    pub fn get_status(&self, job_id: &str) -> Result<Option<MigrationJob>> {
        {
            let r = self.jobs.read().unwrap();
            if let Some(j) = r.get(job_id) {
                return Ok(Some(j.read().unwrap().clone()));
            }
        }
        self.storage.get("_system_migrations", job_id)
    }

    pub fn list_jobs(&self) -> Result<Vec<MigrationJob>> {
        let keys = self.storage.get_keys_by_prefix("_system_migrations", "")?;
        let mut list = Vec::new();
        for k in keys {
            if let Ok(Some(j)) = self.storage.get::<MigrationJob>("_system_migrations", &k) {
                list.push(j);
            }
        }
        Ok(list)
    }

    pub fn validate_job(&self, job_id: &str) -> Result<ValidationReport> {
        let job: MigrationJob = self
            .storage
            .get("_system_migrations", job_id)?
            .ok_or_else(|| anyhow!("Migration job '{}' not found", job_id))?;

        let primary_key = job.mapping.primary_key.as_deref();
        let report = MigrationValidator::validate(
            job_id,
            &self.storage,
            &job.target_table,
            job.progress.records_processed,
            &job.checkpoint.checksum_so_far,
            primary_key,
            &[],
        )?;

        // Update progress with validation
        let mut updated = job.clone();
        updated.progress.validation = Some(report.clone());
        self.storage.set("_system_migrations", job_id, &updated)?;
        self.storage.flush()?;

        Ok(report)
    }

    fn run_migration_loop(
        job_id: String,
        job_arc: Arc<RwLock<MigrationJob>>,
        storage: StorageEngine,
        embedding_mgr: Arc<EmbeddingManager>,
        tracer: Option<Arc<DistributedTracer>>,
    ) -> Result<()> {
        let (source_type, source_path, target_table, mapping, config, start_offset) = {
            let mut j = job_arc.write().unwrap();
            j.status = MigrationStatus::Running;
            (
                j.source_type.clone(),
                j.source_path.clone(),
                j.target_table.clone(),
                j.mapping.clone(),
                j.config.clone(),
                j.checkpoint.source_offset,
            )
        };

        let mut source: Box<dyn MigrationSource> = match source_type.to_lowercase().as_str() {
            "csv" => Box::new(CsvSource::new(&source_path, None)?),
            "json" | "jsonl" => Box::new(JsonSource::new(&source_path)?),
            "mongo" | "mongodb" => Box::new(MongoJsonSource::new(&source_path)?),
            "postgres" | "postgresql" => Box::new(PostgresDumpSource::new(&source_path)?),
            "mysql" => Box::new(MySQLDumpSource::new(&source_path)?),
            other => return Err(anyhow!("Unsupported source format: {}", other)),
        };

        if start_offset > 0 {
            source.seek_offset(start_offset)?;
        }

        let mut target = PingDartTarget::new(storage.clone());
        let mut running_hasher = sha2::Sha256::default();

        let mut batch_number = { job_arc.read().unwrap().checkpoint.batch_number };
        let mut total_records = { job_arc.read().unwrap().progress.records_processed };
        let start_time_instant = std::time::Instant::now();

        info!("Starting migration job {} from {}", job_id, source_path);

        loop {
            // Check for abort request
            {
                let j = job_arc.read().unwrap();
                if j.status == MigrationStatus::Aborted {
                    info!("Migration job {} was aborted by user", job_id);
                    break;
                }
            }

            let batch_raw = source.next_batch(config.batch_size)?;
            if batch_raw.is_empty() {
                break;
            }

            batch_number += 1;
            let batch_start_instant = std::time::Instant::now();
            let batch_start_epoch_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;

            // Schema drift detection and update
            if mapping.schema_policy == SchemaPolicy::AddMissingColumns {
                let mut discovered_cols = Vec::new();
                for r in &batch_raw {
                    for k in r.keys() {
                        let target_col = mapping.column_renames.get(k).unwrap_or(k);
                        if !discovered_cols.contains(target_col) {
                            discovered_cols.push(target_col.clone());
                        }
                    }
                }
                target.update_schema_if_needed(&target_table, &discovered_cols)?;
            }

            // Transform raw maps into RowData
            let schema_pk = storage.get::<crate::exec::TableSchema>("_schemas", &target_table)
                .ok()
                .flatten()
                .and_then(|s| s.columns.into_iter().find(|c| c.is_primary).map(|c| c.name));

            let mut transformed_rows = Vec::with_capacity(batch_raw.len());
            for raw_row in batch_raw {
                match mapping.transform_record(&raw_row, Some(&embedding_mgr), None) {
                    Ok(cols) => {
                        let pk_val = mapping.primary_key.as_ref()
                            .and_then(|pk| cols.get(pk))
                            .or_else(|| cols.get("id"))
                            .or_else(|| cols.get("_id"))
                            .or_else(|| schema_pk.as_ref().and_then(|pk| cols.get(pk)))
                            .cloned();
                        let id = pk_val.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                        
                        use sha2::Digest;
                        running_hasher.update(id.as_bytes());

                        let row = RowData {
                            _id: id,
                            columns: cols,
                            ..Default::default()
                        };
                        transformed_rows.push(row);
                    }
                    Err(e) => {
                        let mut j = job_arc.write().unwrap();
                        j.progress.records_failed += 1;
                        if j.progress.errors.len() < 50 {
                            j.progress.errors.push(e.to_string());
                        }
                    }
                }
            }

            // Write batch into storage
            let written = target.write_batch(&target_table, &transformed_rows)?;
            target.flush()?;

            total_records += written;

            // Update Progress & Checkpoint
            let elapsed_secs = start_time_instant.elapsed().as_secs_f64().max(0.001);
            let qps = (total_records as f64) / elapsed_secs;
            let current_offset = source.current_offset();

            {
                let mut j = job_arc.write().unwrap();
                j.progress.records_processed = total_records;
                j.progress.current_batch = batch_number;
                j.progress.qps = qps;

                if batch_number % (config.checkpoint_interval_batches as u64) == 0 {
                    j.checkpoint.source_offset = current_offset;
                    j.checkpoint.batch_number = batch_number;
                    j.checkpoint.records_committed = total_records;
                    j.checkpoint.updated_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
                    let _ = j.checkpoint.save(&storage);
                    let _ = storage.set("_system_migrations", &job_id, &*j);
                }
            }

            // Record OpenTelemetry span if tracer configured
            if let Some(ref t) = tracer {
                let mut attrs = HashMap::new();
                attrs.insert("migration.job_id".to_string(), job_id.clone());
                attrs.insert("migration.batch".to_string(), batch_number.to_string());
                attrs.insert("migration.rows".to_string(), written.to_string());
                attrs.insert("migration.qps".to_string(), format!("{:.1}", qps));
                let span = crate::observability::Span {
                    trace_id: format!("mig_{}", job_id.replace('-', "")),
                    span_id: format!("{:016x}", batch_number),
                    parent_span_id: None,
                    name: "MigrationBatch".to_string(),
                    start_time_epoch_ms: batch_start_epoch_ms,
                    duration_ms: batch_start_instant.elapsed().as_secs_f64() * 1000.0,
                    status: "OK".to_string(),
                    attributes: attrs,
                    events: vec![],
                };
                t.record_span(span);
            }
        }

        // Finalize job
        let end_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        {
            let mut j = job_arc.write().unwrap();
            if j.status != MigrationStatus::Aborted {
                j.status = MigrationStatus::Completed;
            }
            j.progress.end_time = Some(end_time);
            j.checkpoint.source_offset = source.current_offset();
            j.checkpoint.records_committed = total_records;
            let _ = j.checkpoint.save(&storage);
            let _ = storage.set("_system_migrations", &job_id, &*j);
            storage.flush()?;
        }

        info!("Migration job {} completed. Total records: {}", job_id, total_records);
        Ok(())
    }
}
