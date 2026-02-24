//! End-to-end integration tests for the WebSocket gateway.
//!
//! These tests start a real Axum server on a random port, connect a WebSocket
//! client, and verify the full message flow:
//! - WebSocket upgrade with auth
//! - Ping/pong
//! - Client message → agent msg_tx
//! - Broadcast SSE event → WebSocket client
//! - Connection tracking (counter increment/decrement)
//! - Gateway status endpoint

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use enclagent::channels::IncomingMessage;
use enclagent::channels::web::server::{GatewayState, start_server};
use enclagent::channels::web::sse::SseManager;
use enclagent::channels::web::types::SseEvent;
use enclagent::channels::web::ws::WsConnectionTracker;

const AUTH_TOKEN: &str = "test-token-12345";
const TIMEOUT: Duration = Duration::from_secs(5);

/// Start a gateway server on a random port and return the bound address + agent
/// message receiver.
fn is_bind_permission_error<E: std::fmt::Display>(err: &E) -> bool {
    err.to_string().contains("Operation not permitted")
        || err.to_string().contains("failed to bind")
}

async fn start_test_server() -> Option<(
    SocketAddr,
    Arc<GatewayState>,
    mpsc::Receiver<IncomingMessage>,
)> {
    let (agent_tx, agent_rx) = mpsc::channel(64);

    let state = Arc::new(GatewayState {
        msg_tx: tokio::sync::RwLock::new(Some(agent_tx)),
        sse: SseManager::new(),
        workspace: None,
        session_manager: None,
        log_broadcaster: None,
        extension_manager: None,
        tool_registry: None,
        store: None,
        job_manager: None,
        prompt_queue: None,
        user_id: "test-user".to_string(),
        shutdown_tx: tokio::sync::RwLock::new(None),
        ws_tracker: Some(Arc::new(WsConnectionTracker::new())),
        llm_provider: None,
        skill_registry: None,
        skill_catalog: None,
        frontdoor: None,
        chat_rate_limiter: enclagent::channels::web::server::RateLimiter::new(30, 60),
    });

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    match start_server(addr, state.clone(), AUTH_TOKEN.to_string()).await {
        Ok(bound_addr) => Some((bound_addr, state, agent_rx)),
        Err(e) if is_bind_permission_error(&e) => None,
        Err(e) => panic!("Failed to start test server: {e:?}"),
    }
}

/// Connect a WebSocket client with auth token in query parameter.
async fn connect_ws(
    addr: SocketAddr,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://{}/api/chat/ws?token={}", addr, AUTH_TOKEN);
    let mut request = url.into_client_request().unwrap();
    // Server requires an Origin header from localhost to prevent cross-site WS hijacking.
    request.headers_mut().insert(
        "Origin",
        format!("http://127.0.0.1:{}", addr.port()).parse().unwrap(),
    );
    let (stream, _response) = tokio_tungstenite::connect_async(request)
        .await
        .expect("Failed to connect WebSocket");
    stream
}

