//! The private loopback API between the bridge and Corylus. Only the bridge
//! speaks it; MCP clients speak MCP to the bridge.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AgentError;

/// Header every bridge request carries; Corylus refuses other versions.
pub const PROTOCOL_HEADER: &str = "x-corylus-mcp-protocol";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloRequest {
    /// The MCP client's name as it introduced itself to the bridge, e.g.
    /// "claude-ai" or "claude-code".
    pub client: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloResponse {
    pub session: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRequest {
    pub session: String,
    pub tool: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallResponse {
    Ok(Value),
    Error(AgentError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRequest {
    pub session: String,
}

/// Something in the open project changed. Carries no document text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// `corylus://project`, `corylus://binder`, `corylus://document/<path>` or
    /// `corylus://bible/<path>`.
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsResponse {
    pub events: Vec<Event>,
}

/// Body of a refused request (401, 403, 426).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Refusal {
    pub error: AgentError,
}
