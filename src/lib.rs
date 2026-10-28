//! # corylus-mcp
//!
//! MCP access to [Corylus](https://github.com/LudoBermejoES/corylus).
//!
//! An MCP client (Claude Desktop, Claude Code, …) launches the `corylus-mcp`
//! bridge (in `bridge/`) as a stdio MCP server. The bridge serves the tool
//! [`catalog`] and forwards each call to a running Corylus over a private,
//! token-protected loopback API ([`wire`]) that Corylus hosts with [`server`],
//! finding it through [`endpoint`]. With Corylus closed the bridge still lists
//! the tools and answers every call with `not_running`.
//!
//! Corylus implements [`server::AgentBackend`]; this crate knows nothing about
//! Corylus's internals.

pub mod catalog;
pub mod endpoint;
mod error;
pub mod server;
pub mod wire;

pub use error::{AgentError, ErrorKind};

/// Version of the bridge ↔ Corylus API. Bump on any incompatible change to
/// [`wire`] or to a tool's arguments.
pub const PROTOCOL_VERSION: u32 = 1;
