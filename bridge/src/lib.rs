//! The stdio MCP server an MCP client launches. It serves the tool catalog
//! itself and forwards each call to the running Corylus over the loopback API;
//! with Corylus closed, every call answers `not_running`.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use corylus_mcp::catalog::catalog;
use corylus_mcp::endpoint::{EndpointInfo, read_endpoint};
use corylus_mcp::wire::{
    CallRequest, CallResponse, EventsResponse, HelloRequest, HelloResponse, PROTOCOL_HEADER, Refusal,
    SessionRequest,
};
use corylus_mcp::{AgentError, ErrorKind, PROTOCOL_VERSION};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, ServerCapabilities, ServerConfig, SubscribeRequestParams,
    SubscriptionFilter, Tool, UnsubscribeRequestParams,
};
use rmcp::service::{NotificationContext, Peer, RequestContext, SubscriptionContext, SubscriptionSink};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Value, json};

/// How long to wait before trying a closed Corylus again.
const RETRY: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
struct Conn {
    base: String,
    token: String,
    session: String,
}

#[derive(Default)]
struct Subscribers {
    /// 2026-07-28 clients: one sink per `subscriptions/listen`.
    sinks: Vec<SubscriptionSink>,
    /// Older clients: their peer and the URIs they subscribed to.
    legacy: Option<(Peer<RoleServer>, HashSet<String>)>,
}

struct Inner {
    http: reqwest::Client,
    app_data: PathBuf,
    client_name: Mutex<String>,
    conn: tokio::sync::Mutex<Option<Conn>>,
    subscribers: Mutex<Subscribers>,
    poller_started: AtomicBool,
}

#[derive(Clone)]
pub struct Bridge {
    inner: Arc<Inner>,
}

/// What went wrong talking to Corylus.
enum Failure {
    /// Corylus is not reachable: no endpoint file, or nothing listening.
    Down,
    /// The session expired (Corylus restarted or dropped it); say hello again.
    Expired,
    /// The token in the endpoint file was regenerated under us.
    Unauthorized,
    Refused(AgentError),
}

