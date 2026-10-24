use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub trait MigrationSource: Send + Sync {
    fn next_batch(&mut self, batch_size: usize) -> Result<Vec<HashMap<String, serde_json::Value>>>;
    fn total_estimated_records(&self) -> Option<usize>;
    fn current_offset(&self) -> u64;
    fn seek_offset(&mut self, offset: u64) -> Result<()>;
    fn source_type(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// 1. CSV Source
// ---------------------------------------------------------------------------
pub struct CsvSource {
    file_path: PathBuf,
    reader: BufReader<File>,
    headers: Vec<String>,
    current_byte_offset: u64,
    delimiter: char,
}

impl CsvSource {
    pub fn new<P: AsRef<Path>>(path: P, delimiter: Option<char>) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        let file = File::open(&p)?;
        let mut reader = BufReader::new(file);

        let delim = delimiter.unwrap_or(',');
        let mut first_line = String::new();
        let bytes_read = reader.read_line(&mut first_line)?;
        if bytes_read == 0 {
            return Err(anyhow!("CSV file is empty"));
        }

        let headers: Vec<String> = parse_csv_line(&first_line, delim)
            .into_iter()
            .map(|s| s.trim().trim_matches('"').to_string())
            .collect();

        Ok(Self {
            file_path: p,
            reader,
            headers,
            current_byte_offset: bytes_read as u64,
            delimiter: delim,
        })
    }
}

impl MigrationSource for CsvSource {
    fn next_batch(&mut self, batch_size: usize) -> Result<Vec<HashMap<String, serde_json::Value>>> {
        let mut batch = Vec::with_capacity(batch_size);
        let mut line = String::new();

        while batch.len() < batch_size {
            line.clear();
            let bytes_read = self.reader.read_line(&mut line)?;
            if bytes_read == 0 {
                break;
            }
            self.current_byte_offset += bytes_read as u64;

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let fields = parse_csv_line(trimmed, self.delimiter);
            let mut row = HashMap::new();
            for (idx, header) in self.headers.iter().enumerate() {
                if let Some(val) = fields.get(idx) {
                    let cleaned = val.trim().trim_matches('"');
                    row.insert(header.clone(), serde_json::Value::String(cleaned.to_string()));
                }
            }
            batch.push(row);
        }

        Ok(batch)
    }

    fn total_estimated_records(&self) -> Option<usize> {
        None
    }

    fn current_offset(&self) -> u64 {
        self.current_byte_offset
    }

    fn seek_offset(&mut self, offset: u64) -> Result<()> {
        let file = File::open(&self.file_path)?;
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(offset))?;
        self.reader = reader;
        self.current_byte_offset = offset;
        Ok(())
    }

    fn source_type(&self) -> &'static str {
        "csv"
    }
}

