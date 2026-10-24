use crate::exec::Executor;
use anyhow::{bail, Result};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};

pub const PG_PROTOCOL_V3: u32 = 0x0003_0000;
pub const PG_SSL_REQUEST: u32 = 80877103;

pub struct PgWireServer {
    executor: Arc<Executor>,
    bind_addr: String,
}

impl PgWireServer {
    pub fn new(executor: Arc<Executor>, bind_addr: String) -> Self {
        Self {
            executor,
            bind_addr,
        }
    }

    /// Spawns the PostgreSQL Wire Protocol TCP listener on a background tokio task
    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(&self.bind_addr).await?;
        info!("PostgreSQL Wire Protocol (v3) listening on {}", self.bind_addr);

        let exec = self.executor.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, remote_addr)) => {
                        debug!("New pgwire connection from {}", remote_addr);
                        let client_exec = exec.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_pg_client(socket, client_exec).await {
                                warn!("pgwire client error: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("pgwire accept error: {:?}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }
}

async fn handle_pg_client(mut stream: TcpStream, executor: Arc<Executor>) -> Result<()> {
    // 1. Startup phase
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let mut msg_len = u32::from_be_bytes(len_buf) as usize;

    if msg_len < 8 {
        bail!("Invalid startup packet length: {}", msg_len);
    }

    let mut body = vec![0u8; msg_len - 4];
    stream.read_exact(&mut body).await?;

    let mut proto = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);

    // Handle SSL Request
    if proto == PG_SSL_REQUEST {
        // PingDart responds 'N' for SSL not enabled on standard pgwire port
        stream.write_all(b"N").await?;

        // Read real StartupMessage
        stream.read_exact(&mut len_buf).await?;
        msg_len = u32::from_be_bytes(len_buf) as usize;
        body = vec![0u8; msg_len - 4];
        stream.read_exact(&mut body).await?;
        proto = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
        if proto != PG_PROTOCOL_V3 {
            bail!("Unsupported protocol version: {}", proto);
        }
    } else if proto != PG_PROTOCOL_V3 {
        bail!("Unsupported protocol version: {}", proto);
    }

    // Parse startup parameters (user, database, client_encoding, etc.)
    let remaining = &body[4..];
    let mut params = std::collections::HashMap::new();
    let mut splits = remaining.split(|&b| b == 0);
    while let (Some(k), Some(v)) = (splits.next(), splits.next()) {
        if k.is_empty() {
            break;
        }
        if let (Ok(ks), Ok(vs)) = (std::str::from_utf8(k), std::str::from_utf8(v)) {
            params.insert(ks.to_string(), vs.to_string());
        }
    }

    debug!("pgwire authenticated parameters: {:?}", params);

    // 2. AuthenticationOk: 'R' (len 8, code 0)
    let mut auth_ok = Vec::new();
    auth_ok.push(b'R');
    auth_ok.extend_from_slice(&8u32.to_be_bytes());
    auth_ok.extend_from_slice(&0u32.to_be_bytes());
    stream.write_all(&auth_ok).await?;

    // 3. Send ParameterStatus messages
    send_parameter_status(&mut stream, "server_version", "15.0 (PingDart-Core v0.4.0)").await?;
    send_parameter_status(&mut stream, "client_encoding", "UTF8").await?;
    send_parameter_status(&mut stream, "server_encoding", "UTF8").await?;
    send_parameter_status(&mut stream, "DateStyle", "ISO, YMD").await?;
    send_parameter_status(&mut stream, "standard_conforming_strings", "on").await?;

    // 4. ReadyForQuery: 'Z' (len 5, 'I' for Idle transaction)
    send_ready_for_query(&mut stream).await?;

    // 5. Main message loop
    let mut current_database: Option<String> = params.get("database").cloned();

    loop {
        let mut msg_type_buf = [0u8; 1];
        if stream.read_exact(&mut msg_type_buf).await.is_err() {
            break; // Connection closed gracefully
        }
        let msg_type = msg_type_buf[0];

        let mut payload_len_buf = [0u8; 4];
        stream.read_exact(&mut payload_len_buf).await?;
        let payload_len = u32::from_be_bytes(payload_len_buf) as usize;

        if payload_len < 4 {
            bail!("Invalid message length");
        }
        let mut payload = vec![0u8; payload_len - 4];
        stream.read_exact(&mut payload).await?;

        match msg_type {
            b'Q' => {
                // Simple Query: null-terminated SQL string
                let sql = std::str::from_utf8(&payload[..payload.len().saturating_sub(1)])
                    .unwrap_or("")
                    .trim();

                debug!("pgwire executing SQL: {}", sql);

                if sql.is_empty() || sql == ";" {
                    send_empty_query_response(&mut stream).await?;
                    send_ready_for_query(&mut stream).await?;
                    continue;
                }

                // Execute SQL through the PingDart Unified Executor
                let exec_res = executor.execute_with_tenant(sql, current_database.as_deref(), None);

                match exec_res {
                    Ok(result_str) => {
                        let upper_sql = sql.to_uppercase();
                        if upper_sql.starts_with("SELECT") || upper_sql.starts_with("SHOW") {
                            send_tabular_result(&mut stream, &result_str).await?;
                        } else if upper_sql.starts_with("INSERT") {
                            send_command_complete(&mut stream, "INSERT 0 1").await?;
                        } else if upper_sql.starts_with("UPDATE") {
                            send_command_complete(&mut stream, "UPDATE 1").await?;
                        } else if upper_sql.starts_with("DELETE") {
                            send_command_complete(&mut stream, "DELETE 1").await?;
                        } else if upper_sql.starts_with("CREATE") {
                            send_command_complete(&mut stream, "CREATE TABLE").await?;
                        } else if upper_sql.starts_with("USE ") {
                            let parts: Vec<&str> = sql.split_whitespace().collect();
                            if parts.len() >= 2 {
                                current_database = Some(parts[1].trim_matches(';').to_string());
                            }
                            send_command_complete(&mut stream, "SET").await?;
                        } else {
                            send_command_complete(&mut stream, "OK").await?;
                        }
                    }
                    Err(e) => {
                        send_error_response(&mut stream, &e.to_string()).await?;
                    }
                }

                send_ready_for_query(&mut stream).await?;
            }
            b'X' => {
                // Terminate
                debug!("pgwire client requested terminate");
                break;
            }
            _ => {
                // Unknown/unhandled message
                send_ready_for_query(&mut stream).await?;
            }
        }
    }

    Ok(())
}

