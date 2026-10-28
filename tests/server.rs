use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use corylus_mcp::server::{AgentBackend, BoxFuture, Client, Notifier, ServeOptions, serve};
use corylus_mcp::wire::{CallResponse, EventsResponse, HelloResponse, PROTOCOL_HEADER, Refusal};
use corylus_mcp::{AgentError, ErrorKind, PROTOCOL_VERSION};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<(Client, String, Value)>>,
    clients: Mutex<Vec<Vec<Client>>>,
}

impl AgentBackend for Fake {
    fn call(&self, client: Client, tool: String, args: Value) -> BoxFuture<'_, Result<Value, AgentError>> {
        self.calls.lock().unwrap().push((client.clone(), tool.clone(), args.clone()));
        Box::pin(async move {
            match tool.as_str() {
                "fail" => Err(AgentError::new(ErrorKind::PermissionRequired, "needs allow_edits")
                    .with_data(json!({"required": "allow_edits"}))),
                _ => Ok(json!({"tool": tool, "args": args, "client": client.name})),
            }
        })
    }

    fn clients_changed(&self, clients: Vec<Client>) {
        self.clients.lock().unwrap().push(clients);
    }
}

struct Running {
    base: String,
    token: String,
    backend: Arc<Fake>,
    notifier: Notifier,
    stop: CancellationToken,
    http: reqwest::Client,
}

impl Running {
    async fn start(options: ServeOptions) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let token = corylus_mcp::endpoint::new_token();
        let backend = Arc::new(Fake::default());
        let notifier = Notifier::new();
        let stop = CancellationToken::new();
        tokio::spawn(serve(listener, backend.clone(), token.clone(), notifier.clone(), options, stop.clone()));
        Self { base, token, backend, notifier, stop, http: reqwest::Client::new() }
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
    }

    async fn hello(&self, client: &str) -> String {
        let r: HelloResponse = self.post("/v1/hello").json(&json!({"client": client})).send().await.unwrap().json().await.unwrap();
        r.session
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[tokio::test]
async fn missing_or_wrong_token_is_refused_and_nothing_runs() {
    let s = Running::start(ServeOptions::default()).await;
    let no_token = s.http.post(format!("{}/v1/hello", s.base)).header(PROTOCOL_HEADER, "1").json(&json!({"client": "x"})).send().await.unwrap();
    assert_eq!(no_token.status(), 401);
    let wrong = s.http.post(format!("{}/v1/hello", s.base)).bearer_auth("nope").header(PROTOCOL_HEADER, "1").json(&json!({"client": "x"})).send().await.unwrap();
    assert_eq!(wrong.status(), 401);
    assert!(s.notifier.clients().is_empty());
    assert!(s.backend.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_web_origin_is_refused_even_with_the_token() {
    let s = Running::start(ServeOptions::default()).await;
    let r = s.post("/v1/hello").header("Origin", "https://evil.example").json(&json!({"client": "x"})).send().await.unwrap();
    assert_eq!(r.status(), 403);
    assert!(s.notifier.clients().is_empty());
}

#[tokio::test]
async fn an_incompatible_bridge_is_told_to_update() {
    let s = Running::start(ServeOptions::default()).await;
    let r = s.http.post(format!("{}/v1/hello", s.base)).bearer_auth(&s.token).header(PROTOCOL_HEADER, "999").json(&json!({"client": "x"})).send().await.unwrap();
    assert_eq!(r.status(), 426);
    let body: Refusal = r.json().await.unwrap();
    assert_eq!(body.error.kind, ErrorKind::ProtocolMismatch);
    assert!(body.error.message.contains("Update the MCP module"));
}

#[tokio::test]
async fn a_call_reaches_the_backend_with_the_client_name() {
    let s = Running::start(ServeOptions::default()).await;
    let session = s.hello("claude-code").await;
    let r: CallResponse = s.post("/v1/call").json(&json!({"session": session, "tool": "list_binder", "args": {"include_trashed": true}})).send().await.unwrap().json().await.unwrap();
    match r {
        CallResponse::Ok(v) => {
            assert_eq!(v["tool"], "list_binder");
            assert_eq!(v["client"], "claude-code");
            assert_eq!(v["args"]["include_trashed"], true);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(s.backend.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_backend_error_reaches_the_bridge_with_its_data() {
    let s = Running::start(ServeOptions::default()).await;
    let session = s.hello("c").await;
    let r: CallResponse = s.post("/v1/call").json(&json!({"session": session, "tool": "fail"})).send().await.unwrap().json().await.unwrap();
    match r {
        CallResponse::Error(e) => {
            assert_eq!(e.kind, ErrorKind::PermissionRequired);
            assert_eq!(e.data.unwrap()["required"], "allow_edits");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn an_unknown_session_is_not_found() {
    let s = Running::start(ServeOptions::default()).await;
    let r = s.post("/v1/call").json(&json!({"session": "bogus", "tool": "x"})).send().await.unwrap();
    assert_eq!(r.status(), 404);
    assert!(s.backend.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn notifications_reach_every_client_once() {
    let s = Running::start(ServeOptions::default()).await;
    let a = s.hello("a").await;
    let b = s.hello("b").await;
    s.notifier.notify("corylus://document/Ch1.md");
    s.notifier.notify("corylus://document/Ch1.md");
    s.notifier.notify("corylus://binder");
    for session in [a, b] {
        let r: EventsResponse = s.get(&format!("/v1/events?session={session}")).send().await.unwrap().json().await.unwrap();
        let uris: Vec<_> = r.events.iter().map(|e| e.uri.as_str()).collect();
        assert_eq!(uris, ["corylus://document/Ch1.md", "corylus://binder"]);
    }
}

#[tokio::test]
async fn a_waiting_poll_wakes_as_soon_as_something_changes() {
    let s = Running::start(ServeOptions { idle_timeout: Duration::from_secs(30), poll_wait: Duration::from_secs(10) }).await;
    let session = s.hello("a").await;
    let notifier = s.notifier.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        notifier.notify("corylus://project");
    });
    let started = Instant::now();
    let r: EventsResponse = s.get(&format!("/v1/events?session={session}")).send().await.unwrap().json().await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(r.events[0].uri, "corylus://project");
}

#[tokio::test]
async fn clients_appear_and_leave_on_bye_and_on_idle_timeout() {
    let s = Running::start(ServeOptions { idle_timeout: Duration::from_millis(300), poll_wait: Duration::from_millis(50) }).await;
    let a = s.hello("claude-ai").await;
    let _b = s.hello("claude-code").await;
    assert_eq!(s.notifier.clients().len(), 2);

    let r = s.post("/v1/bye").json(&json!({"session": a})).send().await.unwrap();
    assert_eq!(r.status(), 204);
    let names: Vec<_> = s.notifier.clients().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["claude-code"]);

    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(s.notifier.clients().is_empty(), "idle session was not dropped");
    let last = s.backend.clients.lock().unwrap().last().cloned().unwrap();
    assert!(last.is_empty());
}
