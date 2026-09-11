use crate::client::PingDartClient;
use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

#[derive(Clone)]
pub struct TaskLogger {
    client: PingDartClient,
}

impl TaskLogger {
    pub fn new(client: PingDartClient) -> Self {
        Self { client }
    }

    pub async fn ensure_schema(&self) -> Result<()> {
        let create_tasks = "CREATE TABLE IF NOT EXISTS agent_tasks (
            id TEXT PRIMARY KEY,
            name TEXT,
            goal TEXT,
            status TEXT,
            created_at TEXT,
            completed_at TEXT,
            summary TEXT
        );";
        let _ = self.client.query(create_tasks).await;

        let create_steps = "CREATE TABLE IF NOT EXISTS agent_steps (
            id TEXT PRIMARY KEY,
            task_id TEXT,
            step_number INT,
            thought TEXT,
            action TEXT,
            observation TEXT,
            created_at TEXT
        );";
        let _ = self.client.query(create_steps).await;
        Ok(())
    }

    pub async fn log_task_start(&self, task_id: &str, name: &str, goal: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let sql = format!(
            "INSERT INTO agent_tasks (id, name, goal, status, created_at) VALUES ('{}', '{}', '{}', 'RUNNING', '{}');",
            task_id,
            name.replace('\'', "''"),
            goal.replace('\'', "''"),
            now
        );
        self.client.query(&sql).await.map(|_| ())
    }

    pub async fn log_step(
        &self,
        task_id: &str,
        step_number: u32,
        thought: &str,
        action: &str,
        observation: &str,
    ) -> Result<()> {
        let step_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let sql = format!(
            "INSERT INTO agent_steps (id, task_id, step_number, thought, action, observation, created_at) VALUES ('{}', '{}', {}, '{}', '{}', '{}', '{}');",
            step_id,
            task_id,
            step_number,
            thought.replace('\'', "''"),
            action.replace('\'', "''"),
            observation.replace('\'', "''"),
            now
        );
        self.client.query(&sql).await.map(|_| ())
    }

    pub async fn log_task_complete(&self, task_id: &str, status: &str, summary: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let sql = format!(
            "UPDATE agent_tasks SET status = '{}', completed_at = '{}', summary = '{}' WHERE id = '{}';",
            status,
            now,
            summary.replace('\'', "''"),
            task_id
        );
        self.client.query(&sql).await.map(|_| ())
    }
}