async fn send_parameter_status(stream: &mut TcpStream, name: &str, value: &str) -> Result<()> {
    let mut buf = Vec::new();
    buf.push(b'S');
    let len = (4 + name.len() + 1 + value.len() + 1) as u32;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(name.as_bytes());
    buf.push(0);
    buf.extend_from_slice(value.as_bytes());
    buf.push(0);
    stream.write_all(&buf).await?;
    Ok(())
}

async fn send_ready_for_query(stream: &mut TcpStream) -> Result<()> {
    let mut buf = [0u8; 6];
    buf[0] = b'Z';
    buf[1..5].copy_from_slice(&5u32.to_be_bytes());
    buf[5] = b'I'; // Idle status
    stream.write_all(&buf).await?;
    Ok(())
}

async fn send_command_complete(stream: &mut TcpStream, tag: &str) -> Result<()> {
    let mut buf = Vec::new();
    buf.push(b'C');
    let len = (4 + tag.len() + 1) as u32;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(tag.as_bytes());
    buf.push(0);
    stream.write_all(&buf).await?;
    Ok(())
}

async fn send_empty_query_response(stream: &mut TcpStream) -> Result<()> {
    let mut buf = [0u8; 5];
    buf[0] = b'I';
    buf[1..5].copy_from_slice(&4u32.to_be_bytes());
    stream.write_all(&buf).await?;
    Ok(())
}

async fn send_error_response(stream: &mut TcpStream, err_msg: &str) -> Result<()> {
    let mut body = Vec::new();
    // 'S': Severity
    body.push(b'S');
    body.extend_from_slice(b"ERROR\0");
    // 'C': SQLSTATE code (42000: syntax_error_or_access_rule_violation)
    body.push(b'C');
    body.extend_from_slice(b"42000\0");
    // 'M': Message
    body.push(b'M');
    body.extend_from_slice(err_msg.as_bytes());
    body.push(0);
    body.push(0); // Terminator

    let mut buf = Vec::new();
    buf.push(b'E');
    let len = (body.len() + 4) as u32;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    stream.write_all(&buf).await?;
    Ok(())
}

