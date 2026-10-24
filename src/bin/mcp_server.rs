use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::io::{self, BufRead, Write};

#[derive(Deserialize, Debug)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Serialize, Debug)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize, Debug)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

struct McpClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl McpClient {
    async fn new(base_url: &str) -> Self {
        let http = reqwest::Client::new();
        let token = if let Ok(t) = env::var("PINGDART_TOKEN") {
            t
        } else {
            // Attempt automatic login with root
            let user = env::var("PINGDART_USER").unwrap_or_else(|_| "admin".to_string());
            let pass = env::var("PINGDART_PASS").unwrap_or_else(|_| "admin123".to_string());
            match http.post(format!("{}/login", base_url))
                .json(&json!({ "username": user, "password": pass }))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(body) = resp.json::<Value>().await {
                        body["token"].as_str().unwrap_or("cluster_internal").to_string()
                    } else {
                        "cluster_internal".to_string()
                    }
                }
                Err(_) => "cluster_internal".to_string(),
            }
        };

        Self {
            base_url: base_url.to_string(),
            token,
            http,
        }
    }

    async fn execute_query(&self, query: &str, db: Option<&str>, tenant: Option<&str>) -> Result<String> {
        let url = format!("{}/query", self.base_url);
        let resp = self.http.post(&url)
            .json(&json!({
                "query": query,
                "token": self.token,
                "database": db,
                "tenant_id": tenant,
            }))
            .send()
            .await?;

        let res_json: Value = resp.json().await?;
        if res_json["status"] == "success" {
            Ok(res_json["result"].as_str().unwrap_or("").to_string())
        } else {
            Err(anyhow!("{}", res_json["error"].as_str().unwrap_or("Query execution failed")))
        }
    }

    async fn semantic_search(&self, table: &str, column: &str, query: &str, db: Option<&str>, top_k: usize) -> Result<String> {
        let db_name = db.unwrap_or("default");
        let sql = format!(
            "SELECT * FROM {} WHERE SEMANTIC_MATCH({}, '{}') > 0.0 LIMIT {};",
            table, column, query.replace('\'', "''"), top_k
        );
        self.execute_query(&sql, Some(db_name), None).await
    }

    async fn memory_store(&self, content: &str, memory_type: &str, session_id: Option<&str>, importance: f32) -> Result<String> {
        let url = format!("{}/ai/memory/store", self.base_url);
        let resp = self.http.post(&url)
            .json(&json!({
                "token": self.token,
                "content": content,
                "memory_type": memory_type,
                "session_id": session_id,
                "importance": importance,
            }))
            .send()
            .await?;

        let res_json: Value = resp.json().await?;
        if res_json["status"] == "success" {
            Ok(res_json["result"].as_str().unwrap_or("Memory stored").to_string())
        } else {
            Err(anyhow!("{}", res_json["error"].as_str().unwrap_or("Memory store failed")))
        }
    }

    async fn memory_recall(&self, session_id: Option<&str>, memory_type: Option<&str>, limit: usize) -> Result<String> {
        let url = format!("{}/ai/memory/recall", self.base_url);
        let resp = self.http.post(&url)
            .json(&json!({
                "token": self.token,
                "session_id": session_id,
                "memory_type": memory_type,
                "limit": limit,
            }))
            .send()
            .await?;

        let res_json: Value = resp.json().await?;
        if res_json["status"] == "success" {
            Ok(res_json["result"].as_str().unwrap_or("[]").to_string())
        } else {
            Err(anyhow!("{}", res_json["error"].as_str().unwrap_or("Memory recall failed")))
        }
    }

    async fn graph_neighbors(&self, node_id: &str, edge_type: Option<&str>) -> Result<String> {
        let url = format!("{}/graph/neighbors", self.base_url);
        let resp = self.http.post(&url)
            .json(&json!({
                "token": self.token,
                "node_id": node_id,
                "edge_type": edge_type,
            }))
            .send()
            .await?;

        let res_json: Value = resp.json().await?;
        if res_json["status"] == "success" {
            Ok(res_json["result"].as_str().unwrap_or("[]").to_string())
        } else {
            Err(anyhow!("{}", res_json["error"].as_str().unwrap_or("Graph query failed")))
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let base_url = env::var("PINGDART_URL").unwrap_or_else(|_| "http://127.0.0.1:4650".to_string());
    let client = McpClient::new(&base_url).await;

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                };
                let _ = writeln!(handle, "{}", serde_json::to_string(&err_resp)?);
                let _ = handle.flush();
                continue;
            }
        };

        // Process RPC method
        let response = match req.method.as_str() {
            "initialize" => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "pingdart-mcp",
                        "version": "0.1.0"
                    }
                })),
                error: None,
            },
            "notifications/initialized" => {
                // MCP notifications do not send a response
                continue;
            },
            "tools/list" => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(json!({
                    "tools": [
                        {
                            "name": "sql_query",
                            "description": "Execute any SQL or DDL query on PingDartDB (supports vector search, RLS, multi-tenant isolation)",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string", "description": "The SQL statement" },
                                    "database": { "type": "string", "description": "Optional target database name" },
                                    "tenant_id": { "type": "string", "description": "Optional tenant ID for isolation" }
                                },
                                "required": ["query"]
                            }
                        },
                        {
                            "name": "semantic_search",
                            "description": "Search unstructured data using semantic intent via PingDart's embedded AI models",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "table": { "type": "string", "description": "Target table name" },
                                    "column": { "type": "string", "description": "Content or vector column name" },
                                    "query": { "type": "string", "description": "Natural language query text" },
                                    "database": { "type": "string", "description": "Database name (default: 'default')" },
                                    "top_k": { "type": "number", "description": "Max number of matches to retrieve" }
                                },
                                "required": ["table", "column", "query"]
                            }
                        },
                        {
                            "name": "memory_store",
                            "description": "Store cognitive memory for AI agents (Episodic, Semantic, Working, LongTerm)",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "content": { "type": "string", "description": "Memory text" },
                                    "memory_type": { "type": "string", "description": "Episodic, Semantic, Working, or LongTerm" },
                                    "session_id": { "type": "string", "description": "Agent session identifier" },
                                    "importance": { "type": "number", "description": "Salience score from 0.0 to 1.0" }
                                },
                                "required": ["content", "memory_type"]
                            }
                        },
                        {
                            "name": "memory_recall",
                            "description": "Recall cognitive memories for an AI agent session",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "session_id": { "type": "string", "description": "Agent session ID" },
                                    "memory_type": { "type": "string", "description": "Optional memory type filter" },
                                    "limit": { "type": "number", "description": "Max results to return" }
                                }
                            }
                        },
                        {
                            "name": "graph_neighbors",
                            "description": "Traverse knowledge graph entity neighbors and relationships",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "node_id": { "type": "string", "description": "Node identifier" },
                                    "edge_type": { "type": "string", "description": "Optional edge type filter" }
                                },
                                "required": ["node_id"]
                            }
                        }
                    ]
                })),
                error: None,
            },
            "tools/call" => {
                let params = req.params.unwrap_or(Value::Null);
                let tool_name = params["name"].as_str().unwrap_or("");
                let args = &params["arguments"];

                let call_result = match tool_name {
                    "sql_query" => {
                        let query = args["query"].as_str().unwrap_or("");
                        let db = args["database"].as_str();
                        let tenant = args["tenant_id"].as_str();
                        client.execute_query(query, db, tenant).await
                    },
                    "semantic_search" => {
                        let table = args["table"].as_str().unwrap_or("");
                        let col = args["column"].as_str().unwrap_or("");
                        let query = args["query"].as_str().unwrap_or("");
                        let db = args["database"].as_str();
                        let top_k = args["top_k"].as_u64().unwrap_or(5) as usize;
                        client.semantic_search(table, col, query, db, top_k).await
                    },
                    "memory_store" => {
                        let content = args["content"].as_str().unwrap_or("");
                        let memory_type = args["memory_type"].as_str().unwrap_or("Working");
                        let session_id = args["session_id"].as_str();
                        let importance = args["importance"].as_f64().unwrap_or(0.5) as f32;
                        client.memory_store(content, memory_type, session_id, importance).await
                    },
                    "memory_recall" => {
                        let session_id = args["session_id"].as_str();
                        let memory_type = args["memory_type"].as_str();
                        let limit = args["limit"].as_u64().unwrap_or(10) as usize;
                        client.memory_recall(session_id, memory_type, limit).await
                    },
                    "graph_neighbors" => {
                        let node_id = args["node_id"].as_str().unwrap_or("");
                        let edge_type = args["edge_type"].as_str();
                        client.graph_neighbors(node_id, edge_type).await
                    },
                    _ => Err(anyhow!("Unknown tool: {}", tool_name)),
                };

                match call_result {
                    Ok(text) => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: Some(json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": text
                                }
                            ],
                            "isError": false
                        })),
                        error: None,
                    },
                    Err(e) => JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: Some(json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("Error: {}", e)
                                }
                            ],
                            "isError": true
                        })),
                        error: None,
                    }
                }
            },
            _ => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", req.method),
                    data: None,
                }),
            },
        };

        let out = serde_json::to_string(&response)?;
        let _ = writeln!(handle, "{}", out);
        let _ = handle.flush();
    }

    Ok(())
}