impl Bridge {
    pub fn new(app_data: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                http: reqwest::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(150))
                    .build()
                    .expect("HTTP client"),
                app_data,
                client_name: Mutex::new("MCP client".into()),
                conn: tokio::sync::Mutex::new(None),
                subscribers: Mutex::new(Subscribers::default()),
                poller_started: AtomicBool::new(false),
            }),
        }
    }

    fn client_name(&self) -> String {
        self.inner.client_name.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn request(&self, method: reqwest::Method, conn_base: &str, token: &str, path: &str) -> reqwest::RequestBuilder {
        self.inner
            .http
            .request(method, format!("{conn_base}{path}"))
            .bearer_auth(token)
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION.to_string())
    }

    async fn classify(resp: Result<reqwest::Response, reqwest::Error>) -> Result<reqwest::Response, Failure> {
        let resp = resp.map_err(|_| Failure::Down)?;
        match resp.status().as_u16() {
            200..=299 => Ok(resp),
            401 => Err(Failure::Unauthorized),
            404 => Err(Failure::Expired),
            _ => match resp.json::<Refusal>().await {
                Ok(r) => Err(Failure::Refused(r.error)),
                Err(_) => Err(Failure::Down),
            },
        }
    }

    async fn hello(&self, info: &EndpointInfo) -> Result<Conn, Failure> {
        let base = format!("http://127.0.0.1:{}", info.port);
        let resp = self
            .request(reqwest::Method::POST, &base, &info.token, "/v1/hello")
            .json(&HelloRequest { client: self.client_name() })
            .send()
            .await;
        let hello: HelloResponse = Self::classify(resp).await?.json().await.map_err(|_| Failure::Down)?;
        Ok(Conn { base, token: info.token.clone(), session: hello.session })
    }

    /// The current connection, opening one if needed.
    async fn conn(&self) -> Result<Conn, Failure> {
        let mut guard = self.inner.conn.lock().await;
        if let Some(c) = guard.as_ref() {
            return Ok(c.clone());
        }
        let info = read_endpoint(&self.inner.app_data).map_err(|_| Failure::Down)?;
        let conn = self.hello(&info).await?;
        *guard = Some(conn.clone());
        Ok(conn)
    }

    async fn drop_conn(&self) {
        *self.inner.conn.lock().await = None;
    }

    /// Run one tool call in Corylus, reconnecting once if the session or the
    /// token went stale.
    pub async fn call(&self, tool: &str, args: Value) -> Result<Value, AgentError> {
        for attempt in 0..2 {
            let conn = match self.conn().await {
                Ok(c) => c,
                Err(Failure::Refused(e)) => return Err(e),
                Err(Failure::Unauthorized | Failure::Expired) if attempt == 0 => {
                    self.drop_conn().await;
                    continue;
                }
                Err(_) => return Err(AgentError::not_running()),
            };
            let resp = self
                .request(reqwest::Method::POST, &conn.base, &conn.token, "/v1/call")
                .json(&CallRequest { session: conn.session.clone(), tool: tool.to_string(), args: args.clone() })
                .send()
                .await;
            match Self::classify(resp).await {
                Ok(r) => {
                    return match r.json::<CallResponse>().await {
                        Ok(CallResponse::Ok(v)) => Ok(v),
                        Ok(CallResponse::Error(e)) => Err(e),
                        Err(e) => Err(AgentError::new(ErrorKind::Internal, format!("bad reply from Corylus: {e}"))),
                    };
                }
                Err(Failure::Refused(e)) => return Err(e),
                Err(Failure::Expired | Failure::Unauthorized) if attempt == 0 => {
                    self.drop_conn().await;
                }
                Err(_) => {
                    self.drop_conn().await;
                    return Err(AgentError::not_running());
                }
            }
        }
        Err(AgentError::not_running())
    }

    /// Keep a session open with Corylus and turn its change events into MCP
    /// notifications, for as long as the bridge runs. The session is also what
    /// makes the client show up as connected in Corylus.
    pub fn start_poller(&self) {
        if self.inner.poller_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                let conn = match me.conn().await {
                    Ok(c) => c,
                    Err(Failure::Unauthorized | Failure::Expired) => {
                        me.drop_conn().await;
                        continue;
                    }
                    Err(_) => {
                        tokio::time::sleep(RETRY).await;
                        continue;
                    }
                };
                let resp = me
                    .request(reqwest::Method::GET, &conn.base, &conn.token, "/v1/events")
                    .query(&[("session", conn.session.as_str())])
                    .send()
                    .await;
                match Self::classify(resp).await {
                    Ok(r) => {
                        if let Ok(events) = r.json::<EventsResponse>().await {
                            for e in events.events {
                                me.deliver(&e.uri).await;
                            }
                        }
                    }
                    Err(Failure::Expired | Failure::Unauthorized) => me.drop_conn().await,
                    Err(_) => {
                        me.drop_conn().await;
                        tokio::time::sleep(RETRY).await;
                    }
                }
            }
        });
    }

    async fn deliver(&self, uri: &str) {
        let (sinks, legacy) = {
            let s = self.inner.subscribers.lock().unwrap_or_else(|p| p.into_inner());
            let legacy = s.legacy.as_ref().and_then(|(peer, uris)| {
                uris.iter().any(|u| uri.starts_with(u.as_str())).then(|| peer.clone())
            });
            (s.sinks.clone(), legacy)
        };
        for sink in sinks {
            // A sink refuses URIs its client did not ask for; that is fine.
            let _ = sink.notify_resource_updated(uri).await;
            if uri == "corylus://binder" {
                let _ = sink.notify_resource_list_changed().await;
            }
        }
        if let Some(peer) = legacy {
            let _ = peer
                .notify_resource_updated(rmcp::model::ResourceUpdatedNotificationParam::new(uri))
                .await;
        }
    }

    /// Tell Corylus this client is leaving, so it stops showing as connected.
    pub async fn goodbye(&self) {
        if let Some(conn) = self.inner.conn.lock().await.take() {
            let _ = self
                .request(reqwest::Method::POST, &conn.base, &conn.token, "/v1/bye")
                .json(&SessionRequest { session: conn.session })
                .timeout(Duration::from_secs(2))
                .send()
                .await;
        }
    }
}

