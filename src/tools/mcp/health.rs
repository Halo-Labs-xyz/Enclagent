//! MCP connectivity probes used by startup/doctor/status surfaces.

use std::error::Error as _;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::tools::mcp::config::McpServerConfig;

/// Typed MCP health state for operator surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpHealthState {
    Healthy,
    Disabled,
    InvalidUrl,
    DnsFailure,
    ConnectFailure,
    Timeout,
    AuthFailure,
    HttpFailure,
}

impl McpHealthState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Disabled => "disabled",
            Self::InvalidUrl => "invalid_url",
            Self::DnsFailure => "dns_failure",
            Self::ConnectFailure => "connect_failure",
            Self::Timeout => "timeout",
            Self::AuthFailure => "auth_failure",
            Self::HttpFailure => "http_failure",
        }
    }

    pub fn is_healthy(self) -> bool {
        matches!(self, Self::Healthy | Self::Disabled)
    }
}

/// Preflight probe result for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerHealth {
    pub name: String,
    pub url: String,
    pub state: McpHealthState,
    pub detail: String,
    pub http_status: Option<u16>,
}

impl McpServerHealth {
    pub fn is_healthy(&self) -> bool {
        self.state.is_healthy()
    }
}

/// Probe all enabled MCP servers.
pub async fn probe_enabled_servers(
    servers: impl Iterator<Item = McpServerConfig>,
    timeout: Duration,
) -> Vec<McpServerHealth> {
    let mut results = Vec::new();
    for server in servers {
        results.push(probe_server(&server, timeout).await);
    }
    results
}

/// Probe one MCP server and classify failures by URL/DNS/connect/auth.
pub async fn probe_server(config: &McpServerConfig, timeout: Duration) -> McpServerHealth {
    if !config.enabled {
        return McpServerHealth {
            name: config.name.clone(),
            url: config.url.clone(),
            state: McpHealthState::Disabled,
            detail: "server disabled".to_string(),
            http_status: None,
        };
    }

    if reqwest::Url::parse(&config.url).is_err() {
        return McpServerHealth {
            name: config.name.clone(),
            url: config.url.clone(),
            state: McpHealthState::InvalidUrl,
            detail: "URL parse failed".to_string(),
            http_status: None,
        };
    }

    let client = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(client) => client,
        Err(error) => {
            return McpServerHealth {
                name: config.name.clone(),
                url: config.url.clone(),
                state: McpHealthState::ConnectFailure,
                detail: format!("HTTP client init failed: {error}"),
                http_status: None,
            };
        }
    };

    match client
        .get(&config.url)
        .header("Accept", "application/json, text/event-stream")
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            let state = if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                McpHealthState::AuthFailure
            } else if status.is_server_error() {
                McpHealthState::HttpFailure
            } else {
                // 200/204/400/404/405 are all acceptable as "reachable"
                // because different MCP servers expose different root behavior.
                McpHealthState::Healthy
            };
            McpServerHealth {
                name: config.name.clone(),
                url: config.url.clone(),
                state,
                detail: format!("HTTP {}", status.as_u16()),
                http_status: Some(status.as_u16()),
            }
        }
        Err(error) => McpServerHealth {
            name: config.name.clone(),
            url: config.url.clone(),
            state: classify_transport_error(&error),
            detail: error.to_string(),
            http_status: None,
        },
    }
}

fn classify_transport_error(error: &reqwest::Error) -> McpHealthState {
    if error.is_timeout() {
        return McpHealthState::Timeout;
    }

    let mut source = error.source();
    while let Some(err) = source {
        if let Some(io_error) = err.downcast_ref::<std::io::Error>() {
            return match io_error.kind() {
                std::io::ErrorKind::NotFound => McpHealthState::DnsFailure,
                std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::AddrNotAvailable => McpHealthState::ConnectFailure,
                _ => McpHealthState::ConnectFailure,
            };
        }
        source = err.source();
    }

    let lowered = error.to_string().to_ascii_lowercase();
    if lowered.contains("dns")
        || lowered.contains("lookup")
        || lowered.contains("name or service not known")
        || lowered.contains("no such host")
    {
        McpHealthState::DnsFailure
    } else {
        McpHealthState::ConnectFailure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_state_labels_are_stable() {
        assert_eq!(McpHealthState::Healthy.as_str(), "healthy");
        assert_eq!(McpHealthState::DnsFailure.as_str(), "dns_failure");
    }
}
