use serde::{Deserialize, Serialize};

/// Why a tool call did not run. The bridge turns this into an MCP tool error
/// (`isError: true`) whose text the agent can read and explain to the writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Corylus is closed, or the module is disabled.
    NotRunning,
    /// The bridge and Corylus speak different protocol versions.
    ProtocolMismatch,
    /// The writer paused agent access for this session.
    Paused,
    /// The tool needs an open project and none is open.
    NoProject,
    /// The project's agent permission level is Off.
    AccessOff,
    /// The project's level does not allow this; `data.required` names the level.
    PermissionRequired,
    /// The writer rejected the change, or did not answer in time.
    Declined,
    /// The target changed since the agent read it; `data.current_hash` is the
    /// version to read again.
    Stale,
    NotFound,
    /// A quoted passage matched more than once; `data.matches` is the count.
    Ambiguous,
    InvalidInput,
    /// The Corylus window did not answer in time.
    Timeout,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentError {
    pub kind: ErrorKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl AgentError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), data: None }
    }

    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    pub fn not_running() -> Self {
        Self::new(
            ErrorKind::NotRunning,
            "Corylus is not running. Open Corylus (with the MCP module enabled in Settings) and try again.",
        )
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, message)
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for AgentError {}
