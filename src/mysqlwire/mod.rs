use crate::exec::Executor;
use anyhow::{bail, Result};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};

pub struct MySqlWireServer {
    executor: Arc<Executor>,
    bind_addr: String,
}

impl MySqlWireServer {
    pub fn new(executor: Arc<Executor>, bind_addr: String) -> Self {
        Self {
            executor,
            bind_addr,
        }
    }

    /// Spawns the MySQL Wire Protocol (v10) TCP listener on a background tokio task
    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(&self.bind_addr).await?;
        info!("MySQL Wire Protocol (v10) listening on {}", self.bind_addr);

        let exec = self.executor.clone();
        tokio::spawn(async move {
            let mut conn_id: u32 = 1;
            loop {
                match listener.accept().await {
                    Ok((socket, remote_addr)) => {
                        debug!("New MySQL connection #{} from {}", conn_id, remote_addr);
                        let client_exec = exec.clone();
                        let this_id = conn_id;
                        conn_id = conn_id.wrapping_add(1);
                        tokio::spawn(async move {
                            if let Err(e) = handle_mysql_client(socket, client_exec, this_id).await {
                                warn!("MySQL client #{} error: {:?}", this_id, e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("MySQL accept error: {:?}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }
}

// ─── MySQL Wire Protocol Constants & Helpers ───
const CLIENT_LONG_PASSWORD: u32 = 0x0000_0001;
const CLIENT_FOUND_ROWS: u32 = 0x0000_0002;
const CLIENT_LONG_FLAG: u32 = 0x0000_0004;
const CLIENT_CONNECT_WITH_DB: u32 = 0x0000_0008;
const CLIENT_PROTOCOL_41: u32 = 0x0000_0200;
const CLIENT_INTERACTIVE: u32 = 0x0000_0400;
const CLIENT_TRANSACTIONS: u32 = 0x0000_2000;
const CLIENT_SECURE_CONNECTION: u32 = 0x0000_8000;
const CLIENT_PLUGIN_AUTH: u32 = 0x0008_0000;
const CLIENT_PLUGIN_AUTH_LENENC_CLIENT_DATA: u32 = 0x0020_0000;

const SERVER_STATUS_AUTOCOMMIT: u16 = 0x0002;
const MYSQL_TYPE_VAR_STRING: u8 = 0xfd;

fn write_lenenc_int(buf: &mut Vec<u8>, val: u64) {
    if val < 251 {
        buf.push(val as u8);
    } else if val <= 0xffff {
        buf.push(0xfc);
        buf.extend_from_slice(&(val as u16).to_le_bytes());
    } else if val <= 0xffffff {
        buf.push(0xfd);
        buf.push((val & 0xff) as u8);
        buf.push(((val >> 8) & 0xff) as u8);
        buf.push(((val >> 16) & 0xff) as u8);
    } else {
        buf.push(0xfe);
        buf.extend_from_slice(&val.to_le_bytes());
    }
}

fn write_lenenc_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    write_lenenc_int(buf, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

fn write_null_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
}

async fn write_packet(stream: &mut TcpStream, seq_id: &mut u8, payload: &[u8]) -> Result<()> {
    let len = payload.len();
    let header = [
        (len & 0xff) as u8,
        ((len >> 8) & 0xff) as u8,
        ((len >> 16) & 0xff) as u8,
        *seq_id,
    ];
    stream.write_all(&header).await?;
    stream.write_all(payload).await?;
    *seq_id = seq_id.wrapping_add(1);
    Ok(())
}

async fn read_packet(stream: &mut TcpStream) -> Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    let len = (header[0] as usize) | ((header[1] as usize) << 8) | ((header[2] as usize) << 16);
    let seq_id = header[3];
    let mut payload = vec![0u8; len];
    if len > 0 {
        stream.read_exact(&mut payload).await?;
    }
    Ok((seq_id, payload))
}

fn make_ok_packet(affected_rows: u64, last_insert_id: u64, status_flags: u16, warnings: u16, info: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.push(0x00); // OK header
    write_lenenc_int(&mut p, affected_rows);
    write_lenenc_int(&mut p, last_insert_id);
    p.extend_from_slice(&status_flags.to_le_bytes());
    p.extend_from_slice(&warnings.to_le_bytes());
    if !info.is_empty() {
        p.extend_from_slice(info.as_bytes());
    }
    p
}

fn make_err_packet(error_code: u16, sql_state: &str, message: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.push(0xff); // ERR header
    p.extend_from_slice(&error_code.to_le_bytes());
    p.push(b'#'); // SQLSTATE marker
    let state_bytes = sql_state.as_bytes();
    if state_bytes.len() >= 5 {
        p.extend_from_slice(&state_bytes[..5]);
    } else {
        p.extend_from_slice(b"HY000");
    }
    p.extend_from_slice(message.as_bytes());
    p
}

fn make_eof_packet(warnings: u16, status_flags: u16) -> Vec<u8> {
    let mut p = Vec::new();
    p.push(0xfe); // EOF header
    p.extend_from_slice(&warnings.to_le_bytes());
    p.extend_from_slice(&status_flags.to_le_bytes());
    p
}

fn make_col_def_packet(catalog: &str, schema: &str, table: &str, col_name: &str, col_type: u8) -> Vec<u8> {
    let mut p = Vec::new();
    write_lenenc_str(&mut p, catalog);
    write_lenenc_str(&mut p, schema);
    write_lenenc_str(&mut p, table);
    write_lenenc_str(&mut p, table);
    write_lenenc_str(&mut p, col_name);
    write_lenenc_str(&mut p, col_name);
    p.push(0x0c); // Length of fixed fields: 12
    p.extend_from_slice(&45u16.to_le_bytes()); // Charset: utf8mb4 (45)
    p.extend_from_slice(&255u32.to_le_bytes()); // Column length
    p.push(col_type); // Column type
    p.extend_from_slice(&0u16.to_le_bytes()); // Flags
    p.push(0x00); // Decimals
    p.extend_from_slice(&[0u8; 2]); // Filler
    p
}

// ─── Handle Client Connection ───
async fn handle_mysql_client(mut stream: TcpStream, executor: Arc<Executor>, conn_id: u32) -> Result<()> {
    let mut seq_id: u8 = 0;

    // 1. Send Initial Handshake (HandshakeV10)
    let mut hs = Vec::new();
    hs.push(10); // Protocol version
    write_null_str(&mut hs, "8.0.35-PingDart-ClusterDB");
    hs.extend_from_slice(&conn_id.to_le_bytes());
    hs.extend_from_slice(b"pingdart"); // Salt part 1 (8 bytes)
    hs.push(0); // Filler

    // Capabilities (lower 2 bytes)
    let cap_low: u16 = (CLIENT_LONG_PASSWORD
        | CLIENT_FOUND_ROWS
        | CLIENT_LONG_FLAG
        | CLIENT_CONNECT_WITH_DB
        | CLIENT_PROTOCOL_41
        | CLIENT_INTERACTIVE
        | CLIENT_TRANSACTIONS
        | CLIENT_SECURE_CONNECTION) as u16;
    hs.extend_from_slice(&cap_low.to_le_bytes());

    hs.push(45); // utf8mb4_general_ci
    hs.extend_from_slice(&SERVER_STATUS_AUTOCOMMIT.to_le_bytes());

    // Capabilities (upper 2 bytes)
    let cap_high: u16 = ((CLIENT_PLUGIN_AUTH | CLIENT_PLUGIN_AUTH_LENENC_CLIENT_DATA) >> 16) as u16;
    hs.extend_from_slice(&cap_high.to_le_bytes());

    hs.push(21); // Auth plugin data length (8 + 12 + 1)
    hs.extend_from_slice(&[0u8; 10]); // Reserved

    hs.extend_from_slice(b"clusterdb123"); // Salt part 2 (12 bytes)
    hs.push(0); // Auth data terminator

    write_null_str(&mut hs, "mysql_native_password");

    write_packet(&mut stream, &mut seq_id, &hs).await?;

    // 2. Read HandshakeResponse41
    let (client_seq, response) = read_packet(&mut stream).await?;
    seq_id = client_seq.wrapping_add(1);

    if response.len() < 32 {
        bail!("Invalid HandshakeResponse packet length");
    }

    let client_cap = u32::from_le_bytes([response[0], response[1], response[2], response[3]]);
    let mut offset = 32;

    // Read username
    let user_end = response[offset..].iter().position(|&b| b == 0).unwrap_or(0);
    let username = std::str::from_utf8(&response[offset..offset + user_end]).unwrap_or("root").to_string();
    offset += user_end + 1;

    // Skip auth response
    if (client_cap & CLIENT_PLUGIN_AUTH_LENENC_CLIENT_DATA) != 0 {
        if offset < response.len() {
            let auth_len = response[offset] as usize;
            offset += 1 + auth_len;
        }
    } else if (client_cap & CLIENT_SECURE_CONNECTION) != 0 {
        if offset < response.len() {
            let auth_len = response[offset] as usize;
            offset += 1 + auth_len;
        }
    } else if offset < response.len() {
        let auth_end = response[offset..].iter().position(|&b| b == 0).unwrap_or(0);
        offset += auth_end + 1;
    }

    // Read database if CLIENT_CONNECT_WITH_DB
    let mut current_database: Option<String> = None;
    if (client_cap & CLIENT_CONNECT_WITH_DB) != 0 && offset < response.len() {
        let db_end = response[offset..].iter().position(|&b| b == 0).unwrap_or(0);
        if db_end > 0 {
            if let Ok(db_str) = std::str::from_utf8(&response[offset..offset + db_end]) {
                current_database = Some(db_str.to_string());
            }
        }
    }

    debug!("MySQL user '{}' authenticated, active db: {:?}", username, current_database);

    // 3. Send OK Packet to complete handshake
    let ok = make_ok_packet(0, 0, SERVER_STATUS_AUTOCOMMIT, 0, "");
    write_packet(&mut stream, &mut seq_id, &ok).await?;

    // 4. Command Loop
    loop {
        let (_seq, payload) = match read_packet(&mut stream).await {
            Ok(p) => p,
            Err(_) => break, // Client disconnected
        };

        if payload.is_empty() {
            continue;
        }

        let cmd = payload[0];
        let mut resp_seq: u8 = 1;

        match cmd {
            0x01 => {
                // COM_QUIT
                debug!("MySQL client #{} requested QUIT", conn_id);
                break;
            }
            0x02 => {
                // COM_INIT_DB
                let db_name = std::str::from_utf8(&payload[1..]).unwrap_or("default").trim().to_string();
                current_database = Some(db_name);
                let ok = make_ok_packet(0, 0, SERVER_STATUS_AUTOCOMMIT, 0, "");
                write_packet(&mut stream, &mut resp_seq, &ok).await?;
            }
            0x0e => {
                // COM_PING
                let ok = make_ok_packet(0, 0, SERVER_STATUS_AUTOCOMMIT, 0, "");
                write_packet(&mut stream, &mut resp_seq, &ok).await?;
            }
            0x1f => {
                // COM_RESET_CONNECTION
                let ok = make_ok_packet(0, 0, SERVER_STATUS_AUTOCOMMIT, 0, "");
                write_packet(&mut stream, &mut resp_seq, &ok).await?;
            }
            0x03 => {
                // COM_QUERY
                let sql = std::str::from_utf8(&payload[1..]).unwrap_or("").trim();
                debug!("MySQL #{} query: {}", conn_id, sql);

                handle_query(&mut stream, &mut resp_seq, &executor, sql, &mut current_database, &username).await?;
            }
            _ => {
                // For other unhandled commands, return OK
                let ok = make_ok_packet(0, 0, SERVER_STATUS_AUTOCOMMIT, 0, "");
                write_packet(&mut stream, &mut resp_seq, &ok).await?;
            }
        }
    }

    Ok(())
}

async fn handle_query(
    stream: &mut TcpStream,
    seq_id: &mut u8,
    executor: &Arc<Executor>,
    sql: &str,
    current_database: &mut Option<String>,
    username: &str,
) -> Result<()> {
    let sql_clean = sql.trim_end_matches(';').trim();
    let upper = sql_clean.to_uppercase();

    // ── Handle Client Initial Handshake Discovery Queries ──
    if upper == "SELECT @@VERSION_COMMENT LIMIT 1" || upper.starts_with("SELECT @@VERSION_COMMENT") {
        send_single_val_result(stream, seq_id, "@@version_comment", "PingDart ClusterDB Engine").await?;
        return Ok(());
    }

    if upper == "SELECT DATABASE()" || upper.starts_with("SELECT DATABASE()") {
        let db_str = current_database.as_deref().unwrap_or("default");
        send_single_val_result(stream, seq_id, "DATABASE()", db_str).await?;
        return Ok(());
    }

    if upper == "SELECT USER()" || upper == "SELECT CURRENT_USER()" {
        send_single_val_result(stream, seq_id, "USER()", username).await?;
        return Ok(());
    }

    if upper.starts_with("SET ") || upper.starts_with("SET NAMES") {
        let ok = make_ok_packet(0, 0, SERVER_STATUS_AUTOCOMMIT, 0, "");
        write_packet(stream, seq_id, &ok).await?;
        return Ok(());
    }

    if upper.starts_with("USE ") {
        let parts: Vec<&str> = sql_clean.split_whitespace().collect();
        if parts.len() >= 2 {
            let db = parts[1].trim_matches('`').trim_matches('\'').trim_matches('"').to_string();
            *current_database = Some(db);
        }
        let ok = make_ok_packet(0, 0, SERVER_STATUS_AUTOCOMMIT, 0, "");
        write_packet(stream, seq_id, &ok).await?;
        return Ok(());
    }

    if upper.starts_with("SHOW WARNINGS") || upper.starts_with("SHOW ERRORS") {
        send_empty_tabular_result(stream, seq_id, &["Level", "Code", "Message"]).await?;
        return Ok(());
    }

    if upper.starts_with("SHOW VARIABLES") || upper.starts_with("SHOW GLOBAL VARIABLES") || upper.starts_with("SHOW SESSION VARIABLES") {
        let vars = vec![
            ("auto_increment_increment", "1"),
            ("character_set_client", "utf8mb4"),
            ("character_set_connection", "utf8mb4"),
            ("character_set_results", "utf8mb4"),
            ("character_set_server", "utf8mb4"),
            ("collation_connection", "utf8mb4_general_ci"),
            ("collation_server", "utf8mb4_general_ci"),
            ("interactive_timeout", "28800"),
            ("lower_case_table_names", "0"),
            ("max_allowed_packet", "67108864"),
            ("net_buffer_length", "16384"),
            ("net_write_timeout", "60"),
            ("sql_mode", "NO_ENGINE_SUBSTITUTION"),
            ("version", "8.0.35-PingDart-ClusterDB"),
            ("version_comment", "PingDart ClusterDB Engine"),
            ("wait_timeout", "28800"),
        ];

        let mut rows = Vec::new();
        for (k, v) in vars {
            if upper.contains("LIKE") {
                let parts: Vec<&str> = sql_clean.split("LIKE").collect();
                if parts.len() >= 2 {
                    let pattern = parts[1].trim().trim_matches('\'').trim_matches('"').replace('%', "");
                    if !k.contains(&pattern) {
                        continue;
                    }
                }
            }
            rows.push(serde_json::json!({
                "Variable_name": k,
                "Value": v
            }));
        }
        send_json_array_result(stream, seq_id, &rows, current_database.as_deref().unwrap_or("default")).await?;
        return Ok(());
    }

    if upper.starts_with("SHOW DATABASES") || upper.starts_with("SHOW SCHEMAS") {
        match executor.execute_with_tenant("SHOW DATABASES;", current_database.as_deref(), None) {
            Ok(json_str) => {
                if let Ok(dbs) = serde_json::from_str::<Vec<String>>(&json_str) {
                    let rows: Vec<serde_json::Value> = dbs.into_iter().map(|d| serde_json::json!({ "Database": d })).collect();
                    send_json_array_result(stream, seq_id, &rows, "def").await?;
                    return Ok(());
                }
            }
            Err(e) => {
                let err = make_err_packet(1064, "42000", &e.to_string());
                write_packet(stream, seq_id, &err).await?;
                return Ok(());
            }
        }
    }

    if upper.starts_with("SELECT @@") {
        // Multi-variable session query commonly sent by DBeaver, MySQL Workbench, Navicat
        let cols: Vec<&str> = sql_clean
            .strip_prefix("SELECT ")
            .unwrap_or(sql_clean)
            .split(',')
            .map(|s| {
                if let Some(alias) = s.split(" AS ").nth(1) {
                    alias.trim()
                } else if let Some(alias) = s.split(" as ").nth(1) {
                    alias.trim()
                } else {
                    s.trim()
                }
            })
            .collect();

        let mut row_obj = serde_json::Map::new();
        for col in cols {
            let val = match col {
                c if c.contains("auto_increment_increment") => "1",
                c if c.contains("character_set") => "utf8mb4",
                c if c.contains("collation") => "utf8mb4_general_ci",
                c if c.contains("version") => "8.0.35-PingDart-ClusterDB",
                c if c.contains("sql_mode") => "NO_ENGINE_SUBSTITUTION",
                c if c.contains("time_zone") => "SYSTEM",
                c if c.contains("tx_isolation") => "REPEATABLE-READ",
                c if c.contains("max_allowed_packet") => "67108864",
                _ => "1",
            };
            row_obj.insert(col.to_string(), serde_json::Value::String(val.to_string()));
        }

        send_json_array_result(stream, seq_id, &[serde_json::Value::Object(row_obj)], "def").await?;
        return Ok(());
    }

    // ── Execute standard SQL via PingDart Unified Executor ──
    let exec_res = executor.execute_with_tenant(sql, current_database.as_deref(), None);

    match exec_res {
        Ok(result_str) => {
            if upper.starts_with("SELECT") || upper.starts_with("SHOW") || upper.starts_with("DESCRIBE") || upper.starts_with("EXPLAIN") {
                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&result_str) {
                    if let Some(arr) = json_val.as_array() {
                        send_json_array_result(stream, seq_id, arr, current_database.as_deref().unwrap_or("default")).await?;
                        return Ok(());
                    }
                }
                // If not array, send OK with info
                let ok = make_ok_packet(0, 0, SERVER_STATUS_AUTOCOMMIT, 0, &result_str);
                write_packet(stream, seq_id, &ok).await?;
            } else if upper.starts_with("INSERT") {
                let ok = make_ok_packet(1, 1, SERVER_STATUS_AUTOCOMMIT, 0, "");
                write_packet(stream, seq_id, &ok).await?;
            } else if upper.starts_with("UPDATE") || upper.starts_with("DELETE") {
                let ok = make_ok_packet(1, 0, SERVER_STATUS_AUTOCOMMIT, 0, "");
                write_packet(stream, seq_id, &ok).await?;
            } else {
                let ok = make_ok_packet(0, 0, SERVER_STATUS_AUTOCOMMIT, 0, "");
                write_packet(stream, seq_id, &ok).await?;
            }
        }
        Err(e) => {
            let err = make_err_packet(1064, "42000", &e.to_string());
            write_packet(stream, seq_id, &err).await?;
        }
    }

    Ok(())
}

async fn send_single_val_result(stream: &mut TcpStream, seq_id: &mut u8, col_name: &str, val: &str) -> Result<()> {
    // 1. Column count = 1
    let mut col_cnt = Vec::new();
    write_lenenc_int(&mut col_cnt, 1);
    write_packet(stream, seq_id, &col_cnt).await?;

    // 2. Column definition
    let col_def = make_col_def_packet("def", "", "", col_name, MYSQL_TYPE_VAR_STRING);
    write_packet(stream, seq_id, &col_def).await?;

    // 3. EOF packet
    let eof = make_eof_packet(0, SERVER_STATUS_AUTOCOMMIT);
    write_packet(stream, seq_id, &eof).await?;

    // 4. Single Row
    let mut row = Vec::new();
    write_lenenc_str(&mut row, val);
    write_packet(stream, seq_id, &row).await?;

    // 5. EOF packet
    let eof2 = make_eof_packet(0, SERVER_STATUS_AUTOCOMMIT);
    write_packet(stream, seq_id, &eof2).await?;

    Ok(())
}

async fn send_empty_tabular_result(stream: &mut TcpStream, seq_id: &mut u8, cols: &[&str]) -> Result<()> {
    let mut col_cnt = Vec::new();
    write_lenenc_int(&mut col_cnt, cols.len() as u64);
    write_packet(stream, seq_id, &col_cnt).await?;

    for c in cols {
        let col_def = make_col_def_packet("def", "", "", c, MYSQL_TYPE_VAR_STRING);
        write_packet(stream, seq_id, &col_def).await?;
    }

    let eof = make_eof_packet(0, SERVER_STATUS_AUTOCOMMIT);
    write_packet(stream, seq_id, &eof).await?;

    let eof2 = make_eof_packet(0, SERVER_STATUS_AUTOCOMMIT);
    write_packet(stream, seq_id, &eof2).await?;

    Ok(())
}

async fn send_json_array_result(
    stream: &mut TcpStream,
    seq_id: &mut u8,
    arr: &[serde_json::Value],
    schema: &str,
) -> Result<()> {
    if arr.is_empty() {
        // Return 1 dummy column with 0 rows
        send_empty_tabular_result(stream, seq_id, &["Result"]).await?;
        return Ok(());
    }

    let columns: Vec<String> = if let Some(first_obj) = arr[0].as_object() {
        first_obj.keys().cloned().collect()
    } else {
        vec!["Value".to_string()]
    };

    // 1. Column count packet
    let mut col_cnt = Vec::new();
    write_lenenc_int(&mut col_cnt, columns.len() as u64);
    write_packet(stream, seq_id, &col_cnt).await?;

    // 2. Column definitions
    for col in &columns {
        let col_def = make_col_def_packet("def", schema, "", col, MYSQL_TYPE_VAR_STRING);
        write_packet(stream, seq_id, &col_def).await?;
    }

    // 3. EOF packet (columns done)
    let eof = make_eof_packet(0, SERVER_STATUS_AUTOCOMMIT);
    write_packet(stream, seq_id, &eof).await?;

    // 4. Row packets
    for item in arr {
        let mut row_payload = Vec::new();
        if let Some(obj) = item.as_object() {
            for col in &columns {
                if let Some(v) = obj.get(col) {
                    if v.is_null() {
                        row_payload.push(0xfb); // NULL
                    } else if let Some(s) = v.as_str() {
                        write_lenenc_str(&mut row_payload, s);
                    } else {
                        write_lenenc_str(&mut row_payload, &v.to_string());
                    }
                } else {
                    row_payload.push(0xfb);
                }
            }
        } else if let Some(s) = item.as_str() {
            write_lenenc_str(&mut row_payload, s);
        } else {
            write_lenenc_str(&mut row_payload, &item.to_string());
        }
        write_packet(stream, seq_id, &row_payload).await?;
    }

    // 5. EOF packet (rows done)
    let eof2 = make_eof_packet(0, SERVER_STATUS_AUTOCOMMIT);
    write_packet(stream, seq_id, &eof2).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageEngine;

    #[tokio::test]
    async fn test_mysqlwire_handshake_and_query() {
        let storage = StorageEngine::new_temp().unwrap();
        let executor = Arc::new(Executor::new(storage));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn mock mysqlwire listener
        tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await {
                let _ = handle_mysql_client(socket, executor, 1).await;
            }
        });

        let mut client = TcpStream::connect(format!("127.0.0.1:{}", port)).await.unwrap();

        // 1. Read Initial Handshake from server
        let (seq, hs_payload) = read_packet(&mut client).await.unwrap();
        assert_eq!(seq, 0);
        assert_eq!(hs_payload[0], 10); // Protocol 10
        assert!(hs_payload.windows(8).any(|w| w == b"pingdart"));

        // 2. Send HandshakeResponse41
        let mut resp = Vec::new();
        // Client capabilities (4 bytes)
        resp.extend_from_slice(&(CLIENT_PROTOCOL_41 | CLIENT_SECURE_CONNECTION).to_le_bytes());
        // Max packet size (4 bytes)
        resp.extend_from_slice(&16777216u32.to_le_bytes());
        // Charset (1 byte)
        resp.push(45);
        // Reserved (23 bytes)
        resp.extend_from_slice(&[0u8; 23]);
        // Username ("root\0")
        resp.extend_from_slice(b"root\0");
        // Auth response (length = 0)
        resp.push(0);

        let mut client_seq = 1;
        write_packet(&mut client, &mut client_seq, &resp).await.unwrap();

        // 3. Expect OK Packet (0x00)
        let (ok_seq, ok_payload) = read_packet(&mut client).await.unwrap();
        assert_eq!(ok_seq, 2);
        assert_eq!(ok_payload[0], 0x00);

        // 4. Send COM_QUERY: SELECT @@version_comment
        let mut q = Vec::new();
        q.push(0x03); // COM_QUERY
        q.extend_from_slice(b"SELECT @@version_comment LIMIT 1");
        let mut q_seq = 0;
        write_packet(&mut client, &mut q_seq, &q).await.unwrap();

        // Expect column count packet = 1
        let (_, col_cnt) = read_packet(&mut client).await.unwrap();
        assert_eq!(col_cnt[0], 1);

        // Expect column definition packet
        let (_, col_def) = read_packet(&mut client).await.unwrap();
        assert!(col_def.windows(17).any(|w| w == b"@@version_comment"));

        // Expect EOF packet
        let (_, eof1) = read_packet(&mut client).await.unwrap();
        assert_eq!(eof1[0], 0xfe);

        // Expect Row packet containing "PingDart ClusterDB Engine"
        let (_, row) = read_packet(&mut client).await.unwrap();
        assert!(row.windows(8).any(|w| w == b"PingDart"));

        // Expect final EOF packet
        let (_, eof2) = read_packet(&mut client).await.unwrap();
        assert_eq!(eof2[0], 0xfe);
    }
}
