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
pub mod mysqlwire;
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
    tracing_subscriber::fmt()
        .without_time()
        .with_target(false)
        .compact()
        .init();
    
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "benchmark" {
        crate::benchmark::benchmark::run_benchmark().await;
        return Ok(());
    }

    let mut port: u16 = 4650;
    let mut node_id: Option<String> = None;
    let mut join_seed: Option<String> = None;
    let mut data_dir = std::env::var("PINGDART_DATA_DIR").unwrap_or_else(|_| {
        if std::path::Path::new("Database/pingdart_data").exists() {
            "Database/pingdart_data".to_string()
        } else {
            "pingdart_data".to_string()
        }
    });

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

    println!(
        "\x1b[36m\n  ____  _             ____             _     \n |  _ \\(_)_ __   __ _|  _ \\  __ _ _ __| |_   \n | |_) | | '_ \\ / _` | | | |/ _` | '__| __|  \n |  __/| | | | | (_| | |_| | (_| | |  | |_   \n |_|   |_|_| |_|\\__, |____/ \\__,_|_|   \\__|  PingDart ClusterDB v0.5.0\n                |___/                        \x1b[0m\n  \x1b[1m➜ HTTP API:\x1b[0m    http://0.0.0.0:{}\n  \x1b[1m➜ Web Panel:\x1b[0m   http://localhost:{}/myadmin/\n  \x1b[1m➜ Postgres:\x1b[0m    0.0.0.0:5432\n  \x1b[1m➜ Storage:\x1b[0m     ./{} (Sled WAL Sync)\n",
        port, port, data_dir
    );

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

    // Initialize main admin user if not already present, or preserve active session & permissions
    let root_user = match db.get::<auth::User>("_users", &admin_user)? {
        Some(mut existing) => {
            // User exists in persistent storage: keep active token, tenants, and roles intact!
            if !existing.verify_password(&admin_pass) {
                existing.set_password(&admin_pass);
                let _ = db.set("_users", &existing.username, &existing);
            }
            existing
        }
        None => {
            let fresh = auth::User::new_root_with_name(&admin_user, admin_pass.clone());
            db.set("_users", &fresh.username, &fresh)?;
            fresh
        }
    };
    info!("Main admin user '{}' initialized with configured credentials (active session preserved: {})", root_user.username, root_user.token.is_some());

    // Also synchronize admin alias for backward compatibility without overwriting existing session
    if db.get::<auth::User>("_users", "admin")?.is_none() {
        let admin_alias = auth::User::new_user(
            "admin".to_string(),
            admin_pass,
            "%".to_string(),
            vec![auth::Role::MainAdmin],
        );
        let _ = db.set("_users", &admin_alias.username, &admin_alias);
    }

    // Initialize or sync mounika user with credentials (preserving custom role grants & IP assignments)
    let mounika_pass = "Ravindra12@".to_string();
    match db.get::<auth::User>("_users", "mounika")? {
        Some(mut existing) => {
            let mut changed = false;
            if !existing.verify_password(&mounika_pass) {
                existing.set_password(&mounika_pass);
                changed = true;
            }
            // Ensure mounika does not inherit global root or wildcard permissions
            let initial_len = existing.roles.len();
            existing.roles.retain(|r| match r {
                auth::Role::MainAdmin => false,
                auth::Role::DatabaseAdmin(pattern) if pattern == "*.*" || pattern == "*" => false,
                _ => true,
            });
            if existing.roles.len() != initial_len {
                changed = true;
            }
            if existing.roles.is_empty() {
                existing.roles.push(auth::Role::DatabaseAdmin("meddylist.*".to_string()));
                changed = true;
            }
            if changed {
                let _ = db.set("_users", &existing.username, &existing);
                let _ = db.flush();
            }
            if let Some(t) = &existing.token {
                let _ = db.set("_tokens", t, &existing.username);
            }
        }
        None => {
            let mut fresh = auth::User::new_user(
                "mounika".to_string(),
                mounika_pass,
                "%".to_string(),
                vec![auth::Role::DatabaseAdmin("meddylist.*".to_string())],
            );
            let token = uuid::Uuid::new_v4().to_string();
            fresh.token = Some(token.clone());
            let _ = db.set("_users", &fresh.username, &fresh);
            let _ = db.set("_tokens", &token, &fresh.username);
        }
    };
    info!("User 'mounika' synchronized with persistent permissions");

    // Start HTTP API Gateway with clustering support
    server::start_server(db.clone(), port, node_id, join_seed).await?;

    info!("Flushing storage engine dirty pages and WAL logs to disk...");
    let flushed_bytes = db.flush()?;
    info!("Successfully flushed {} bytes to disk. PingDart Core shut down cleanly.", flushed_bytes);

    Ok(())
}
