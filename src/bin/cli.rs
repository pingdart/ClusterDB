use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::exit;

fn print_help() {
    println!("PingDartDB CLI Client");
    println!("Usage: pingdart [OPTIONS] [COMMAND]");
    println!();
    println!("Options:");
    println!("  -h, --host <HOST>        Server hostname or IP (default: 127.0.0.1)");
    println!("  -P, --port <PORT>        Server port (default: 4650)");
    println!("  -u, --user <USER>        Database username (default: ravi80181)");
    println!("  -p, --password <PASS>    Database password");
    println!("  -e, --execute <SQL>      Execute query and exit immediately");
    println!("  -v, --version            Display version information");
    println!("      --help               Display this help guide");
    println!();
    println!("Commands:");
    println!("  login                    Prompt for credentials and save session token");
    println!("  status                   Show database cluster operational status");
}

fn get_session_file() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".pingdart").join("session")
}

fn save_session_token(token: &str, user: &str) {
    let path = get_session_file();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let data = format!("token={}\nuser={}\n", token, user);
    let _ = fs::write(&path, data);
}

fn load_session_token() -> Option<(String, String)> {
    let path = get_session_file();
    if let Ok(content) = fs::read_to_string(path) {
        let mut token = String::new();
        let mut user = String::new();
        for line in content.lines() {
            if let Some(t) = line.strip_prefix("token=") {
                token = t.trim().to_string();
            } else if let Some(u) = line.strip_prefix("user=") {
                user = u.trim().to_string();
            }
        }
        if !token.is_empty() {
            return Some((token, user));
        }
    }
    None
}

