# Local agent analysis

The current adapter implements authenticated **analysis only**. Plan creation, independent approval and approved execution remain pending. No delete, shell, approval or bin-purge tool is registered.

## Build and verify

```powershell
cargo build --release -p voldiq-headless
cargo test -p voldiq-headless
```

The executable is `target/release/voldiq-cli.exe`, separate from the desktop `voldiq.exe`. Its normal dependency tree includes `voldiq-core` and the official [`rmcp` Rust SDK](https://github.com/modelcontextprotocol/rust-sdk), locked to 3.8.0. It has no Tauri, WebView or production Node dependency. The SDK handles standard MCP negotiation; the retained SDK-client fixture verifies a real stdio handshake and tool calls against the executable. Broader client-version compatibility is not yet certified.

In the desktop application, open Settings, then Agents and permissions. Enter a connection name, allowed existing folders, optional exclusions and limits. Review the separate native confirmation dialog. Voldiq reveals the credential once for you to save in the client environment. The section lists connections and their last use, and provides rotation and revocation. This UI is source-checked; native interaction acceptance remains manual. The CLI and MCP do not expose provisioning, rotation or approval. Do not create credentials by editing a database or configuration file.

## Connect a stdio client

Supply a separately human-provisioned connection credential through the client's process environment as `VOLDIQ_AGENT_TOKEN`. Never put it in arguments, Git, logs or reports. Each connection has its own credential and explicit root/exclusion/capability/limit policy. Revocation and rotation take effect on subsequent calls, including an already connected client.

Generic launch contract:

```text
command: absolute path to voldiq-cli.exe
args: ["mcp", "serve", "--stdio"]
environment: forward VOLDIQ_AGENT_TOKEN securely from the local environment
```

Default state is the desktop's `%LOCALAPPDATA%/io.voldiq.desktop` directory. Tests and portable deployments can supply `--state <directory>` before the command. Separate state has separate credentials, jobs and snapshots; it does not inherit access from another registry.

For Codex, merge a new entry into existing configuration after reviewing it. The [official stdio documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli) and [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference) were checked on 2026-10-31, together with the installed `codex mcp add --help`.

```toml
[mcp_servers.voldiq]
command = 'C:\absolute\path\voldiq-cli.exe'
args = ["mcp", "serve", "--stdio"]
env_vars = ["VOLDIQ_AGENT_TOKEN"]
```

This example forwards an environment variable by name, without embedding its value. The launching Codex process must have that variable. Existing client approvals and other server settings remain intact. This task has not modified the user's Codex configuration or connected this chat to Voldiq.

## Tools and limits

| Tool | Capability | Result |
|---|---|---|
| `voldiq_capabilities` | Metadata | Actual permissions, limits and execution availability |
| `voldiq_scan_start` | Scan; metadata needed to read completion | Persistent job ID; one shared scan lease |
| `voldiq_job_get` | Metadata | Owner-scoped job, progress, terminal state, snapshot ID |
| `voldiq_job_cancel` | Scan | Cooperative cancellation request for an owned job |
| `voldiq_stats` | Metadata | Indexed logical bytes, unique known allocation or null, counts and coverage |
| `voldiq_query` | Metadata | Bounded page with explicit structured filters |
| `voldiq_details` | Metadata | One indexed entry, path and ancestor metadata |

All calls authorize in the backend. A client name, root announcement or snapshot ID grants no permission. Query pages use `query.offset` and `query.limit`; `total` and `largest` refer only to the authorized indexed dataset. Unknown fields and unsupported sort names are rejected. The advanced text query parser is still pending; `query.text` currently means literal name search.

Incoming protocol lines are bounded to 64 KiB, tool arguments to 16 KiB and structured responses to 512 KiB. The fixed seven-tool catalogue fits one page. At most four tool workers execute concurrently; their permits remain held even if a request disconnects before its blocking worker finishes. Query pages respect the connection limit, at most 500 entries. The service caches one loaded index, never serializes the entire tree, and rejects snapshots whose exclusion coverage no longer satisfies the current policy.

Currently a connection can query its own completed scan snapshots. Sharing authorized desktop snapshots across connections is pending. Scan results persist in the existing compact snapshot database. The GUI and agents acquire the same OS scan lease; they cannot start competing full scans. Durable job history retains at most 10,000 scan records. It is not the security audit ledger for future file operations.

CLI commands are listed by `voldiq-cli --help`. JSON envelopes have `schema_version: 1`. Exit codes are 0 success, 2 invalid arguments, 3 missing/invalid credential, 4 unavailable/failed operation and 130 cancelled foreground scan. CLI scans remain in the foreground; `--wait` is accepted explicitly. Initial job identity goes to stderr, final JSON to stdout. Ctrl+C requests cancellation and waits for the worker to finish. MCP reserves stdout entirely for the protocol.

Jobs belong to the process doing the work, independently of a particular tool request. If a client kills its stdio server, the OS releases the lease. A job-status query reconciles that owner's unfinished records as interrupted when it can acquire the free lease; a new agent scan also reconciles stale records before starting. A busy lease never proves interruption. There is no resident daemon and no automatic retry. SQLite transactions preserve committed snapshots. Cooperative cancellation cannot interrupt a blocked operating-system call.

## Boundaries

Metadata stays local until the client receives it. An AI client may send it to its provider. Stdio is a transport, not a promise that nothing leaves the device.

These checks control operations through Voldiq. They do not sandbox an agent that independently has filesystem access under the same Windows account. Local SQLite state and hashes are not claimed tamper-proof against such a process. No tool treats file names, notes or other indexed text as instructions.

Verification status: transport and seven analysis tools implemented; connected and verified with the real official SDK client on owned fixtures; not configured in the user's clients. Human provisioning UI is implemented with native dialogs but still awaits manual interaction acceptance. The required immutable-plan approval/execution lifecycle remains pending.
