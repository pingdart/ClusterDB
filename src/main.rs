mod storage;
mod auth;
mod exec;
mod server;
pub mod index;
pub mod planner;
pub mod benchmark;
pub mod vector;
pub mod search;
pub mod memory;
pub mod graph;
pub mod cdc;
pub mod embedding;
pub mod cluster;
pub mod tenant;
pub mod columnar;
pub mod object_store;
pub mod backup;
pub mod observability;
pub mod security;
pub mod migration;
pub mod wal;
pub mod mvcc;
pub mod pgwire;
pub mod realtime;
pub mod api;
use anyhow::Result;
use tracing::info;

fn load_env_file() {
    for p in &[".env", "../RustApis/.env"] {
        if let Ok(content) = std::fs::read_to_string(p) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=') {
                    let key = k.trim();
                    let val = v.trim().trim_matches('"').trim_matches('\'');
                    if std::env::var(key).is_err() {
                        unsafe {
                            std::env::set_var(key, val);
                        }
                    }
                }
            }
            break;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    load_env_file();
    tracing_subscriber::fmt::init();
    
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "benchmark" {
        crate::benchmark::benchmark::run_benchmark().await;
        return Ok(());
    }

    let mut port: u16 = 4650;
    let mut node_id: Option<String> = None;
    let mut join_seed: Option<String> = None;
    let mut data_dir = "pingdart_data".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(4650);
                    i += 1;
                }
            }
            "--node-id" | "-n" => {
                if i + 1 < args.len() {
                    node_id = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--join" | "-j" => {
                if i + 1 < args.len() {
                    join_seed = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--data-dir" | "-d" => {
                if i + 1 < args.len() {
                    data_dir = args[i + 1].clone();
                    i += 1;
                }
            }
            "--config" | "-c" => {
                if i + 1 < args.len() {
                    let config_path = &args[i + 1];
                    if let Ok(content) = std::fs::read_to_string(config_path) {
                        if let Ok(val) = content.parse::<toml::Value>() {
                            if let Some(server) = val.get("server") {
                                if let Some(p) = server.get("port").and_then(|v| v.as_integer()) {
                                    port = p as u16;
                                }
                                if let Some(d) = server.get("data_dir").and_then(|v| v.as_str()) {
                                    data_dir = d.to_string();
                                }
                            }
                            if let Some(cluster) = val.get("cluster") {
                                if let Some(n) = cluster.get("node_id").and_then(|v| v.as_str()) {
                                    if !n.is_empty() {
                                        node_id = Some(n.to_string());
                                    }
                                }
                                if let Some(j) = cluster.get("join_seed").and_then(|v| v.as_str()) {
                                    if !j.is_empty() {
                                        join_seed = Some(j.to_string());
                                    }
                                }
                            }
                        }
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    info!("PingDart Core Engine Starting on port {} (Node: {:?})...", port, node_id);
    
    // Initialize storage in the configured data directory
    let db = storage::StorageEngine::new(&data_dir)?;
    info!("Storage engine initialized at ./{}", data_dir);

    // Ensure 'default' database is registered in _databases
    let _ = db.set("_databases", "default", &true);

    // Read admin user & password from environment or use configured defaults
    let admin_user = std::env::var("PINGDART_DB_USER")
        .or_else(|_| std::env::var("ADMIN_USER"))
        .unwrap_or_else(|_| "ravi80181".to_string());
    let admin_pass = std::env::var("PINGDART_DB_PASSWORD")
        .or_else(|_| std::env::var("ADMIN_PASSWORD"))
        .unwrap_or_else(|_| "RavindraChinnu12@".to_string());

    // Initialize main admin user (ravi80181 with configured credentials)
    let root_user = auth::User::new_root_with_name(&admin_user, admin_pass.clone());
    db.set("_users", &root_user.username, &root_user)?;
    info!("Main admin user '{}' initialized with configured credentials", root_user.username);

    // Also synchronize admin alias for backward compatibility
    let admin_alias = auth::User::new_user(
        "admin".to_string(),
        admin_pass,
        "%".to_string(),
        vec![auth::Role::MainAdmin],
    );
    let _ = db.set("_users", &admin_alias.username, &admin_alias);

    // Start HTTP API Gateway with clustering support
    server::start_server(db.clone(), port, node_id, join_seed).await?;

    info!("Flushing storage engine dirty pages and WAL logs to disk...");
    let flushed_bytes = db.flush()?;
    info!("Successfully flushed {} bytes to disk. PingDart Core shut down cleanly.", flushed_bytes);

    Ok(())
}
