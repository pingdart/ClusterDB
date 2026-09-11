use crate::exec::{ColumnDef, RowData};
use crate::storage::StorageEngine;
use anyhow::Result;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

pub trait MigrationTarget: Send + Sync {
    fn write_batch(&mut self, table: &str, rows: &[RowData]) -> Result<usize>;
    fn update_schema_if_needed(&mut self, table: &str, new_columns: &[String]) -> Result<()>;
    fn flush(&mut self) -> Result<()>;
}

pub struct PingDartTarget {
    storage: StorageEngine,
}

impl PingDartTarget {
    pub fn new(storage: StorageEngine) -> Self {
        Self { storage }
    }
}

impl MigrationTarget for PingDartTarget {
    fn write_batch(&mut self, table: &str, rows: &[RowData]) -> Result<usize> {
        let mut count = 0;
        for row in rows {
            let row_key = if !row._id.is_empty() {
                row._id.clone()
            } else {
                uuid::Uuid::new_v4().to_string()
            };
            self.storage.set(table, &row_key, row)?;
            count += 1;
        }
        Ok(count)
    }

    fn update_schema_if_needed(&mut self, table: &str, new_columns: &[String]) -> Result<()> {
        let base_name = if let Some(pos) = table.find('_') {
            &table[pos + 1..]
        } else {
            table
        };

        if let Ok(Some(mut schema)) = self.storage.get::<crate::exec::TableSchema>("_schemas", table) {
            let mut changed = false;
            for col in new_columns {
                if !schema.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col)) {
                    schema.columns.push(ColumnDef {
                        name: col.clone(),
                        data_type: "TEXT".to_string(),
                        is_primary: false,
                        auto_increment: false,
                        is_unique: false,
                        is_not_null: false,
                        default_value: None,
                        generated_from: None,
                        using_model: None,
                    });
                    changed = true;
                }
            }
            if changed {
                self.storage.set("_schemas", table, &schema)?;
                self.storage.set("_tables", table, &true)?;
                self.storage.flush()?;
            }
        } else {
            // Table doesn't exist yet, create it with new_columns
            let primary_col_name = new_columns.iter().find(|c| c.eq_ignore_ascii_case("id") || c.eq_ignore_ascii_case("_id"))
                .or_else(|| new_columns.iter().find(|c| c.to_lowercase().ends_with("_id")))
                .or_else(|| new_columns.first());

            let mut schema_cols = Vec::new();
            for col in new_columns {
                let is_pk = primary_col_name.map_or(false, |pk| pk == col);
                schema_cols.push(ColumnDef {
                    name: col.clone(),
                    data_type: "TEXT".to_string(),
                    is_primary: is_pk,
                    auto_increment: false,
                    is_unique: is_pk,
                    is_not_null: is_pk,
                    default_value: None,
                    generated_from: None,
                    using_model: None,
                });
            }
            let schema = crate::exec::TableSchema {
                name: base_name.to_string(),
                columns: schema_cols,
            };
            self.storage.set("_schemas", table, &schema)?;
            self.storage.set("_tables", table, &true)?;
            self.storage.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.storage.flush()?;
        Ok(())
    }
}

pub struct FileExportTarget {
    _file_path: PathBuf,
    format: String, // "csv" or "json"
    file: File,
    headers_written: bool,
}

impl FileExportTarget {
    pub fn new<P: AsRef<Path>>(path: P, format: &str) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        let file = File::create(&p)?;
        Ok(Self {
            _file_path: p,
            format: format.to_lowercase(),
            file,
            headers_written: false,
        })
    }
}

impl MigrationTarget for FileExportTarget {
    fn write_batch(&mut self, _table: &str, rows: &[RowData]) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }

        if self.format == "csv" {
            if !self.headers_written {
                let first = &rows[0];
                let mut cols: Vec<String> = first.columns.keys().cloned().collect();
                cols.sort();
                writeln!(self.file, "{}", cols.join(","))?;
                self.headers_written = true;
            }

            for row in rows {
                let mut cols: Vec<String> = row.columns.keys().cloned().collect();
                cols.sort();
                let line: Vec<String> = cols
                    .iter()
                    .map(|c| {
                        let val = row.columns.get(c).cloned().unwrap_or_default();
                        format!("\"{}\"", val.replace('"', "\"\""))
                    })
                    .collect();
                writeln!(self.file, "{}", line.join(","))?;
            }
        } else {
            // JSON Lines
            for row in rows {
                let json_line = serde_json::to_string(&row.columns)?;
                writeln!(self.file, "{}", json_line)?;
            }
        }

        Ok(rows.len())
    }

    fn update_schema_if_needed(&mut self, _table: &str, _new_columns: &[String]) -> Result<()> {
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        Ok(())
    }
}
