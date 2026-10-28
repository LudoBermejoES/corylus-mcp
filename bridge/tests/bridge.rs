use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use corylus_mcp::endpoint::{EndpointInfo, new_token, write_endpoint};
use corylus_mcp::server::{AgentBackend, BoxFuture, Client, Notifier, ServeOptions, serve};
use corylus_mcp::{AgentError, PROTOCOL_VERSION};
use corylus_mcp_bridge::Bridge;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, ProtocolVersion, ServerNotification,
    SubscriptionFilter,
};
use rmcp::service::RunningService;
use rmcp::{ClientServiceExt, RoleClient, ServiceExt};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<(String, String, Value)>>,
    clients: Mutex<Vec<String>>,
}

impl AgentBackend for Fake {
    fn call(&self, client: Client, tool: String, args: Value) -> BoxFuture<'_, Result<Value, AgentError>> {
        self.calls.lock().unwrap().push((client.name.clone(), tool.clone(), args.clone()));
        Box::pin(async move { Ok(json!({ "tool": tool, "echo": args })) })
    }

    fn clients_changed(&self, clients: Vec<Client>) {
        *self.clients.lock().unwrap() = clients.into_iter().map(|c| c.name).collect();
    }
}

struct Corylus {
    backend: Arc<Fake>,
    notifier: Notifier,
    stop: CancellationToken,
}

impl Corylus {
    /// Start a fake Corylus and publish its endpoint file in `app_data`.
    async fn start(app_data: &Path) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = new_token();
        write_endpoint(app_data, &EndpointInfo { port, token: token.clone(), protocol: PROTOCOL_VERSION }).unwrap();
        let backend = Arc::new(Fake::default());
        let notifier = Notifier::new();
        let stop = CancellationToken::new();
        tokio::spawn(serve(listener, backend.clone(), token, notifier.clone(), ServeOptions::default(), stop.clone()));
        Self { backend, notifier, stop }
    }
}

impl Drop for Corylus {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// Serve the bridge on one end of an in-memory pipe and connect a client to
/// the other, as an MCP client launching the bridge over stdio would.
async fn client_for(bridge: Bridge, name: &str) -> RunningService<RoleClient, ClientConfig> {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Ok(running) = bridge.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    ClientConfig::new(ClientCapabilities::default(), Implementation::new(name, "1.0"))
        .serve(client_io)
        .await
        .expect("client connects to the bridge")
}

fn text_of(result: &rmcp::model::CallToolResult) -> String {
    result.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect()
}

async fn eventually(mut cond: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn tools_are_listed_and_calls_reach_corylus_with_the_client_name() {
    let dir = tempfile::tempdir().unwrap();
    let corylus = Corylus::start(dir.path()).await;
    let client = client_for(Bridge::new(dir.path().into()), "claude-code").await;

    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), corylus_mcp::catalog::catalog().len());
    assert!(tools.iter().any(|t| t.name == "read_document"));

    let args = json!({"item": "Manuscript/Ch1.md"}).as_object().unwrap().clone();
    let r = client.call_tool(CallToolRequestParams::new("read_document").with_arguments(args)).await.unwrap();
    assert_ne!(r.is_error, Some(true), "{}", text_of(&r));
    assert_eq!(r.structured_content.unwrap()["echo"]["item"], "Manuscript/Ch1.md");

    let calls = corylus.backend.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "claude-code");
    assert_eq!(calls[0].1, "read_document");
}

#[tokio::test]
async fn with_corylus_closed_tools_list_and_every_call_says_not_running() {
    let dir = tempfile::tempdir().unwrap();
    let client = client_for(Bridge::new(dir.path().into()), "claude-ai").await;

    assert!(!client.list_all_tools().await.unwrap().is_empty());
    let r = client.call_tool(CallToolRequestParams::new("list_binder")).await.unwrap();
    assert_eq!(r.is_error, Some(true));
    assert!(text_of(&r).contains("not_running"), "{}", text_of(&r));
}