fn parse_csv_line(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;

    for c in line.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == delimiter && !in_quotes {
            fields.push(cur.clone());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    fields.push(cur);
    fields
}

// ---------------------------------------------------------------------------
// 2. JSON & JSONL Source
// ---------------------------------------------------------------------------
pub struct JsonSource {
    file_path: PathBuf,
    reader: BufReader<File>,
    current_byte_offset: u64,
    is_jsonl: bool,
    array_records: Option<Vec<HashMap<String, serde_json::Value>>>,
    array_index: usize,
}

impl JsonSource {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        let file = File::open(&p)?;
        let mut reader = BufReader::new(file);

        // Peek first char to see if array or jsonl
        let mut first_line = String::new();
        let _bytes_read = reader.read_line(&mut first_line)?;
        let trimmed = first_line.trim();

        if trimmed.starts_with('[') {
            // Whole file is a JSON array
            let full_text = std::fs::read_to_string(&p)?;
            let raw_array: Vec<HashMap<String, serde_json::Value>> = serde_json::from_str(&full_text)?;
            let array = raw_array.into_iter().map(Self::normalize_json_record).collect();
            Ok(Self {
                file_path: p,
                reader,
                current_byte_offset: full_text.len() as u64,
                is_jsonl: false,
                array_records: Some(array),
                array_index: 0,
            })
        } else if trimmed.starts_with('{') {
            let full_text = std::fs::read_to_string(&p)?;
            if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&full_text) {
                // If it's a map where values are objects (Firebase Realtime DB collection export: {"id1": {...}, "id2": {...}})
                let is_collection_map = !map.is_empty() && map.values().all(|v| v.is_object());
                if is_collection_map {
                    let mut array = Vec::new();
                    for (k, v) in map {
                        if let serde_json::Value::Object(mut inner_map) = v {
                            if !inner_map.contains_key("id") && !inner_map.contains_key("_id") {
                                inner_map.insert("id".to_string(), serde_json::Value::String(k));
                            }
                            let record: HashMap<String, serde_json::Value> = inner_map.into_iter().collect();
                            array.push(Self::normalize_json_record(record));
                        }
                    }
                    return Ok(Self {
                        file_path: p,
                        reader,
                        current_byte_offset: full_text.len() as u64,
                        is_jsonl: false,
                        array_records: Some(array),
                        array_index: 0,
                    });
                }
            }
            // Otherwise, treat as JSONL (newline-delimited JSON)
            let file = File::open(&p)?;
            Ok(Self {
                file_path: p,
                reader: BufReader::new(file),
                current_byte_offset: 0,
                is_jsonl: true,
                array_records: None,
                array_index: 0,
            })
        } else {
            // Treat as JSONL (newline-delimited JSON)
            let file = File::open(&p)?;
            Ok(Self {
                file_path: p,
                reader: BufReader::new(file),
                current_byte_offset: 0,
                is_jsonl: true,
                array_records: None,
                array_index: 0,
            })
        }
    }

    pub fn normalize_json_record(raw: HashMap<String, serde_json::Value>) -> HashMap<String, serde_json::Value> {
        let mut out = HashMap::new();
        for (k, v) in raw {
            let norm_k = if k == "_id" { "id".to_string() } else { k };
            let norm_v = match v {
                serde_json::Value::Object(ref map) => {
                    // Normalize MongoDB Extended JSON: $oid, $date, $numberLong, $numberInt
                    if let Some(oid) = map.get("$oid") {
                        oid.clone()
                    } else if let Some(date) = map.get("$date") {
                        date.clone()
                    } else if let Some(long) = map.get("$numberLong") {
                        long.clone()
                    } else if let Some(int) = map.get("$numberInt") {
                        int.clone()
                    }
                    // Normalize Firebase / Firestore Timestamps: {_seconds: 1234567890, _nanoseconds: 0}
                    else if let Some(sec) = map.get("_seconds") {
                        sec.clone()
                    } else {
                        serde_json::Value::String(v.to_string())
                    }
                }
                _ => v,
            };
            out.insert(norm_k, norm_v);
        }
        out
    }
}

impl MigrationSource for JsonSource {
    fn next_batch(&mut self, batch_size: usize) -> Result<Vec<HashMap<String, serde_json::Value>>> {
        if let Some(ref arr) = self.array_records {
            if self.array_index >= arr.len() {
                return Ok(Vec::new());
            }
            let end = (self.array_index + batch_size).min(arr.len());
            let batch = arr[self.array_index..end].to_vec();
            self.array_index = end;
            return Ok(batch);
        }

        let mut batch = Vec::with_capacity(batch_size);
        let mut line = String::new();

        while batch.len() < batch_size {
            line.clear();
            let bytes_read = self.reader.read_line(&mut line)?;
            if bytes_read == 0 {
                break;
            }
            self.current_byte_offset += bytes_read as u64;

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(obj) = serde_json::from_str::<HashMap<String, serde_json::Value>>(trimmed) {
                batch.push(Self::normalize_json_record(obj));
            }
        }

        Ok(batch)
    }

    fn total_estimated_records(&self) -> Option<usize> {
        self.array_records.as_ref().map(|a| a.len())
    }

    fn current_offset(&self) -> u64 {
        if self.is_jsonl {
            self.current_byte_offset
        } else {
            self.array_index as u64
        }
    }

    fn seek_offset(&mut self, offset: u64) -> Result<()> {
        if self.is_jsonl {
            let file = File::open(&self.file_path)?;
            let mut reader = BufReader::new(file);
            reader.seek(SeekFrom::Start(offset))?;
            self.reader = reader;
            self.current_byte_offset = offset;
        } else {
            self.array_index = offset as usize;
        }
        Ok(())
    }

    fn source_type(&self) -> &'static str {
        "json"
    }
}

