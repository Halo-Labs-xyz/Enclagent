use std::sync::Arc;

use async_trait::async_trait;

use crate::context::JobContext;
use crate::secrets::SecretsStore;
use crate::tools::tool::{Tool, ToolDomain, ToolError, ToolOutput};
use crate::tools::wasm::capabilities::Capabilities;
use crate::tools::wasm::runtime_stub::{PreparedModule, WasmToolRuntime};

#[derive(Debug, Clone)]
pub struct OAuthRefreshConfig {
    pub token_url: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub secret_name: String,
    pub provider: Option<String>,
}

pub struct WasmToolWrapper {
    name: String,
    description: String,
    schema: serde_json::Value,
    _runtime: Arc<WasmToolRuntime>,
    _prepared: Arc<PreparedModule>,
    _capabilities: Capabilities,
}

impl WasmToolWrapper {
    pub fn new(
        runtime: Arc<WasmToolRuntime>,
        prepared: Arc<PreparedModule>,
        capabilities: Capabilities,
    ) -> Self {
        Self {
            name: prepared.name.clone(),
            description: prepared.description.clone(),
            schema: prepared.schema.clone(),
            _runtime: runtime,
            _prepared: prepared,
            _capabilities: capabilities,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.schema = schema;
        self
    }

    pub fn with_secrets_store(self, _store: Arc<dyn SecretsStore + Send + Sync>) -> Self {
        self
    }

    pub fn with_oauth_refresh(self, _oauth_refresh: OAuthRefreshConfig) -> Self {
        self
    }
}

#[async_trait]
impl Tool for WasmToolWrapper {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _context: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError::Sandbox(
            "WASM runtime disabled at compile time (enable feature \"wasm-runtime\")".to_string(),
        ))
    }
}
