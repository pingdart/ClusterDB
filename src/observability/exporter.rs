use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelExporterConfig {
    pub endpoint: String,
    pub service_name: String,
    pub enabled: bool,
}

impl Default for OtelExporterConfig {
    fn default() -> Self {
        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:4318/v1/traces".to_string());
        let enabled = std::env::var("OTEL_EXPORTER").is_ok()
            || std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok();
        let service_name = std::env::var("OTEL_SERVICE_NAME")
            .unwrap_or_else(|_| "pingdart-database".to_string());

        Self {
            endpoint,
            service_name,
            enabled,
        }
    }
}

pub struct OtelExporter {
    client: reqwest::Client,
    config: OtelExporterConfig,
}

impl OtelExporter {
    pub fn new(config: OtelExporterConfig) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap_or_default(),
            config,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Converts internal Spans to the standard OpenTelemetry OTLP JSON payload format
    pub fn format_otlp_json(&self, spans: &[crate::observability::Span]) -> serde_json::Value {
        let otlp_spans: Vec<serde_json::Value> = spans
            .iter()
            .map(|s| {
                let start_nano = s.start_time_epoch_ms * 1_000_000;
                let end_nano = start_nano + (s.duration_ms * 1_000_000.0) as u64;

                let attributes: Vec<serde_json::Value> = s
                    .attributes
                    .iter()
                    .map(|(k, v)| {
                        serde_json::json!({
                            "key": k,
                            "value": { "stringValue": v }
                        })
                    })
                    .collect();

                let mut span_obj = serde_json::json!({
                    "traceId": s.trace_id,
                    "spanId": s.span_id,
                    "name": s.name,
                    "kind": 1, // SPAN_KIND_INTERNAL
                    "startTimeUnixNano": start_nano.to_string(),
                    "endTimeUnixNano": end_nano.to_string(),
                    "attributes": attributes,
                    "status": {
                        "code": if s.status == "OK" { 1 } else { 2 } // 1: STATUS_CODE_OK, 2: STATUS_CODE_ERROR
                    }
                });

                if let Some(ref parent_id) = s.parent_span_id {
                    span_obj["parentSpanId"] = serde_json::json!(parent_id);
                }

                span_obj
            })
            .collect();

        serde_json::json!({
            "resourceSpans": [
                {
                    "resource": {
                        "attributes": [
                            {
                                "key": "service.name",
                                "value": { "stringValue": self.config.service_name }
                            },
                            {
                                "key": "service.version",
                                "value": { "stringValue": "0.5.0" }
                            }
                        ]
                    },
                    "scopeSpans": [
                        {
                            "scope": {
                                "name": "pingdart-core",
                                "version": "0.5.0"
                            },
                            "spans": otlp_spans
                        }
                    ]
                }
            ]
        })
    }

    /// Sends trace spans to configured OpenTelemetry Collector / Jaeger / Grafana Tempo endpoint
    pub async fn export_spans(&self, spans: &[crate::observability::Span]) -> Result<()> {
        if !self.config.enabled || spans.is_empty() {
            return Ok(());
        }

        let body = self.format_otlp_json(spans);
        let resp = self
            .client
            .post(&self.config.endpoint)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                debug!("Exported {} spans to {}", spans.len(), self.config.endpoint);
                Ok(())
            }
            Ok(r) => {
                let status = r.status();
                let txt = r.text().await.unwrap_or_default();
                error!("OTLP export rejected ({}): {}", status, txt);
                Err(anyhow::anyhow!("OTLP export rejected with status {}: {}", status, txt))
            }
            Err(e) => {
                error!("Failed to connect to OTLP collector at {}: {}", self.config.endpoint, e);
                Err(anyhow::anyhow!("Failed to connect to OTLP collector: {}", e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_otlp_json_formatting() {
        let exporter = OtelExporter::new(OtelExporterConfig {
            endpoint: "http://localhost:4318/v1/traces".to_string(),
            service_name: "pingdart-test".to_string(),
            enabled: true,
        });

        let mut attrs = HashMap::new();
        attrs.insert("db.statement".to_string(), "SELECT 1".to_string());

        let span = crate::observability::Span {
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".to_string(),
            span_id: "00f067aa0ba902b7".to_string(),
            parent_span_id: None,
            name: "QueryExecution".to_string(),
            start_time_epoch_ms: 1700000000000,
            duration_ms: 5.2,
            status: "OK".to_string(),
            attributes: attrs,
            events: vec![],
        };

        let json = exporter.format_otlp_json(&[span]);
        assert_eq!(
            json["resourceSpans"][0]["resource"]["attributes"][0]["value"]["stringValue"],
            "pingdart-test"
        );
        let span_out = &json["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(span_out["traceId"], "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(span_out["spanId"], "00f067aa0ba902b7");
        assert_eq!(span_out["status"]["code"], 1);
    }
}
