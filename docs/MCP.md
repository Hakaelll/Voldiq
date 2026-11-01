# Local agent analysis and plans

The adapter implements authenticated analysis, exact recycling plans and execution/cancellation requests to the open Windows desktop. Humans review and approve each exact plan in Voldiq. No arbitrary delete, shell, approval-granting or bin-purge tool is registered. Full elevated/Shell acceptance through this new agent route remains manual.

## Build and verify

```powershell
cargo build --release -p voldiq-headless
cargo test -p voldiq-headless
```

The executable is `target/release/voldiq-cli.exe`, separate from the desktop `voldiq.exe`. Its normal dependency tree includes `voldiq-core` and the official [`rmcp` Rust SDK](https://github.com/modelcontextprotocol/rust-sdk), locked to 3.8.0. It has no Tauri, WebView or production Node dependency. The SDK handles standard MCP negotiation; the retained SDK-client fixture verifies a real stdio handshake and tool calls against the executable. Broader client-version compatibility is not yet certified.

In the desktop application, open Settings, then Agents and permissions. Enter a connection name, allowed existing folders, optional exclusions and limits. Scanning, content reading, planning and execution requests are separate permissions. Content, planning and execution are off by default. Review the separate native confirmation dialog. Voldiq reveals the credential once for you to save in the client environment. The section lists connections and their last use, and provides rotation and revocation. This UI is source-checked; native interaction acceptance remains manual. The CLI and MCP do not expose provisioning, rotation or approval grants. Do not create credentials by editing a database or configuration file.

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
| `voldiq_folder_metadata` | Metadata; ReadContent to include note text | Current protection status and optional plain-text note for the exact indexed directory identity |
| `voldiq_history` | Metadata | Paged compatible snapshots, including desktop captures |
| `voldiq_plan_prepare` | PreparePlans + Metadata | Immutable manifest from exact snapshot node IDs, freshly checked against disk |
| `voldiq_plan_get` | Metadata | Owned manifest, lifecycle state and bounded result summary |
| `voldiq_plan_request_approval` | RequestApproval + Metadata | Records a pending request, then returns `needs_human_approval` |
| `voldiq_plan_audit` | Metadata | Owned events paged after a sequence number |
| `voldiq_plan_execute` | ExecuteApproved + Metadata | Queues the exact plan ID and manifest hash for the desktop; returns `dispatch: queued` or an existing state |
| `voldiq_plan_cancel` | ExecuteApproved + Metadata | Cancels an owned queued plan or requests a running worker to stop safely |

All calls authorize in the backend. A client name, root announcement or snapshot ID grants no permission. Query pages use `query.offset` and `query.limit`; `total` and `largest` refer only to the authorized indexed dataset. Unknown fields and unsupported sort names are rejected. The advanced text query parser is still pending; `query.text` currently means literal name search.

Supported sort values are `size` (logical bytes, default), `allocated` (known allocation), `name` and `modified`. Allocation ordering includes explicitly partial positive evidence and keeps wholly unknown allocation after measured zero. Callers must retain `allocation_known`; ordering does not turn partial evidence into a complete measurement. `min_size` and `largest` remain logical bytes for every sort. CLI JSON queries use the same contract.

Incoming protocol lines are bounded to 64 KiB, tool arguments to 16 KiB and structured responses to 512 KiB. The fixed fifteen-tool catalogue fits one page. At most four tool workers execute concurrently; their permits remain held even if a request disconnects before its blocking worker finishes. Query pages respect the connection limit, at most 500 entries. The service caches one loaded index, never serializes the entire tree, and rejects snapshots whose exclusion coverage no longer satisfies the current policy.

Connections can query saved snapshots from the desktop or another connection only when the current native root identity and exclusion coverage satisfy their own policy. The service checks small scope metadata before loading the index. Broader captures are rejected as a whole, not silently filtered into misleading totals. A snapshot ID grants no authority. Jobs remain private to their initiating connection.

`voldiq_history` and CLI `history [offset] [limit]` return at most 100 authorized snapshots per page. Offsets count visible captures. One SQLite read view keeps multi-page selection consistent during concurrent captures. A request examines at most 10,000 metadata rows; `limited: true` explicitly marks incomplete discovery. `next_offset` refers only to a known next authorized page. Matching `root_key` values identify the same stored native root and exact exclusions; do not infer growth from captures with incompatible coverage. Current authorization requires that the native root identity can still be verified; disconnected or replaced roots fail closed.

Agent scan results persist in the existing compact snapshot database. Desktop scans, duplicate verification, cleanup, restoration and snapshot work now use the same core OS operation lease as headless scans. A native guard retains it until the worker exits, including after cancellation or local completion acknowledgement. The existing `scan.lock` filename is retained for compatibility with earlier scan clients; it is not approval proof. Durable job history retains at most 10,000 agent scan records. GUI job mirroring and automatic shared-index persistence remain pending. The job table is not the file-operation security audit ledger.

CLI commands are listed by `voldiq-cli --help`. JSON envelopes have `schema_version: 1`. Exit codes are 0 success, 2 invalid arguments, 3 missing/invalid credential, 4 unavailable/failed operation, 5 independent human approval required and 130 cancelled foreground scan. CLI scans remain in the foreground; `--wait` is accepted explicitly. Initial job identity goes to stderr, final JSON to stdout. Ctrl+C requests cancellation and waits for the worker to finish. MCP reserves stdout entirely for the protocol.

CLI `folder <snapshot_id> <node_id> [--note]` and MCP `voldiq_folder_metadata` with `include_note` omitted or false never read note text. Their `not_requested` value does not mean an empty note. Requesting text requires `ReadContent`; the human-only connection screen grants it explicitly. Notes remain literal untrusted text and cannot grant permissions or approve work. Reads verify the current native directory against scan evidence, so replacement or missing legacy evidence requires a rescan. A returned note is current; `source_snapshot` identifies folder evidence rather than a historical note. See [folder metadata](FOLDER-METADATA.md).

## Plan review

Use `plan prepare <snapshot_id> <JSON node IDs>` with file IDs returned by a query on that same snapshot. For example, the selection argument has the shape `[4,9]`. The backend requires complete regular files, persisted native identities and unchanged live evidence. A legacy snapshot without native evidence needs a rescan; it is never promoted to live authority. The manifest preserves the source snapshot flag and passes the same root, exclusion and protection checks.

`plan request <plan_id>` records the exact request and returns exit 5, `needs_human_approval`. This is an accepted request awaiting a human, not an approval or file operation. MCP returns that same code as a structured tool error. Use `plan get <plan_id>` or `voldiq_plan_get` for subsequent state. `plan audit <plan_id> [after_sequence] [limit]` reads bounded lifecycle events.

Settings contains Agent plan review, with 25 requests per page, connection identity, exact paths and file identities, byte strings, source scope, manifest hash and expiry. Approval requires a separate native Yes/No dialog and revalidation after it. Rejection revokes the live grant. Closing the window ends the review session and discards its grants, including while a confirmation is pending. A later session requires renewed approval. Human review does not change the connection's last-agent-use time.

Approval lasts at most two minutes and is bound to one exact manifest, credential, policy and user. The public manifest is at most 440 KiB, leaving room for the bounded result and envelope. Private native evidence is not returned through MCP. An approved status row is never sufficient authority. Transfer and restore planning remain pending. See [the core boundary](AGENT-PLANS.md).

Execution permission is a separate human provisioning option, off by default. CLI `plan execute <plan_id> <manifest_hash>` and MCP `voldiq_plan_execute` queue an exact request. The desktop worker owns the live human authority, shared operation lease, journal and native helper. Headless processes never launch a helper. Without an observed independent approval session, a fresh request returns `needs_human_approval`; the availability probe is not approval proof. A closing session can invalidate a queued request before the worker acts. Poll `plan get` for actual state and bounded per-node outcomes. Repeating a request reports existing execution or terminal state rather than repeating I/O.

CLI `plan cancel <plan_id> <manifest_hash>` and MCP `voldiq_plan_cancel` can stop an owned queued plan or request safe cancellation during execution. A running plan stays `executing` until the worker records its result. Do not interpret a cancellation request as proof that nothing moved or everything returned. Unknown and partial outcomes remain explicit. The native review screen also offers a stop request and recorded outcome counts. Native interaction and elevated/Shell acceptance remain manual.

Jobs belong to the process doing the work, independently of a particular tool request. If a client kills its stdio server, the OS releases the lease. A job-status query reconciles that owner's unfinished records as interrupted when it can acquire the free lease; a new agent scan also reconciles stale records before starting. A busy lease never proves interruption. There is no resident daemon and no automatic retry. SQLite transactions preserve committed snapshots. Cooperative cancellation cannot interrupt a blocked operating-system call.

## Boundaries

Metadata and explicitly permitted note content stay local until the client receives them. An AI client may send them to its provider. Stdio is a transport, not a promise that nothing leaves the device.

These checks control operations through Voldiq. They do not sandbox an agent that independently has filesystem access under the same Windows account. Local SQLite state and hashes are not claimed tamper-proof against such a process. No tool treats file names, notes or other indexed text as instructions.

Verification status: transport, nine analysis tools and six plan tools are implemented. Real CLI and official SDK clients check exact-hash execution/cancellation intents, absence of a human channel, rejected approval injection, replay and unchanged owned source files. A core dispatch fixture exercises independent human approval, helper refusal, persisted result and closed-session denial. Native provisioning, review, event presentation and the full elevated/Shell route await manual acceptance. User clients are not configured.
