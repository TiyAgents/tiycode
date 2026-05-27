# TiyCode ACP Server

TiyCode can expose its existing Rust agent runtime as an Agent Client Protocol (ACP) server.

## Entrypoints

- `tiycode acp --stdio` starts a headless ACP server over stdin/stdout. Stdout is reserved for ACP JSON-RPC traffic; logs continue to stderr and the normal log files.
- `TIY_ACP_HTTP_LISTEN=127.0.0.1:0 npm run dev` enables the optional desktop HTTP/WebSocket endpoint. The server listens only on loopback addresses and exposes:
  - `GET /health` for a basic readiness check.
  - `GET /acp` as a WebSocket transport carrying ACP JSON-RPC line messages.

HTTP/WebSocket is opt-in and disabled by default.

## Capability strategy

TiyCode keeps file and terminal execution local to the agent runtime. ACP clients receive streamed assistant messages, tool call status/results, plan updates, and file-change metadata, but TiyCode does not ask the client to perform `fs/*` or `terminal/*` requests in this phase.

Tool updates use ACP `ToolKind` values as follows: filesystem reads map to `read`, file mutations map to `edit`/`delete`/`move`, searches map to `search`, shell and terminal controls map to `execute`, and planning/task-board tools map to `think`. Raw tool input/output is included where available so clients can render detailed results without executing the operation themselves.

## Permission flow

When the existing TiyCode policy engine requires approval, ACP clients receive a `session/request_permission` request with `allow_once` and `reject_once` options. The selected option is bridged back into `ToolGateway::resolve_approval`, so the normal audit and policy flow remains authoritative. If the client cancels, disconnects, or does not answer within 60 seconds, TiyCode rejects the pending tool call to avoid leaving the run stuck in `waiting_approval`.

## Session mapping

ACP `SessionId` values map to TiyCode thread IDs. `session/new` creates or reuses a workspace for the requested `cwd`, creates a TiyCode thread, and returns that thread ID as the ACP session ID. `session/load`, `session/list`, `session/prompt`, `session/cancel`, and `session/close` bridge to the existing thread and run managers.