/// Formats a JSON Array of records into an ASCII grid table (MySQL style)
fn print_formatted_table(val: &Value) -> bool {
    let rows = match val.as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return false,
    };

    // Extract all unique column headers
    let mut columns = BTreeSet::new();
    for row in rows {
        if let Some(obj) = row.as_object() {
            for k in obj.keys() {
                columns.insert(k.clone());
            }
        }
    }

    if columns.is_empty() {
        return false;
    }

    let cols: Vec<String> = columns.into_iter().collect();
    let mut col_widths: Vec<usize> = cols.iter().map(|c| c.len()).collect();

    // Calculate maximum widths
    for row in rows {
        if let Some(obj) = row.as_object() {
            for (i, c) in cols.iter().enumerate() {
                let cell_len = match obj.get(c) {
                    Some(Value::String(s)) => s.len(),
                    Some(v) => v.to_string().len(),
                    None => 4, // "NULL"
                };
                if cell_len > col_widths[i] {
                    col_widths[i] = cell_len;
                }
            }
        }
    }

    // Border builder
    let mut border = String::from("+");
    for w in &col_widths {
        border.push_str(&"-".repeat(*w + 2));
        border.push('+');
    }

    println!("{}", border);

    // Header row
    print!("|");
    for (i, c) in cols.iter().enumerate() {
        print!(" {:<width$} |", c, width = col_widths[i]);
    }
    println!();
    println!("{}", border);

    // Data rows
    for row in rows {
        if let Some(obj) = row.as_object() {
            print!("|");
            for (i, c) in cols.iter().enumerate() {
                let val_str = match obj.get(c) {
                    Some(Value::String(s)) => s.clone(),
                    Some(v) => v.to_string(),
                    None => "NULL".to_string(),
                };
                print!(" {:<width$} |", val_str, width = col_widths[i]);
            }
            println!();
        }
    }

    println!("{}", border);
    let row_count = rows.len();
    println!("{} {} in set", row_count, if row_count == 1 { "row" } else { "rows" });
    true
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = 4650;
    let mut arg_user: Option<String> = None;
    let mut arg_pass: Option<String> = None;
    let mut execute_sql: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" => {
                print_help();
                return Ok(());
            }
            "-v" | "--version" => {
                println!("PingDartDB CLI v0.5.0");
                return Ok(());
            }
            "-h" | "--host" => {
                if i + 1 < args.len() {
                    host = args[i + 1].clone();
                    i += 1;
                }
            }
            "-P" | "--port" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(4650);
                    i += 1;
                }
            }
            "-u" | "--user" => {
                if i + 1 < args.len() {
                    arg_user = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "-p" | "--password" => {
                if i + 1 < args.len() {
                    arg_pass = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "-e" | "--execute" => {
                if i + 1 < args.len() {
                    execute_sql = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "status" => {
                let client = reqwest::Client::new();
                let res = client.get(format!("http://{}:{}/", host, port)).send().await;
                match res {
                    Ok(resp) => {
                        let text = resp.text().await.unwrap_or_default();
                        println!("{}", text);
                    }
                    Err(e) => {
                        eprintln!("Failed to connect to PingDartDB at {}:{}: {}", host, port, e);
                        exit(1);
                    }
                }
                return Ok(());
            }
            "login" => {
                // Interactive login command
                let mut rl = DefaultEditor::new()?;
                let u = rl.readline("Username [ravi80181]: ")?.trim().to_string();
                let user = if u.is_empty() { "ravi80181".to_string() } else { u };
                let p = rl.readline("Password: ")?.trim().to_string();

                let client = reqwest::Client::new();
                let login_res = client
                    .post(format!("http://{}:{}/login", host, port))
                    .json(&json!({ "username": user, "password": p }))
                    .send()
                    .await?;

                if login_res.status().is_success() {
                    let j: Value = login_res.json().await?;
                    if let Some(token) = j["token"].as_str() {
                        save_session_token(token, &user);
                        println!("Login successful! Session cached in ~/.pingdart/session");
                        return Ok(());
                    }
                }
                eprintln!("Authentication failed.");
                exit(1);
            }
            _ => {}
        }
        i += 1;
    }

    let base_url = format!("http://{}:{}", host, port);
    let client = reqwest::Client::new();

    // Authenticate or restore cached session
    let mut token = String::new();
    #[allow(unused_assignments)]
    let mut username = String::new();

    if let (Some(u), Some(p)) = (arg_user.as_ref(), arg_pass.as_ref()) {
        let login_res = client
            .post(format!("{}/login", base_url))
            .json(&json!({ "username": u, "password": p }))
            .send()
            .await?;
        if login_res.status().is_success() {
            let j: Value = login_res.json().await?;
            token = j["token"].as_str().unwrap_or_default().to_string();
        } else {
            eprintln!("Error: Authentication failed with provided credentials.");
            exit(1);
        }
    } else if let Some((cached_token, cached_user)) = load_session_token() {
        token = cached_token;
        username = cached_user;
        println!("Session restored for user '{}'. Connected to {}.", username, base_url);
    }

    let mut rl = DefaultEditor::new()?;

    // If still no token, prompt interactively
    if token.is_empty() {
        println!("PingDartDB CLI v0.5.0");
        println!("Connected to {}", base_url);
        println!();

        let u = match rl.readline("Username [ravi80181]: ") {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() { "ravi80181".to_string() } else { trimmed.to_string() }
            }
            Err(_) => exit(1),
        };
        username = u;

        let password = match rl.readline("Password: ") {
            Ok(line) => line.trim().to_string(),
            Err(_) => exit(1),
        };

        let login_res = client
            .post(format!("{}/login", base_url))
            .json(&json!({ "username": username, "password": password }))
            .send()
            .await?;

        if !login_res.status().is_success() {
            eprintln!("Login failed! Please check credentials.");
            exit(1);
        }

        let login_json: Value = login_res.json().await?;
        token = login_json["token"].as_str().unwrap_or_default().to_string();
        save_session_token(&token, &username);
        println!("Authentication successful for user '{}'.", username);
        println!();
    }

    // If execute flag was provided, run query and exit immediately
    if let Some(sql) = execute_sql {
        let payload = json!({ "token": token, "query": sql });
        let resp = client.post(format!("{}/query", base_url)).json(&payload).send().await?;
        let json_resp: Value = resp.json().await?;
        if json_resp["status"] == "success" {
            if let Some(result_str) = json_resp["result"].as_str() {
                if let Ok(parsed_arr) = serde_json::from_str::<Value>(result_str) {
                    if !print_formatted_table(&parsed_arr) {
                        println!("{}", result_str);
                    }
                } else {
                    println!("{}", result_str);
                }
            } else {
                println!("OK");
            }
        } else {
            eprintln!("Error: {}", json_resp["error"].as_str().unwrap_or("Query failed"));
            exit(1);
        }
        return Ok(());
    }

    println!("Type SQL queries followed by Enter.");
    println!("Commands: USE <db>, SHOW DATABASES, SHOW TABLES, EXIT");
    println!();

    let mut current_db: Option<String> = None;

    loop {
        let prompt = match &current_db {
            Some(db) => format!("pingdart [{}]> ", db),
            None => "pingdart> ".to_string(),
        };

        match rl.readline(&prompt) {
            Ok(line) => {
                let query = line.trim();
                if query.is_empty() {
                    continue;
                }
                if query.eq_ignore_ascii_case("exit") || query.eq_ignore_ascii_case("quit") {
                    println!("Bye");
                    break;
                }

                let _ = rl.add_history_entry(query);

                // Local intercept for USE <database>
                let q_lower = query.to_lowercase();
                if q_lower.starts_with("use ") {
                    let parts: Vec<&str> = query.split_whitespace().collect();
                    if parts.len() == 2 {
                        let db_name = parts[1].trim_matches(';');
                        current_db = Some(db_name.to_string());
                        println!("Database changed to '{}'", db_name);
                        continue;
                    }
                }

                // Send query
                let mut payload = json!({
                    "token": token,
                    "query": query
                });
                if let Some(db) = &current_db {
                    payload["database"] = json!(db);
                }

                let query_res = client.post(format!("{}/query", base_url)).json(&payload).send().await;

                match query_res {
                    Ok(resp) => {
                        if let Ok(json_resp) = resp.json::<Value>().await {
                            if json_resp["status"] == "success" {
                                if let Some(res_str) = json_resp["result"].as_str() {
                                    if let Ok(parsed_arr) = serde_json::from_str::<Value>(res_str) {
                                        if !print_formatted_table(&parsed_arr) {
                                            println!("{}", res_str);
                                        }
                                    } else {
                                        println!("{}", res_str);
                                    }
                                } else {
                                    println!("Query executed successfully.");
                                }
                            } else {
                                eprintln!("Error: {}", json_resp["error"].as_str().unwrap_or("Query execution error"));
                            }
                        } else {
                            eprintln!("Failed to parse server response.");
                        }
                    }
                    Err(e) => eprintln!("Connection error: {}", e),
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}
