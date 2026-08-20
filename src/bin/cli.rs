use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde_json::json;
use std::process::exit;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== PingDartDB Command Line Interface ===");
    
    let mut rl = DefaultEditor::new()?;
    let client = reqwest::Client::new();
    let base_url = "http://localhost:4650";

    // 1. Login Phase
    let username = match rl.readline("Username: ") {
        Ok(line) => line.trim().to_string(),
        Err(_) => exit(1),
    };
    
    let password = match rl.readline("Password: ") {
        Ok(line) => line.trim().to_string(),
        Err(_) => exit(1),
    };

    println!("Logging in...");
    let login_res = client.post(format!("{}/login", base_url))
        .json(&json!({
            "username": username,
            "password": password
        }))
        .send()
        .await?;

    if !login_res.status().is_success() {
        println!("Login failed! Check credentials or IP whitelist.");
        exit(1);
    }

    let login_json: serde_json::Value = login_res.json().await?;
    let token = login_json["token"].as_str().unwrap_or("").to_string();
    
    if token.is_empty() {
        println!("Error: Server did not return a token.");
        exit(1);
    }

    println!("Login successful! Connected to PingDartDB.");
    println!("Type 'USE <database>' to select a database.");
    println!("Type 'exit' or 'quit' to close.");

    let mut current_db: Option<String> = None;

    // 2. REPL Phase
    loop {
        let prompt = match &current_db {
            Some(db) => format!("pingdart [{}]> ", db),
            None => "pingdart> ".to_string(),
        };
        
        let readline = rl.readline(&prompt);
        match readline {
            Ok(line) => {
                let query = line.trim();
                if query.is_empty() {
                    continue;
                }
                if query.eq_ignore_ascii_case("exit") || query.eq_ignore_ascii_case("quit") {
                    break;
                }
                
                // Intercept USE command locally
                let q_lower = query.to_lowercase();
                if q_lower.starts_with("use ") {
                    let parts: Vec<&str> = query.split_whitespace().collect();
                    if parts.len() == 2 {
                        current_db = Some(parts[1].to_string());
                        println!("Database changed to '{}'", parts[1]);
                        let _ = rl.add_history_entry(query);
                        continue;
                    }
                }
                
                let _ = rl.add_history_entry(query);

                // Send query
                let mut payload = json!({
                    "token": token,
                    "query": query
                });
                if let Some(db) = &current_db {
                    payload["database"] = json!(db);
                }

                let query_res = client.post(format!("{}/query", base_url))
                    .json(&payload)
                    .send()
                    .await;

                match query_res {
                    Ok(resp) => {
                        if let Ok(json_resp) = resp.json::<serde_json::Value>().await {
                            if json_resp["status"] == "success" {
                                println!("{}", json_resp["result"].as_str().unwrap_or("Success"));
                            } else {
                                println!("Error: {}", json_resp["error"].as_str().unwrap_or("Unknown error"));
                            }
                        } else {
                            println!("Failed to parse server response.");
                        }
                    },
                    Err(e) => println!("Connection error: {}", e),
                }
            },
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            },
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            },
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}
