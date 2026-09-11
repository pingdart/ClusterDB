pub mod client;
pub mod logger;
pub mod memory;
pub mod tool;

pub use client::PingDartClient;
pub use logger::TaskLogger;
pub use memory::{AgentMemory, MemoryItem};
pub use tool::{Tool, ToolDefinition, ToolRegistry};

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Arc;

    struct CalculatorTool;

    #[async_trait]
    impl Tool for CalculatorTool {
        fn name(&self) -> &str {
            "calculator"
        }

        fn description(&self) -> &str {
            "Performs basic arithmetic"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" },
                    "op": { "type": "string", "enum": ["add", "multiply"] }
                },
                "required": ["a", "b", "op"]
            })
        }

        async fn execute(&self, args: serde_json::Value) -> anyhow::Result<serde_json::Value> {
            let a = args["a"].as_f64().unwrap_or(0.0);
            let b = args["b"].as_f64().unwrap_or(0.0);
            let op = args["op"].as_str().unwrap_or("add");

            let res = match op {
                "add" => a + b,
                "multiply" => a * b,
                _ => anyhow::bail!("Unsupported op"),
            };

            Ok(json!({ "result": res }))
        }
    }

    #[tokio::test]
    async fn test_agent_tool_registry() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(CalculatorTool));

        let defs = registry.get_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "calculator");

        let args = json!({ "a": 12.0, "b": 30.0, "op": "add" });
        let res = registry.execute("calculator", args).await.unwrap();
        assert_eq!(res["result"].as_f64().unwrap(), 42.0);
    }
}
