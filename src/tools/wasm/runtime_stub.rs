use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use crate::tools::wasm::error::WasmError;
use crate::tools::wasm::limits::{FuelConfig, ResourceLimits};

#[allow(dead_code)]
pub const EPOCH_TICK_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy)]
pub enum OptLevel {
    None,
    Speed,
    SpeedAndSize,
}

#[derive(Debug, Clone)]
pub struct WasmRuntimeConfig {
    pub default_limits: ResourceLimits,
    pub fuel_config: FuelConfig,
    pub cache_compiled: bool,
    pub cache_dir: Option<PathBuf>,
    pub optimization_level: OptLevel,
}

impl Default for WasmRuntimeConfig {
    fn default() -> Self {
        Self {
            default_limits: ResourceLimits::default(),
            fuel_config: FuelConfig::default(),
            cache_compiled: false,
            cache_dir: None,
            optimization_level: OptLevel::Speed,
        }
    }
}

impl WasmRuntimeConfig {
    pub fn for_testing() -> Self {
        Self {
            default_limits: ResourceLimits::default()
                .with_memory(1024 * 1024)
                .with_fuel(100_000)
                .with_timeout(Duration::from_secs(5)),
            fuel_config: FuelConfig::with_limit(100_000),
            cache_compiled: false,
            cache_dir: None,
            optimization_level: OptLevel::None,
        }
    }
}

#[derive(Debug)]
pub struct PreparedModule {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
    component_bytes: Vec<u8>,
    pub limits: ResourceLimits,
}

impl PreparedModule {
    pub fn component_bytes(&self) -> &[u8] {
        &self.component_bytes
    }
}

pub struct WasmToolRuntime {
    config: WasmRuntimeConfig,
    modules: RwLock<HashMap<String, Arc<PreparedModule>>>,
}

impl WasmToolRuntime {
    pub fn new(config: WasmRuntimeConfig) -> Result<Self, WasmError> {
        Ok(Self {
            config,
            modules: RwLock::new(HashMap::new()),
        })
    }

    pub fn config(&self) -> &WasmRuntimeConfig {
        &self.config
    }

    pub async fn prepare(
        &self,
        name: &str,
        _wasm_bytes: &[u8],
        _limits: Option<ResourceLimits>,
    ) -> Result<Arc<PreparedModule>, WasmError> {
        if let Some(module) = self.modules.read().await.get(name) {
            return Ok(Arc::clone(module));
        }
        Err(WasmError::ConfigError(
            "WASM runtime disabled at compile time (enable feature \"wasm-runtime\")".to_string(),
        ))
    }

    pub async fn clear_cache(&self) {
        self.modules.write().await.clear();
    }
}