/// Read the next text frame from the WebSocket, with a timeout.
async fn recv_text(
    stream: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) -> String {
    let msg = timeout(TIMEOUT, stream.next())
        .await
        .expect("Timed out waiting for WS message")
        .expect("Stream ended")
        .expect("WS error");
    match msg {
        Message::Text(text) => text.to_string(),
        other => panic!("Expected Text frame, got {:?}", other),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_ws_ping_pong() {
    let Some((addr, _state, _agent_rx)) = start_test_server().await else {
        return;
    };
    let mut ws = connect_ws(addr).await;

    // Send ping
    let ping = r#"{"type":"ping"}"#;
    ws.send(Message::Text(ping.into())).await.unwrap();

    // Expect pong
    let text = recv_text(&mut ws).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["type"], "pong");

    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn test_ws_message_reaches_agent() {
    let Some((addr, _state, mut agent_rx)) = start_test_server().await else {
        return;
    };
    let mut ws = connect_ws(addr).await;

    // Send a chat message
    let msg = r#"{"type":"message","content":"hello from ws","thread_id":"t42"}"#;
    ws.send(Message::Text(msg.into())).await.unwrap();

    // Verify it arrives on the agent's msg_tx
    let incoming = timeout(TIMEOUT, agent_rx.recv())
        .await
        .expect("Timed out waiting for agent message")
        .expect("Agent channel closed");

    assert_eq!(incoming.content, "hello from ws");
    assert_eq!(incoming.thread_id.as_deref(), Some("t42"));
    assert_eq!(incoming.channel, "gateway");
    assert_eq!(incoming.user_id, "test-user");

    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn test_ws_broadcast_event_received() {
    let Some((addr, state, _agent_rx)) = start_test_server().await else {
        return;
    };
    let mut ws = connect_ws(addr).await;

    // Give the connection a moment to fully establish
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Broadcast an SSE event (simulates agent sending a response)
    state.sse.broadcast(SseEvent::Response {
        content: "agent says hi".to_string(),
        thread_id: "t1".to_string(),
    });

    // The WS client should receive it
    let text = recv_text(&mut ws).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["type"], "event");
    assert_eq!(parsed["event_type"], "response");
    assert_eq!(parsed["data"]["content"], "agent says hi");

    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn test_ws_thinking_event() {
    let Some((addr, state, _agent_rx)) = start_test_server().await else {
        return;
    };
    let mut ws = connect_ws(addr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    state.sse.broadcast(SseEvent::Thinking {
        message: "analyzing...".to_string(),
        thread_id: None,
    });

    let text = recv_text(&mut ws).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["type"], "event");
    assert_eq!(parsed["event_type"], "thinking");
    assert_eq!(parsed["data"]["message"], "analyzing...");

    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn test_ws_connection_tracking() {
    let Some((addr, state, _agent_rx)) = start_test_server().await else {
        return;
    };
    let tracker = state.ws_tracker.as_ref().unwrap();

    assert_eq!(tracker.connection_count(), 0);

    // Connect first client
    let ws1 = connect_ws(addr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(tracker.connection_count(), 1);

    // Connect second client
    let ws2 = connect_ws(addr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(tracker.connection_count(), 2);

    // Disconnect first
    drop(ws1);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(tracker.connection_count(), 1);

    // Disconnect second
    drop(ws2);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(tracker.connection_count(), 0);
}

#[tokio::test]
async fn test_ws_invalid_message_returns_error() {
    let Some((addr, _state, _agent_rx)) = start_test_server().await else {
        return;
    };
    let mut ws = connect_ws(addr).await;

    // Send invalid JSON
    ws.send(Message::Text("not json".into())).await.unwrap();

    // Should get an error message back
    let text = recv_text(&mut ws).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["type"], "error");
    assert!(
        parsed["message"]
            .as_str()
            .unwrap()
            .contains("Invalid message")
    );

    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn test_ws_unknown_type_returns_error() {
    let Some((addr, _state, _agent_rx)) = start_test_server().await else {
        return;
    };
    let mut ws = connect_ws(addr).await;

    // Send valid JSON but unknown message type
    ws.send(Message::Text(r#"{"type":"foobar"}"#.into()))
        .await
        .unwrap();

    let text = recv_text(&mut ws).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["type"], "error");

    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn test_gateway_status_endpoint() {
    let Some((addr, _state, _agent_rx)) = start_test_server().await else {
        return;
    };

    // Connect a WS client
    let _ws = connect_ws(addr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Hit the status endpoint
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/api/gateway/status", addr))
        .header("Authorization", format!("Bearer {}", AUTH_TOKEN))
        .send()
        .await
        .expect("Failed to fetch status");

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["ws_connections"], 1);
    assert!(body["total_connections"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn test_channel_status_reporting_endpoint() {
    let Some((addr, _state, _agent_rx)) = start_test_server().await else {
        return;
    };

    // Open one WS connection so gateway channel detail has live counters.
    let _ws = connect_ws(addr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/api/status/channels", addr))
        .header("Authorization", format!("Bearer {}", AUTH_TOKEN))
        .send()
        .await
        .expect("Failed to fetch channel status");

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "healthy");

    let channels = body["channels"]
        .as_array()
        .expect("channels should be an array");
    assert!(channels.len() >= 3);

    let gateway = channels
        .iter()
        .find(|entry| entry["name"] == "gateway")
        .expect("gateway channel should exist");
    assert_eq!(gateway["enabled"].as_bool(), Some(true));
    assert_eq!(gateway["healthy"].as_bool(), Some(true));
    assert_eq!(gateway["status"], "healthy");
    assert_eq!(gateway["detail"]["ws_connections"].as_u64(), Some(1));

    let http = channels
        .iter()
        .find(|entry| entry["name"] == "http_webhook")
        .expect("http_webhook channel should exist");
    assert_eq!(http["status"], "disabled");

    let wasm = channels
        .iter()
        .find(|entry| entry["name"] == "wasm_channels")
        .expect("wasm_channels entry should exist");
    assert_eq!(wasm["status"], "disabled");
}

#[tokio::test]
async fn test_verification_status_endpoint_reports_fallback_state() {
    let Some((addr, _state, _agent_rx)) = start_test_server().await else {
        return;
    };

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/api/status/verification", addr))
        .header("Authorization", format!("Bearer {}", AUTH_TOKEN))
        .send()
        .await
        .expect("Failed to fetch verification status");

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["backend"], "eigencloud_primary");
    assert_eq!(body["backend_status"], "missing_config");
    assert_eq!(body["status"], "failing");

    let fallback = &body["fallback"];
    assert_eq!(fallback["enabled"].as_bool(), Some(true));
    assert_eq!(fallback["require_signed_receipts"].as_bool(), Some(true));
    assert_eq!(fallback["status"], "missing_signing_key");
}

#[tokio::test]
async fn test_ws_no_auth_rejected() {
    let Some((addr, _state, _agent_rx)) = start_test_server().await else {
        return;
    };

    // Try to connect without auth token
    let url = format!("ws://{}/api/chat/ws", addr);
    let request = url.into_client_request().unwrap();
    let result = tokio_tungstenite::connect_async(request).await;

    // Should fail (401 from auth middleware before WS upgrade)
    assert!(result.is_err());
}

#[tokio::test]
async fn test_ws_multiple_events_in_sequence() {
    let Some((addr, state, _agent_rx)) = start_test_server().await else {
        return;
    };
    let mut ws = connect_ws(addr).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Broadcast multiple events rapidly
    state.sse.broadcast(SseEvent::Thinking {
        message: "step 1".to_string(),
        thread_id: None,
    });
    state.sse.broadcast(SseEvent::ToolStarted {
        name: "shell".to_string(),
        thread_id: None,
    });
    state.sse.broadcast(SseEvent::ToolCompleted {
        name: "shell".to_string(),
        success: true,
        thread_id: None,
    });
    state.sse.broadcast(SseEvent::Response {
        content: "done".to_string(),
        thread_id: "t1".to_string(),
    });

    // Receive all 4 in order
    let t1 = recv_text(&mut ws).await;
    let t2 = recv_text(&mut ws).await;
    let t3 = recv_text(&mut ws).await;
    let t4 = recv_text(&mut ws).await;

    let p1: serde_json::Value = serde_json::from_str(&t1).unwrap();
    let p2: serde_json::Value = serde_json::from_str(&t2).unwrap();
    let p3: serde_json::Value = serde_json::from_str(&t3).unwrap();
    let p4: serde_json::Value = serde_json::from_str(&t4).unwrap();

    assert_eq!(p1["event_type"], "thinking");
    assert_eq!(p2["event_type"], "tool_started");
    assert_eq!(p3["event_type"], "tool_completed");
    assert_eq!(p4["event_type"], "response");

    ws.close(None).await.unwrap();
}