// ---------------------------------------------------------------------------
// 3. MongoDB Extended JSON Source
// ---------------------------------------------------------------------------
pub struct MongoJsonSource {
    inner: JsonSource,
}

impl MongoJsonSource {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self {
            inner: JsonSource::new(path)?,
        })
    }

    pub fn normalize_mongo_record(raw: HashMap<String, serde_json::Value>) -> HashMap<String, serde_json::Value> {
        let mut out = HashMap::new();
        for (k, v) in raw {
            let norm_k = if k == "_id" { "id".to_string() } else { k };
            let norm_v = match v {
                serde_json::Value::Object(ref map) => {
                    if let Some(oid) = map.get("$oid") {
                        oid.clone()
                    } else if let Some(date) = map.get("$date") {
                        date.clone()
                    } else if let Some(long) = map.get("$numberLong") {
                        long.clone()
                    } else if let Some(int) = map.get("$numberInt") {
                        int.clone()
                    } else {
                        serde_json::Value::String(v.to_string())
                    }
                }
                _ => v,
            };
            out.insert(norm_k, norm_v);
        }
        out
    }
}

impl MigrationSource for MongoJsonSource {
    fn next_batch(&mut self, batch_size: usize) -> Result<Vec<HashMap<String, serde_json::Value>>> {
        let raw_batch = self.inner.next_batch(batch_size)?;
        Ok(raw_batch.into_iter().map(Self::normalize_mongo_record).collect())
    }

    fn total_estimated_records(&self) -> Option<usize> {
        self.inner.total_estimated_records()
    }

    fn current_offset(&self) -> u64 {
        self.inner.current_offset()
    }

    fn seek_offset(&mut self, offset: u64) -> Result<()> {
        self.inner.seek_offset(offset)
    }

    fn source_type(&self) -> &'static str {
        "mongodb"
    }
}

// ---------------------------------------------------------------------------
// 4. PostgreSQL Dump Source
// ---------------------------------------------------------------------------
pub struct PostgresDumpSource {
    file_path: PathBuf,
    reader: BufReader<File>,
    current_byte_offset: u64,
}

impl PostgresDumpSource {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        let file = File::open(&p)?;
        Ok(Self {
            file_path: p,
            reader: BufReader::new(file),
            current_byte_offset: 0,
        })
    }
}

impl MigrationSource for PostgresDumpSource {
    fn next_batch(&mut self, batch_size: usize) -> Result<Vec<HashMap<String, serde_json::Value>>> {
        let mut batch = Vec::with_capacity(batch_size);
        let mut line = String::new();

        while batch.len() < batch_size {
            line.clear();
            let bytes_read = self.reader.read_line(&mut line)?;
            if bytes_read == 0 {
                break;
            }
            self.current_byte_offset += bytes_read as u64;

            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("--") || trimmed.starts_with("/*") {
                continue;
            }

            // Parse INSERT INTO table (col1, col2) VALUES (val1, val2);
            let lower = trimmed.to_lowercase();
            if lower.starts_with("insert into ") {
                let mut statement = trimmed.to_string();
                while !statement.trim_end().ends_with(';') {
                    line.clear();
                    let b = self.reader.read_line(&mut line)?;
                    if b == 0 {
                        break;
                    }
                    self.current_byte_offset += b as u64;
                    statement.push(' ');
                    statement.push_str(line.trim());
                }
                if let Some(records) = parse_sql_insert_statement(&statement) {
                    batch.extend(records);
                }
            }
        }

        Ok(batch)
    }

    fn total_estimated_records(&self) -> Option<usize> {
        None
    }

    fn current_offset(&self) -> u64 {
        self.current_byte_offset
    }

    fn seek_offset(&mut self, offset: u64) -> Result<()> {
        let file = File::open(&self.file_path)?;
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(offset))?;
        self.reader = reader;
        self.current_byte_offset = offset;
        Ok(())
    }

    fn source_type(&self) -> &'static str {
        "postgresql"
    }
}

// ---------------------------------------------------------------------------
// 5. MySQL Dump Source
// ---------------------------------------------------------------------------
pub struct MySQLDumpSource {
    file_path: PathBuf,
    reader: BufReader<File>,
    current_byte_offset: u64,
}

impl MySQLDumpSource {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        let file = File::open(&p)?;
        Ok(Self {
            file_path: p,
            reader: BufReader::new(file),
            current_byte_offset: 0,
        })
    }
}

