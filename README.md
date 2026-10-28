# corylus-mcp

MCP access to [Corylus](https://github.com/LudoBermejoES/corylus), a desktop
writing app: lets an LLM client such as Claude Desktop or Claude Code read a
writer's project and, within the permission the writer sets, edit it and drive
the app.

You do not install this by hand. Corylus downloads the bridge from this repo's
releases when you install the MCP module in **Settings → Modules**, and
registers it with your MCP clients from there.

## How it fits together

```
MCP client ──stdio MCP──▶ corylus-mcp (bridge) ──loopback HTTP + token──▶ Corylus
```

- **The bridge** (`bridge/`, binary `corylus-mcp`) is what the MCP client
  launches. It serves the tool catalog itself, forwards each call to the running
  Corylus, and turns Corylus's change events into MCP notifications. With
  Corylus closed it still lists the tools and answers every call with
  `not_running`.
- **The library** (this crate) holds the tool catalog with JSON schemas, the
  error kinds, the private bridge ↔ Corylus API (`wire`), the endpoint
  discovery file (`endpoint`), and the server Corylus hosts (`server`).
  Corylus implements `server::AgentBackend`; nothing here depends on Corylus.

Corylus listens on `127.0.0.1` only, only while it is open, and only after the
writer installed and enabled the module. Every request needs the token Corylus
writes to `<app-data>/com.corylus.app/mcp/endpoint.json` (user-only); requests
from web pages (with an `Origin` header) are refused. No secret goes into any
MCP client's configuration.

## Protocol version

`PROTOCOL_VERSION` (currently 1) covers the bridge ↔ Corylus API and the tool
arguments. Corylus refuses a bridge with another version and tells the writer to
update the module. Bump it on any incompatible change.

## Development

```sh
cargo test                                   # library
cargo test --manifest-path bridge/Cargo.toml # bridge
```

Releases: push a tag `vX.Y.Z`; the release workflow builds the bridge for
windows-x86_64, darwin-aarch64, darwin-x86_64 and linux-x86_64 and publishes the
binaries with `SHA256SUMS`. Corylus pins the version and digests it accepts.

## License

MIT