#[tokio::test]
async fn a_stale_endpoint_file_with_nothing_listening_is_not_running() {
    let dir = tempfile::tempdir().unwrap();
    // A port nobody listens on: bind, note it, release it.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    write_endpoint(dir.path(), &EndpointInfo { port, token: new_token(), protocol: PROTOCOL_VERSION }).unwrap();
    let client = client_for(Bridge::new(dir.path().into()), "x").await;
    let r = client.call_tool(CallToolRequestParams::new("list_binder")).await.unwrap();
    assert!(text_of(&r).contains("not_running"));
}

#[tokio::test]
async fn an_unknown_tool_never_reaches_corylus() {
    let dir = tempfile::tempdir().unwrap();
    let corylus = Corylus::start(dir.path()).await;
    let client = client_for(Bridge::new(dir.path().into()), "x").await;
    let r = client.call_tool(CallToolRequestParams::new("delete_everything")).await.unwrap();
    assert_eq!(r.is_error, Some(true));
    assert!(text_of(&r).contains("invalid_input"));
    assert!(corylus.backend.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_client_shows_as_connected_and_leaves_on_goodbye() {
    let dir = tempfile::tempdir().unwrap();
    let corylus = Corylus::start(dir.path()).await;
    let bridge = Bridge::new(dir.path().into());
    let _client = client_for(bridge.clone(), "claude-ai").await;
    let backend = corylus.backend.clone();
    assert!(eventually(|| backend.clients.lock().unwrap().as_slice() == ["claude-ai"]).await, "client never connected");
    bridge.goodbye().await;
    assert!(eventually(|| backend.clients.lock().unwrap().is_empty()).await);
}

#[tokio::test]
async fn a_restarted_corylus_with_a_new_token_is_reached_again() {
    let dir = tempfile::tempdir().unwrap();
    let first = Corylus::start(dir.path()).await;
    let client = client_for(Bridge::new(dir.path().into()), "x").await;
    let ok = client.call_tool(CallToolRequestParams::new("corylus_status")).await.unwrap();
    assert_ne!(ok.is_error, Some(true));
    drop(first);
    tokio::time::sleep(Duration::from_millis(200)).await;

    let second = Corylus::start(dir.path()).await;
    let mut reached = false;
    for _ in 0..3 {
        let r = client.call_tool(CallToolRequestParams::new("corylus_status")).await.unwrap();
        if r.is_error != Some(true) {
            reached = true;
            break;
        }
    }
    assert!(reached, "the bridge did not reconnect to the new Corylus");
    assert!(!second.backend.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_change_in_corylus_reaches_a_subscribed_client() {
    let dir = tempfile::tempdir().unwrap();
    let corylus = Corylus::start(dir.path()).await;
    let bridge = Bridge::new(dir.path().into());
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Ok(running) = bridge.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let client = ().serve_with_lifecycle(
        client_io,
        rmcp::ClientLifecycleMode::Discover { preferred_versions: vec![ProtocolVersion::V_2026_07_28] },
    )
    .await
    .unwrap();
    let mut sub = client
        .listen(SubscriptionFilter::builder().resource_subscriptions(["corylus://document/Ch1.md".to_string()]).build())
        .await
        .unwrap();

    // Wait until the bridge's session exists in Corylus, then change something.
    let notifier = corylus.notifier.clone();
    assert!(eventually(|| !notifier.clients().is_empty()).await);
    corylus.notifier.notify("corylus://document/Ch1.md");

    let n = tokio::time::timeout(Duration::from_secs(10), sub.next()).await.unwrap().unwrap().unwrap();
    match n {
        ServerNotification::ResourceUpdatedNotification(u) => assert_eq!(u.params.uri, "corylus://document/Ch1.md"),
        other => panic!("unexpected notification {other:?}"),
    }
}