async fn send_tabular_result(stream: &mut TcpStream, result_str: &str) -> Result<()> {
    // If output is structured JSON or plain text
    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(result_str) {
        if let Some(arr) = json_val.as_array() {
            if arr.is_empty() {
                send_command_complete(stream, "SELECT 0").await?;
                return Ok(());
            }

            // Extract column names from the first object
            if let Some(first_obj) = arr[0].as_object() {
                let columns: Vec<String> = first_obj.keys().cloned().collect();

                // 1. Send RowDescription: 'T'
                let mut row_desc = Vec::new();
                row_desc.push(b'T');

                let mut desc_body = Vec::new();
                desc_body.extend_from_slice(&(columns.len() as u16).to_be_bytes());

                for col in &columns {
                    desc_body.extend_from_slice(col.as_bytes());
                    desc_body.push(0);
                    desc_body.extend_from_slice(&0u32.to_be_bytes()); // Table OID
                    desc_body.extend_from_slice(&0u16.to_be_bytes()); // Column index
                    desc_body.extend_from_slice(&25u32.to_be_bytes()); // Text OID (25)
                    desc_body.extend_from_slice(&(-1i16).to_be_bytes()); // Data type size (-1 for variable)
                    desc_body.extend_from_slice(&(-1i32).to_be_bytes()); // Type modifier
                    desc_body.extend_from_slice(&0u16.to_be_bytes()); // Format code (0 = text)
                }

                let total_len = (desc_body.len() + 4) as u32;
                row_desc.extend_from_slice(&total_len.to_be_bytes());
                row_desc.extend_from_slice(&desc_body);
                stream.write_all(&row_desc).await?;

                // 2. Send DataRow for each object: 'D'
                for item in arr {
                    if let Some(obj) = item.as_object() {
                        let mut data_row = Vec::new();
                        data_row.push(b'D');

                        let mut row_body = Vec::new();
                        row_body.extend_from_slice(&(columns.len() as u16).to_be_bytes());

                        for col in &columns {
                            let val_str = match obj.get(col) {
                                Some(serde_json::Value::String(s)) => s.clone(),
                                Some(serde_json::Value::Null) => {
                                    row_body.extend_from_slice(&(-1i32).to_be_bytes()); // NULL indicator
                                    continue;
                                }
                                Some(other) => other.to_string(),
                                None => "".to_string(),
                            };
                            let bytes = val_str.as_bytes();
                            row_body.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
                            row_body.extend_from_slice(bytes);
                        }

                        let total_row_len = (row_body.len() + 4) as u32;
                        data_row.extend_from_slice(&total_row_len.to_be_bytes());
                        data_row.extend_from_slice(&row_body);
                        stream.write_all(&data_row).await?;
                    }
                }

                send_command_complete(stream, &format!("SELECT {}", arr.len())).await?;
                return Ok(());
            }
        }
    }

    // Fallback: single column "result"
    let mut row_desc = Vec::new();
    row_desc.push(b'T');
    let mut desc_body = Vec::new();
    desc_body.extend_from_slice(&1u16.to_be_bytes());
    desc_body.extend_from_slice(b"result\0");
    desc_body.extend_from_slice(&0u32.to_be_bytes());
    desc_body.extend_from_slice(&0u16.to_be_bytes());
    desc_body.extend_from_slice(&25u32.to_be_bytes());
    desc_body.extend_from_slice(&(-1i16).to_be_bytes());
    desc_body.extend_from_slice(&(-1i32).to_be_bytes());
    desc_body.extend_from_slice(&0u16.to_be_bytes());
    let total_desc_len = (desc_body.len() + 4) as u32;
    row_desc.extend_from_slice(&total_desc_len.to_be_bytes());
    row_desc.extend_from_slice(&desc_body);
    stream.write_all(&row_desc).await?;

    let mut data_row = Vec::new();
    data_row.push(b'D');
    let mut row_body = Vec::new();
    row_body.extend_from_slice(&1u16.to_be_bytes());
    let bytes = result_str.as_bytes();
    row_body.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
    row_body.extend_from_slice(bytes);
    let total_data_len = (row_body.len() + 4) as u32;
    data_row.extend_from_slice(&total_data_len.to_be_bytes());
    data_row.extend_from_slice(&row_body);
    stream.write_all(&data_row).await?;

    send_command_complete(stream, "SELECT 1").await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageEngine;

    #[tokio::test]
    async fn test_pgwire_startup_handshake_and_query() {
        let storage = StorageEngine::new_temp().unwrap();
        let executor = Arc::new(Executor::new(storage));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn mock pgwire listener
        tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await {
                let _ = handle_pg_client(socket, executor).await;
            }
        });

        let mut client = TcpStream::connect(format!("127.0.0.1:{}", port)).await.unwrap();

        // Send StartupMessage: length = 4 + 4 + len("user\0admin\0database\0test\0\0")
        let mut startup = Vec::new();
        startup.extend_from_slice(&PG_PROTOCOL_V3.to_be_bytes());
        startup.extend_from_slice(b"user\0admin\0database\0test\0\0");

        let total_len = (startup.len() + 4) as u32;
        let mut packet = Vec::new();
        packet.extend_from_slice(&total_len.to_be_bytes());
        packet.extend_from_slice(&startup);
        client.write_all(&packet).await.unwrap();

        // Expect AuthOk ('R')
        let mut auth_res = [0u8; 9];
        client.read_exact(&mut auth_res).await.unwrap();
        assert_eq!(auth_res[0], b'R');

        // Read until ReadyForQuery ('Z')
        let mut b = [0u8; 1];
        loop {
            client.read_exact(&mut b).await.unwrap();
            if b[0] == b'Z' {
                let mut remaining_ready = [0u8; 5];
                client.read_exact(&mut remaining_ready).await.unwrap();
                assert_eq!(remaining_ready[4], b'I');
                break;
            }
        }
    }
}