impl MigrationSource for MySQLDumpSource {
    fn next_batch(&mut self, batch_size: usize) -> Result<Vec<HashMap<String, serde_json::Value>>> {
        let mut batch = Vec::with_capacity(batch_size);
        let mut line = String::new();

        while batch.len() < batch_size {
            line.clear();
            let bytes_read = self.reader.read_line(&mut line)?;
            if bytes_read == 0 {
                break;
            }
            self.current_byte_offset += bytes_read as u64;

            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("--") || trimmed.starts_with("/*") {
                continue;
            }

            let lower = trimmed.to_lowercase();
            if lower.starts_with("insert into ") {
                let mut statement = trimmed.to_string();
                while !statement.trim_end().ends_with(';') {
                    line.clear();
                    let b = self.reader.read_line(&mut line)?;
                    if b == 0 {
                        break;
                    }
                    self.current_byte_offset += b as u64;
                    statement.push(' ');
                    statement.push_str(line.trim());
                }
                if let Some(records) = parse_sql_insert_statement(&statement) {
                    batch.extend(records);
                }
            }
        }

        Ok(batch)
    }

    fn total_estimated_records(&self) -> Option<usize> {
        None
    }

    fn current_offset(&self) -> u64 {
        self.current_byte_offset
    }

    fn seek_offset(&mut self, offset: u64) -> Result<()> {
        let file = File::open(&self.file_path)?;
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(offset))?;
        self.reader = reader;
        self.current_byte_offset = offset;
        Ok(())
    }

    fn source_type(&self) -> &'static str {
        "mysql"
    }
}

/// Generic SQL INSERT statement parser for MySQL & PostgreSQL
/// Syntax: INSERT INTO `table` (`col1`, `col2`) VALUES ('val1', 2), ('val2', 3);
fn parse_sql_insert_statement(sql: &str) -> Option<Vec<HashMap<String, serde_json::Value>>> {
    let lower = sql.to_lowercase();
    let values_idx = lower.find(" values")?;
    let prefix = &sql[..values_idx];
    let after_values = &sql[values_idx + 7..].trim().trim_end_matches(';').trim();

    // Extract columns if present: INSERT INTO table (c1, c2)
    let cols_start = prefix.find('(');
    let cols_end = prefix.rfind(')');
    let columns: Option<Vec<String>> = if let (Some(s), Some(e)) = (cols_start, cols_end) {
        if e > s {
            let col_str = &prefix[s + 1..e];
            Some(
                col_str
                    .split(',')
                    .map(|c| c.trim().trim_matches(|ch| ch == '`' || ch == '"' || ch == '\'').to_string())
                    .collect(),
            )
        } else {
            None
        }
    } else {
        None
    };

    // Parse tuple values: (1, 'name'), (2, 'other')
    let mut results = Vec::new();
    let tuples = split_tuples(after_values);

    for tuple in tuples {
        let raw_fields = split_csv_like(&tuple);
        let mut map = HashMap::new();
        for (i, field) in raw_fields.into_iter().enumerate() {
            let cleaned = field.trim().trim_matches(|c| c == '\'' || c == '"');
            let col_name = if let Some(ref cols) = columns {
                cols.get(i).cloned().unwrap_or_else(|| format!("col_{}", i))
            } else {
                format!("col_{}", i)
            };
            map.insert(col_name, serde_json::Value::String(cleaned.to_string()));
        }
        results.push(map);
    }

    Some(results)
}

fn split_tuples(s: &str) -> Vec<String> {
    let mut tuples = Vec::new();
    let mut cur = String::new();
    let mut in_tuple = false;
    let mut in_quote = false;

    for c in s.chars() {
        if c == '\'' || c == '"' {
            in_quote = !in_quote;
            if in_tuple {
                cur.push(c);
            }
        } else if c == '(' && !in_quote {
            in_tuple = true;
            cur.clear();
        } else if c == ')' && !in_quote {
            in_tuple = false;
            tuples.push(cur.clone());
            cur.clear();
        } else if in_tuple {
            cur.push(c);
        }
    }
    tuples
}

fn split_csv_like(s: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;

    for c in s.chars() {
        if c == '\'' || c == '"' {
            in_quote = !in_quote;
        } else if c == ',' && !in_quote {
            fields.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    fields.push(cur.trim().to_string());
    fields
}
