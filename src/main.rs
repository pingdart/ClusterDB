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
use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
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
            _ => {}
        }
        i += 1;
    }

    info!("PingDart Core Engine Starting on port {} (Node: {:?})...", port, node_id);
    
    // Initialize storage in the configured data directory
    let db = storage::StorageEngine::new(&data_dir)?;
    info!("Storage engine initialized at ./{}", data_dir);

    // Initialize root user with default password (admin / admin123)
    let root_user = auth::User::new_root("admin123".to_string());
    // Only set if not exists, so we don't overwrite on reboot
    if db.get::<auth::User>("_users", "admin")?.is_none() {
        db.set("_users", &root_user.username, &root_user)?;
        info!("Root user initialized with default credentials");
    }

    // Start HTTP API Gateway with clustering support
    server::start_server(db, port, node_id, join_seed).await?;

    Ok(())
}