fn tool_result(result: Result<Value, AgentError>) -> CallToolResult {
    match result {
        Ok(v) => {
            let text = serde_json::to_string_pretty(&v).unwrap_or_default();
            let mut r = CallToolResult::success(vec![ContentBlock::text(text)]);
            if v.is_object() {
                r.structured_content = Some(v);
            }
            r
        }
        Err(e) => {
            let body = json!({ "error": e.kind, "message": e.message, "data": e.data });
            CallToolResult::error(vec![ContentBlock::text(
                serde_json::to_string_pretty(&body).unwrap_or(e.message),
            )])
        }
    }
}

/// The URIs a client can subscribe to; documents and entries use
/// `corylus://document/<path>` and `corylus://bible/<path>`.
const RESOURCES: [(&str, &str, &str); 2] = [
    ("corylus://project", "project", "The open project and your permission level on it"),
    ("corylus://binder", "binder", "The binder tree of the open project"),
];

impl ServerHandler for Bridge {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder().enable_tools().enable_resources().enable_resources_subscribe().build(),
        )
        .with_server_info(Implementation::new("corylus", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "Tools for the Corylus writing app: read and, when the writer allows it, edit the open project \
             (binder, documents, Story Bible, snapshots, comments) and drive Corylus's interface. Call \
             corylus_status first: it tells you whether a project is open and your permission level. \
             Address items by binder id or project-relative path. Changes may wait for the writer's \
             confirmation in Corylus.",
        )
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        if let Some(info) = context.peer.peer_info() {
            let i = &info.client_info;
            *self.inner.client_name.lock().unwrap_or_else(|p| p.into_inner()) =
                i.title.clone().unwrap_or_else(|| i.name.clone());
        }
        self.inner.subscribers.lock().unwrap_or_else(|p| p.into_inner()).legacy =
            Some((context.peer.clone(), HashSet::new()));
        self.start_poller();
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        self.start_poller();
        let tools = catalog()
            .iter()
            .map(|t| Tool::new(t.name, t.description, Arc::new(t.input_schema.clone())))
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        self.start_poller();
        let args = request.arguments.map(Value::Object).unwrap_or(Value::Null);
        let result = if corylus_mcp::catalog::find(&request.name).is_none() {
            Err(AgentError::invalid(format!("unknown tool {}", request.name)))
        } else {
            self.call(&request.name, args).await
        };
        Ok(tool_result(result).into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let items = RESOURCES
            .iter()
            .map(|(uri, name, desc)| Resource::new(*uri, *name).with_description(*desc))
            .collect();
        Ok(ListResourcesResult::with_all_items(items))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri.as_str();
        let (tool, args) = if uri == "corylus://project" {
            ("corylus_status", json!({}))
        } else if uri == "corylus://binder" {
            ("list_binder", json!({}))
        } else if let Some(p) = uri.strip_prefix("corylus://document/") {
            ("read_document", json!({ "item": p }))
        } else if let Some(p) = uri.strip_prefix("corylus://bible/") {
            ("read_bible_entry", json!({ "item": p }))
        } else {
            return Err(McpError::resource_not_found(format!("unknown resource {uri}"), None));
        };
        match self.call(tool, args).await {
            Ok(v) => Ok(ReadResourceResult::new(vec![ResourceContents::text(
                serde_json::to_string_pretty(&v).unwrap_or_default(),
                uri,
            )])
            .into()),
            Err(e) => Err(McpError::internal_error(e.message, Some(json!({ "error": e.kind })))),
        }
    }

    #[allow(deprecated)]
    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        let mut s = self.inner.subscribers.lock().unwrap_or_else(|p| p.into_inner());
        let entry = s.legacy.get_or_insert_with(|| (context.peer.clone(), HashSet::new()));
        entry.1.insert(request.uri);
        Ok(())
    }

    #[allow(deprecated)]
    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        if let Some((_, uris)) = self.inner.subscribers.lock().unwrap_or_else(|p| p.into_inner()).legacy.as_mut() {
            uris.remove(&request.uri);
        }
        Ok(())
    }

    fn accepted_subscription_filter(&self, requested: &SubscriptionFilter) -> Option<SubscriptionFilter> {
        Some(requested.supported_by(&self.get_info().capabilities))
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), McpError> {
        self.start_poller();
        let sink = context.sink().clone();
        self.inner.subscribers.lock().unwrap_or_else(|p| p.into_inner()).sinks.push(sink.clone());
        context.cancelled().await;
        self.inner
            .subscribers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .sinks
            .retain(|s| s.id() != sink.id());
        Ok(())
    }
}
