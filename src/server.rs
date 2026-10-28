//! The loopback endpoint Corylus hosts for the bridge: authenticated tool
//! calls, client sessions, and change events delivered by long poll.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::wire::{
    CallRequest, CallResponse, Event, EventsResponse, HelloRequest, HelloResponse, PROTOCOL_HEADER, Refusal,
    SessionRequest,
};
use crate::{AgentError, ErrorKind, PROTOCOL_VERSION};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One connected MCP client, as the bridge introduced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    pub session: String,
    pub name: String,
}

/// What Corylus implements: run a tool for a client.
pub trait AgentBackend: Send + Sync + 'static {
    fn call(&self, client: Client, tool: String, args: Value) -> BoxFuture<'_, Result<Value, AgentError>>;

    /// The set of connected clients changed (connect, disconnect, timeout).
    fn clients_changed(&self, clients: Vec<Client>) {
        let _ = clients;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ServeOptions {
    /// A session that has not polled or called for this long is dropped.
    pub idle_timeout: Duration,
    /// How long an events request waits for something to report.
    pub poll_wait: Duration,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self { idle_timeout: Duration::from_secs(45), poll_wait: Duration::from_secs(20) }
    }
}

struct Session {
    name: String,
    last_seen: Instant,
    queue: Vec<Event>,
    wake: Arc<Notify>,
}

#[derive(Default)]
struct Sessions(Mutex<HashMap<String, Session>>);

impl Sessions {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Session>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn clients(&self) -> Vec<Client> {
        let mut v: Vec<Client> = self
            .lock()
            .iter()
            .map(|(id, s)| Client { session: id.clone(), name: s.name.clone() })
            .collect();
        v.sort_by(|a, b| a.session.cmp(&b.session));
        v
    }
}

/// Queues change events for every connected client. Cheap to clone; Corylus
/// keeps one and calls [`Notifier::notify`] from its event pipeline.
#[derive(Clone, Default)]
pub struct Notifier {
    sessions: Arc<Sessions>,
}

impl Notifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn notify(&self, uri: impl Into<String>) {
        let event = Event { uri: uri.into() };
        for s in self.sessions.lock().values_mut() {
            // Collapse repeats: the client only needs to know *what* changed.
            if !s.queue.contains(&event) {
                s.queue.push(event.clone());
            }
            s.wake.notify_one();
        }
    }

    pub fn clients(&self) -> Vec<Client> {
        self.sessions.clients()
    }
}

#[derive(Clone)]
struct AppState {
    backend: Arc<dyn AgentBackend>,
    sessions: Arc<Sessions>,
    token: Arc<str>,
    options: ServeOptions,
}

impl AppState {
    fn changed(&self) {
        self.backend.clients_changed(self.sessions.clients());
    }

    fn touch(&self, session: &str) -> Option<Client> {
        let mut map = self.sessions.lock();
        let s = map.get_mut(session)?;
        s.last_seen = Instant::now();
        Some(Client { session: session.to_string(), name: s.name.clone() })
    }
}

/// Serve the bridge API on `listener` until `shutdown` is cancelled.
pub async fn serve(
    listener: TcpListener,
    backend: Arc<dyn AgentBackend>,
    token: String,
    notifier: Notifier,
    options: ServeOptions,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let state = AppState { backend, sessions: notifier.sessions.clone(), token: token.into(), options };

    let reaper = {
        let state = state.clone();
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            let tick = (state.options.idle_timeout / 4).max(Duration::from_millis(50));
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = tokio::time::sleep(tick) => {}
                }
                let removed = {
                    let mut map = state.sessions.lock();
                    let before = map.len();
                    map.retain(|_, s| s.last_seen.elapsed() < state.options.idle_timeout);
                    before != map.len()
                };
                if removed {
                    state.changed();
                }
            }
        })
    };

    let app = Router::new()
        .route("/v1/hello", post(hello))
        .route("/v1/call", post(call))
        .route("/v1/events", get(events))
        .route("/v1/bye", post(bye))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state.clone());

    let result = axum::serve(listener, app).with_graceful_shutdown(shutdown.cancelled_owned()).await;
    reaper.abort();
    state.sessions.lock().clear();
    state.changed();
    result
}

fn refuse(status: StatusCode, kind: ErrorKind, message: &str) -> Response {
    (status, Json(Refusal { error: AgentError::new(kind, message) })).into_response()
}

/// Constant-time comparison, so the token cannot be guessed byte by byte.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn guard(State(state): State<AppState>, headers: HeaderMap, req: Request, next: Next) -> Response {
    // Browsers always send Origin on cross-site requests; the bridge never does.
    if headers.contains_key(header::ORIGIN) {
        return refuse(StatusCode::FORBIDDEN, ErrorKind::Internal, "requests from web pages are not accepted");
    }
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !same(presented.as_bytes(), state.token.as_bytes()) {
        return refuse(StatusCode::UNAUTHORIZED, ErrorKind::Internal, "authentication failed");
    }
    let version = headers.get(PROTOCOL_HEADER).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u32>().ok());
    if version != Some(PROTOCOL_VERSION) {
        return refuse(
            StatusCode::UPGRADE_REQUIRED,
            ErrorKind::ProtocolMismatch,
            "This version of the Corylus MCP bridge is not supported by the running Corylus. Update the MCP module in Corylus (Settings → Modules).",
        );
    }
    next.run(req).await
}

async fn hello(State(state): State<AppState>, Json(req): Json<HelloRequest>) -> Json<HelloResponse> {
    let session = crate::endpoint::new_token()[..32].to_string();
    state.sessions.lock().insert(
        session.clone(),
        Session { name: req.client, last_seen: Instant::now(), queue: Vec::new(), wake: Arc::new(Notify::new()) },
    );
    state.changed();
    Json(HelloResponse { session })
}

fn unknown_session() -> Response {
    refuse(StatusCode::NOT_FOUND, ErrorKind::NotFound, "session expired; say hello again")
}

async fn call(State(state): State<AppState>, Json(req): Json<CallRequest>) -> Response {
    let Some(client) = state.touch(&req.session) else { return unknown_session() };
    let result = state.backend.call(client, req.tool, req.args).await;
    state.touch(&req.session);
    let body = match result {
        Ok(v) => CallResponse::Ok(v),
        Err(e) => CallResponse::Error(e),
    };
    Json(body).into_response()
}

async fn events(State(state): State<AppState>, Query(req): Query<SessionRequest>) -> Response {
    let wake = {
        let mut map = state.sessions.lock();
        let Some(s) = map.get_mut(&req.session) else { return unknown_session() };
        s.last_seen = Instant::now();
        if !s.queue.is_empty() {
            return Json(EventsResponse { events: std::mem::take(&mut s.queue) }).into_response();
        }
        s.wake.clone()
    };
    let _ = tokio::time::timeout(state.options.poll_wait, wake.notified()).await;
    let mut map = state.sessions.lock();
    let Some(s) = map.get_mut(&req.session) else { return unknown_session() };
    s.last_seen = Instant::now();
    Json(EventsResponse { events: std::mem::take(&mut s.queue) }).into_response()
}

async fn bye(State(state): State<AppState>, Json(req): Json<SessionRequest>) -> StatusCode {
    if state.sessions.lock().remove(&req.session).is_some() {
        state.changed();
    }
    StatusCode::NO_CONTENT
}
