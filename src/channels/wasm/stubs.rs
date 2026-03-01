use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;

use crate::channels::{
    Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate,
};
use crate::channels::wasm::WasmChannelError;
use crate::error::ChannelError;
use crate::extensions::ExtensionManager;
use crate::pairing::PairingStore;

#[derive(Debug, Clone, Default)]
pub struct WasmChannelRuntimeConfig;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PreparedChannelModule;

#[derive(Debug, Clone, Default)]
pub struct WasmChannelRuntime;

impl WasmChannelRuntime {
    pub fn new(_config: WasmChannelRuntimeConfig) -> Result<Self, WasmChannelError> {
        Ok(Self)
    }
}

#[derive(Debug, Clone)]
pub struct WasmChannel {
    name: String,
}

impl WasmChannel {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn webhook_secret_name(&self) -> String {
        format!("{}_webhook_secret", self.name)
    }

    pub fn webhook_secret_header(&self) -> Option<&str> {
        Some("x-webhook-secret")
    }

    pub async fn update_config(&self, _updates: std::collections::HashMap<String, serde_json::Value>) {}

    pub async fn set_credential(&self, _placeholder: &str, _value: String) {}
}

#[derive(Debug, Clone)]
pub struct SharedWasmChannel {
    #[allow(dead_code)]
    channel: Arc<WasmChannel>,
}

impl SharedWasmChannel {
    pub fn new(channel: Arc<WasmChannel>) -> Self {
        Self { channel }
    }
}

#[async_trait]
impl Channel for SharedWasmChannel {
    fn name(&self) -> &str {
        self.channel.name()
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        Ok(Box::pin(stream::empty()))
    }

    async fn respond(
        &self,
        _msg: &IncomingMessage,
        _response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn send_status(
        &self,
        _status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LoadedChannel {
    pub name: String,
    pub channel: WasmChannel,
}

impl LoadedChannel {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn webhook_secret_name(&self) -> String {
        self.channel.webhook_secret_name()
    }

    pub fn webhook_secret_header(&self) -> Option<&str> {
        self.channel.webhook_secret_header()
    }
}

#[derive(Debug, Default)]
pub struct LoadResults {
    pub loaded: Vec<LoadedChannel>,
    pub errors: Vec<(PathBuf, WasmChannelError)>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredChannel {
    pub wasm_path: PathBuf,
    pub capabilities_path: Option<PathBuf>,
}

pub struct WasmChannelLoader {
    #[allow(dead_code)]
    runtime: Arc<WasmChannelRuntime>,
    #[allow(dead_code)]
    pairing_store: Arc<PairingStore>,
}

impl WasmChannelLoader {
    pub fn new(runtime: Arc<WasmChannelRuntime>, pairing_store: Arc<PairingStore>) -> Self {
        Self {
            runtime,
            pairing_store,
        }
    }

    pub async fn load_from_dir(&self, _dir: &Path) -> Result<LoadResults, WasmChannelError> {
        Ok(LoadResults::default())
    }
}

pub fn default_channels_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".enclagent")
        .join("channels")
}

pub async fn discover_channels(
    dir: &Path,
) -> Result<HashMap<String, DiscoveredChannel>, std::io::Error> {
    let mut channels = HashMap::new();
    if !dir.is_dir() {
        return Ok(channels);
    }

    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let cap_path = path.with_extension("capabilities.json");
        channels.insert(
            name.to_string(),
            DiscoveredChannel {
                wasm_path: path,
                capabilities_path: if cap_path.exists() { Some(cap_path) } else { None },
            },
        );
    }
    Ok(channels)
}

#[derive(Debug, Clone)]
pub struct RegisteredEndpoint {
    pub channel_name: String,
    pub path: String,
    pub methods: Vec<String>,
    pub require_secret: bool,
}

#[derive(Debug, Default)]
pub struct WasmChannelRouter;

impl WasmChannelRouter {
    pub fn new() -> Self {
        Self
    }

    pub async fn register(
        &self,
        _channel: Arc<WasmChannel>,
        _endpoints: Vec<RegisteredEndpoint>,
        _webhook_secret: Option<String>,
        _secret_header: Option<String>,
    ) {
    }
}

pub fn create_wasm_channel_router(
    _router: Arc<WasmChannelRouter>,
    _extension_manager: Option<Arc<ExtensionManager>>,
) -> axum::Router {
    axum::Router::new()
}
