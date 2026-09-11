use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct PingDartClient {
    base_url: String,
    token: Option<String>,
    database: Option<String>,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct QueryPayload<'a> {
    query: &'a str,
    token: &'a str,
    database: Option<&'a str>,
}

#[derive(Deserialize)]
struct ApiResponse {
    status: String,
    result: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct LoginResponse {
    status: String,
    token: Option<String>,
    error: Option<String>,
}

impl PingDartClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token: None,
            database: Some("default".to_string()),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn with_database(mut self, db: &str) -> Self {
        self.database = Some(db.to_string());
        self
    }

    pub async fn login(&mut self, username: &str, password: &str) -> Result<String> {
        let url = format!("{}/login", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "username": username,
                "password": password
            }))
            .send()
            .await?;

        let data: LoginResponse = resp.json().await?;
        if data.status == "success" && data.token.is_some() {
            let token = data.token.unwrap();
            self.token = Some(token.clone());
            Ok(token)
        } else {
            bail!("Login failed: {:?}", data.error.unwrap_or_default())
        }
    }

    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    pub async fn query(&self, sql: &str) -> Result<String> {
        let token = self.token.as_deref().unwrap_or("admin_token");
        let url = format!("{}/query", self.base_url);

        let payload = QueryPayload {
            query: sql,
            token,
            database: self.database.as_deref(),
        };

        let resp = self.http.post(&url).json(&payload).send().await?;
        let data: ApiResponse = resp.json().await?;

        if data.status == "success" {
            Ok(data.result.unwrap_or_default())
        } else {
            bail!("Query error: {:?}", data.error.unwrap_or_default())
        }
    }

    pub async fn store_memory(
        &self,
        memory_type: &str,
        content: &str,
        session_id: Option<&str>,
        metadata: Option<serde_json::Value>,
    ) -> Result<()> {
        let token = self.token.as_deref().unwrap_or("admin_token");
        let url = format!("{}/ai/memory/store", self.base_url);

        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "token": token,
                "memory_type": memory_type,
                "content": content,
                "session_id": session_id,
                "metadata": metadata
            }))
            .send()
            .await?;

        let data: ApiResponse = resp.json().await?;
        if data.status == "success" {
            Ok(())
        } else {
            bail!("Memory store failed: {:?}", data.error.unwrap_or_default())
        }
    }

    pub async fn recall_memory(
        &self,
        session_id: Option<&str>,
        memory_type: Option<&str>,
        top_k: usize,
    ) -> Result<String> {
        let token = self.token.as_deref().unwrap_or("admin_token");
        let url = format!("{}/ai/memory/recall", self.base_url);

        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "token": token,
                "session_id": session_id,
                "memory_type": memory_type,
                "top_k": top_k
            }))
            .send()
            .await?;

        let data: ApiResponse = resp.json().await?;
        if data.status == "success" {
            Ok(data.result.unwrap_or_default())
        } else {
            bail!("Memory recall failed: {:?}", data.error.unwrap_or_default())
        }
    }

    pub async fn semantic_search(
        &self,
        table: &str,
        text_column: &str,
        query: &str,
        threshold: f32,
        limit: usize,
    ) -> Result<String> {
        let sql = format!(
            "SELECT * FROM {} WHERE SEMANTIC_MATCH({}, '{}') > {} LIMIT {};",
            table, text_column, query.replace('\'', "''"), threshold, limit
        );
        self.query(&sql).await
    }
}
