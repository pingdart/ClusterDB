mod storage;
mod auth;
mod exec;
mod server;
pub mod index;
pub mod planner;
pub mod benchmark;
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

    info!("PingDart Core Engine Starting...");
    
    // Initialize storage in the local directory (data folder)
    let db = storage::StorageEngine::new("pingdart_data")?;
    info!("Storage engine initialized at ./pingdart_data");

    // Initialize root user with default password (admin / admin123)
    let root_user = auth::User::new_root("admin123".to_string());
    // Only set if not exists, so we don't overwrite on reboot
    if db.get::<auth::User>("_users", "admin")?.is_none() {
        db.set("_users", &root_user.username, &root_user)?;
        info!("Root user initialized with default credentials");
    }

    // Start HTTP API Gateway
    let port = 4650;
    server::start_server(db, port).await?;

    Ok(())
}
