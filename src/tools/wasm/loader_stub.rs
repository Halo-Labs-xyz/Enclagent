use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::secrets::SecretsStore;
use crate::tools::registry::{ToolRegistry, WasmRegistrationError};
use crate::tools::wasm::{
    WasmError, WasmStorageError, WasmToolRuntime, WasmToolStore,
};

#[derive(Debug, thiserror::Error)]
pub enum WasmLoadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("WASM runtime disabled at compile time: {0}")]
    Compilation(#[from] WasmError),
    #[error("Storage error: {0}")]
    Storage(#[from] WasmStorageError),
    #[error("Registration error: {0}")]
    Registration(#[from] WasmRegistrationError),
    #[error("Invalid tool name: {0}")]
    InvalidName(String),
    #[error("WASM file not found: {0}")]
    WasmNotFound(PathBuf),
}

pub struct WasmToolLoader {
    #[allow(dead_code)]
    runtime: Arc<WasmToolRuntime>,
    #[allow(dead_code)]
    registry: Arc<ToolRegistry>,
    #[allow(dead_code)]
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
}

impl WasmToolLoader {
    pub fn new(runtime: Arc<WasmToolRuntime>, registry: Arc<ToolRegistry>) -> Self {
        Self {
            runtime,
            registry,
            secrets_store: None,
        }
    }

    pub fn with_secrets_store(mut self, store: Arc<dyn SecretsStore + Send + Sync>) -> Self {
        self.secrets_store = Some(store);
        self
    }

    pub async fn load_from_files(
        &self,
        name: &str,
        wasm_path: &Path,
        _capabilities_path: Option<&Path>,
    ) -> Result<(), WasmLoadError> {
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            return Err(WasmLoadError::InvalidName(name.to_string()));
        }
        if !wasm_path.exists() {
            return Err(WasmLoadError::WasmNotFound(wasm_path.to_path_buf()));
        }
        Err(WasmError::ConfigError(
            "WASM runtime disabled at compile time (enable feature \"wasm-runtime\")".to_string(),
        )
        .into())
    }

    pub async fn load_from_dir(&self, dir: &Path) -> Result<LoadResults, WasmLoadError> {
        if !dir.is_dir() {
            return Err(WasmLoadError::Io(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                format!("{} is not a directory", dir.display()),
            )));
        }
        Ok(LoadResults::default())
    }

    pub async fn load_from_storage(
        &self,
        _store: &dyn WasmToolStore,
        _user_id: &str,
        _name: &str,
    ) -> Result<(), WasmLoadError> {
        Err(WasmError::ConfigError(
            "WASM runtime disabled at compile time (enable feature \"wasm-runtime\")".to_string(),
        )
        .into())
    }

    pub async fn load_all_from_storage(
        &self,
        _store: &dyn WasmToolStore,
        _user_id: &str,
    ) -> Result<LoadResults, WasmLoadError> {
        Ok(LoadResults::default())
    }
}

#[derive(Debug, Default)]
pub struct LoadResults {
    pub loaded: Vec<String>,
    pub errors: Vec<(PathBuf, WasmLoadError)>,
}

pub async fn discover_dev_tools() -> Result<HashMap<String, DiscoveredTool>, std::io::Error> {
    Ok(HashMap::new())
}

pub async fn load_dev_tools(
    _loader: &WasmToolLoader,
    _tools_dir: &Path,
) -> Result<LoadResults, WasmLoadError> {
    Ok(LoadResults::default())
}

pub async fn discover_tools(dir: &Path) -> Result<HashMap<String, DiscoveredTool>, std::io::Error> {
    let mut out = HashMap::new();
    if !dir.is_dir() {
        return Ok(out);
    }

    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("wasm") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let cap_path = path.with_extension("capabilities.json");
        out.insert(
            stem.to_string(),
            DiscoveredTool {
                wasm_path: path,
                capabilities_path: if cap_path.exists() { Some(cap_path) } else { None },
            },
        );
    }

    Ok(out)
}

#[derive(Debug, Clone)]
pub struct DiscoveredTool {
    pub wasm_path: PathBuf,
    pub capabilities_path: Option<PathBuf>,
}
