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
| `voldiq_query` | Metadata | Bounded page with shared structured filters and advanced expression |
| `voldiq_query_translate` | Metadata | Offline expression proposal and exact unparsed input spans; no snapshot/query execution |
| `voldiq_details` | Metadata | One indexed entry, path and ancestor metadata |
| `voldiq_folder_metadata` | Metadata; ReadContent to include note text | Current protection status and optional plain-text note for the exact indexed directory identity |
| `voldiq_folder_budget` | Metadata | Current native-identity limit, indexed logical usage, coverage, measured image-file equivalent and qualified limit forecast |
| `voldiq_history` | Metadata | Paged compatible snapshots, including desktop captures |
| `voldiq_plan_prepare` | PreparePlans + Metadata | Immutable manifest from exact snapshot node IDs, freshly checked against disk |
| `voldiq_plan_get` | Metadata | Owned manifest, lifecycle state and bounded result summary |
| `voldiq_plan_request_approval` | RequestApproval + Metadata | Records a pending request, then returns `needs_human_approval` |
| `voldiq_plan_audit` | Metadata | Owned events paged after a sequence number |
| `voldiq_plan_execute` | ExecuteApproved + Metadata | Queues the exact plan ID and manifest hash for the desktop; returns `dispatch: queued` or an existing state |
| `voldiq_plan_cancel` | ExecuteApproved + Metadata | Cancels an owned queued plan or requests a running worker to stop safely |

All calls authorize in the backend. A client name, root announcement or snapshot ID grants no permission. Query pages use `query.offset` and `query.limit`; `total` and `largest` refer only to the authorized indexed dataset. Unknown fields and unsupported sort names are rejected. `query.text` remains literal name search. Optional `query.expression` uses the bounded core parser below; it intersects with structured filters, parent/scope and pagination rather than replacing them. The same compiler builds all metadata predicates, and a borrowed compiled query retains its exact source query.

An expression combines terms with AND using whitespace. Double quotes group spaces and make a whole quoted term literal. Backslashes are literal, including immediately before a closing quote; no shell expansion or escape processing is performed. Property names, name/path text and glob matching are case insensitive using lowercase Unicode scalar sequences. Expression/text are each limited to 4096 UTF-8 bytes, extension to 128 bytes, and an expression to 64 terms. An empty expression adds no predicate.

| Condition | Meaning/example |
| --- | --- |
| `report` or `"final report"` | Name contains the term or phrase |
| `name:"report (final)"` | Explicit literal name substring |
| `glob:report*.bin` | Full basename glob; `*` is any sequence and `?` one lowercase Unicode scalar |
| `path:"C:\Users\Example\Archive"` | Substring of the indexed absolute native path, never an authorization root |
| `pathglob:*Archive*movie.mp4` | Full indexed path glob; `*` may cross separators and backslashes stay literal |
| `ext:bin`, `ext:none` | Final extension, or no extension on a metadata file; `ext:.none` means the literal extension `none` |
| `category:video`, `category:image`, `category:audio` | Metadata files with one of the documented filename extensions below; no content inspection |
| `size>=1.5GiB` | Indexed logical bytes; decimal KB/MB/GB and binary KiB/MiB/GiB are distinct |
| `modified>=2024-02-29` | UTC calendar-day boundary |
| `modified=2024-02-29T23:59:59Z` | Exact captured UTC second |
| `type:file`, `type:folder`, `type:link` | Metadata file/folder classification, or raw reparse record |
| `cloud:placeholder` | One of the same five observed cloud filters described below |

Size and modification conditions accept `=`, `!=`, `<`, `<=`, `>` and `>=`; a colon on those two properties means equality. Byte units are case insensitive, through EB/EiB, and decimal fractions allow at most nine places. Checked integer arithmetic preserves fractional byte thresholds without floating-point rounding. Values outside u64 byte range are rejected. Date-only equality spans that UTC day; `>day` starts at the next day, `<=day` ends before the next day, `<day` ends before its start and `>=day` includes its start. Exact timestamps compare one second. Dates must be valid Gregorian dates from 1970 through 9999, with `Z` and no offset or fraction for timestamps.

Unknown dates never satisfy a date condition, including `!=`. A reparse record without observed file length never satisfies a size predicate. An incomplete folder exposes a lower bound: only `>` or `>=` can match when its observed bytes already prove that condition; equality, inequality and upper-bound predicates do not claim a complete total. Confirmed placeholder files retain their observed advertised length despite unknown allocation. Path predicates do not match native paths that cannot be represented losslessly as text. These are metadata conditions, never content reads or permission grants.

Globs are limited to 256 source characters. Character classes, brace expansion, boolean operators/parentheses, unknown properties and unavailable app attribution are rejected. Quote literal names containing expression punctuation. General glob matching has a shared 100,000,000-step query budget and cooperative cancellation; common exact/prefix/suffix/substring patterns use direct string matching. Exceeding a work limit fails the complete query without returning a partial page. Syntax/work errors use `invalid_query` (CLI exit 2), a UTF-8 byte position and static guidance without echoing user values. Unavailable snapshots and revoked/missing capabilities retain the existing non-disclosing operation errors. Expressions use the same core in the desktop advanced editor, saved views, native query bridge, CLI and MCP. Policy binding remains pending; native editor acceptance remains manual.

The desktop's human-maintained saved-workspace catalogue also validates queries with this compiler. It retains filters and presentation plus a current-folder/analysis-root scope policy, without transient node IDs or pagination. Loading binds to the current analysis and retains normal query checks. Favorite folders retain full observed native identity, revalidate before an explicit scan and pin their ancestors through the existing scanner's lifetime. The bounded SQLite catalogue uses revision-checked transactions and operation exclusion for writes. These preferences, labels and paths grant no agent permission. No catalogue read/write, favorite-scan, provisioning or approval tool is added to CLI/MCP; agent access to user-authored saved views would need its own explicit content/scope policy. Existing agent query and plan authorization remains unchanged.

The catalogue accepts at most 100 views and 100 favorite folders, 128 UTF-8 bytes per display name, 16 KiB per serialized view specification and 32767 UTF-8 bytes per favorite path. Exceeding a bound fails the write without a partial record or dropped filter. Saved date controls retain exact UTC midnight values; numeric filters must round-trip through the desktop's safe integer range. Unsupported schema/record/presentation values fail explicitly instead of resetting or silently adapting a view.

Category matching uses case-insensitive final extensions on metadata files, excluding ordinary directories. Video includes mp4, mkv, mov, avi, webm, m4v, wmv, mpeg and mpg. Image includes jpg, jpeg, png, gif, webp, bmp, tif, tiff, heic, heif, avif and svg. Audio includes mp3, wav, flac, aac, ogg, m4a, opus, wma and aiff. These finite sets neither identify extensionless media nor certify file content or cloud availability.

`voldiq_query_translate` accepts only `{"source":"archivos mayores de 1 GiB"}`. The CLI equivalent is `voldiq-cli translate "archivos mayores de 1 GiB"`. Both require a current Metadata capability, rechecked before translation, and use the same pure core translator as the desktop. The response contains `expression`, `conditions` and `unparsed` spans with exact original `text` and UTF-8 `start`/exclusive `end` positions. It does not load an index, authorize a path, run a query or change any file. Review and edit the expression before a separately authorized query.

Rules support explicit Spanish/English file/folder/media nouns, greater/less/at-least/at-most sizes, modification dates/ranges, named/path-contains phrases, extension/extensionless and a few observed cloud classifications. Examples are `files at least 1.5GB`, `vídeos modificados entre 2024-02-29 y 2024-03-01`, `files named "Final report"` and `files path contains "C:\Archive"`. Decimal/binary units and UTC dates use the existing exact parser. Requests are limited to 4096 UTF-8 bytes and 128 words/quoted phrases; the generated expression retains the normal 64-condition limit. Quotes preserve literal values without shell escaping. Malformed supported values fail with static guidance. Unknown words, negation, OR, relative ages and unavailable app attribution remain explicit unparsed spans. For example, `vídeos antiguos` proposes `type:file category:video` and flags `antiguos`; it never invents a cutoff date. The desktop previews the proposal, requires explicit partial-interpretation acceptance when needed and then offers an editable draft with its ordinary separate syntax validation and Apply.

Desktop syntax inspection is a bounded pure compilation with a tagged valid/invalid response, independent of an index or result-query channel. It never authorizes a path or prepares an operation. Applying a draft separately checks its query/analysis context, then normal metadata requests retain their index and scope checks. Map matches use the same compiled evaluator and its work budget, projected onto at most 350 native targets. Own and descendant record counts remain separate to avoid summing overlapping groups. Current map scope intersects any query scope; counts cover all matching records rather than a paginated subset. Matching folders and unlisted remainders retain context in every representation, while search leaves byte areas unchanged. No parser or filesystem traversal runs in the frontend.

Supported sort values are `size` (logical bytes, default), `allocated` (known allocation), `name` and `modified`. Allocation ordering includes explicitly partial positive evidence and keeps wholly unknown allocation after measured zero. Callers must retain `allocation_known`; ordering does not turn partial evidence into a complete measurement. `min_size` and `largest` remain logical bytes for every sort. CLI JSON queries use the same contract.

Optional `query.cloud` accepts `placeholder`, `recall_required`, `no_recall_flag`, `not_placeholder` and `unknown`; null/omission leaves it unrestricted. Every entry returns its captured `cloud` classification. Known placeholders distinguish file/folder and recall/no-recall-flag observations. Unknown includes legacy snapshots, unsupported providers/tags and unavailable evidence. `not_placeholder` concerns the detected Windows Cloud Files placeholder representation, not remote synchronization or backup ownership. No flag absence or pin intent proves that content is fully local. The handle-only classifier excludes the EA/RECALL_ON_OPEN alias and uses unambiguous offline/data-access indicators; see [Microsoft's attribute definitions](https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants).

Metadata file queries and file-type totals include the observed logical length of confirmed placeholder files. Their serialized `kind` remains `link`, with incomplete coverage/allocation; this does not grant content access or make them cleanup candidates. `files_only` and `kind:file` describe metadata file classification, while `kind:link` retains the reparse-record view. Cloud folders remain untraversed with incomplete subtree coverage. Snapshot schema 6 preserves classifications; old records migrate to `unknown` and need another scan for observation. CLI and MCP use the same filters, scope checks and cancellation. GUI cloud controls use these same predicates, with a bounded core projection for map totals. Live provider acceptance remains pending.

Incoming protocol lines are bounded to 64 KiB, tool arguments to 16 KiB and structured responses to 512 KiB. The fixed seventeen-tool catalogue fits one page. At most four tool workers execute concurrently; their permits remain held even if a request disconnects before its blocking worker finishes. Query pages respect the connection limit, at most 500 entries. The service caches one loaded index, never serializes the entire tree, and rejects snapshots whose exclusion coverage no longer satisfies the current policy.

Connections can query saved snapshots from the desktop or another connection only when the current native root identity and exclusion coverage satisfy their own policy. The service checks small scope metadata before loading the index. Broader captures are rejected as a whole, not silently filtered into misleading totals. A snapshot ID grants no authority. Jobs remain private to their initiating connection.

`voldiq_history` and CLI `history [offset] [limit]` return at most 100 authorized snapshots per page. Offsets count visible captures. One SQLite read view keeps multi-page selection consistent during concurrent captures. A request examines at most 10,000 metadata rows; `limited: true` explicitly marks incomplete discovery. `next_offset` refers only to a known next authorized page. Matching `root_key` values identify the same stored native root and exact exclusions; do not infer growth from captures with incompatible coverage. Current authorization requires that the native root identity can still be verified; disconnected or replaced roots fail closed.

Agent scan results persist in the existing compact snapshot database. Desktop scans, duplicate verification, cleanup, restoration and snapshot work now use the same core OS operation lease as headless scans. A native guard retains it until the worker exits, including after cancellation or local completion acknowledgement. The existing `scan.lock` filename is retained for compatibility with earlier scan clients; it is not approval proof. Durable job history retains at most 10,000 agent scan records. GUI job mirroring and automatic shared-index persistence remain pending. The job table is not the file-operation security audit ledger.

CLI commands are listed by `voldiq-cli --help`. JSON envelopes have `schema_version: 1`. Exit codes are 0 success, 2 invalid arguments, 3 missing/invalid credential, 4 unavailable/failed operation, 5 independent human approval required and 130 cancelled foreground scan. CLI scans remain in the foreground; `--wait` is accepted explicitly. Initial job identity goes to stderr, final JSON to stdout. Ctrl+C requests cancellation and waits for the worker to finish. MCP reserves stdout entirely for the protocol.

CLI `folder <snapshot_id> <node_id> [--note]` and MCP `voldiq_folder_metadata` with `include_note` omitted or false never read note text. Their `not_requested` value does not mean an empty note. Requesting text requires `ReadContent`; the human-only connection screen grants it explicitly. Notes remain literal untrusted text and cannot grant permissions or approve work. Reads verify the current native directory against scan evidence, so replacement or missing legacy evidence requires a rescan. A returned note is current; `source_snapshot` identifies folder evidence rather than a historical note. See [folder metadata](FOLDER-METADATA.md).

## Folder budget analysis

CLI `budget <snapshot_id> <node_id>` and MCP `voldiq_folder_budget` with those same two fields read the shared core report for an indexed directory. Both require current Metadata permission, native root/folder identity and compatible exclusion coverage; permission is rechecked before returning the report. The saved limit is current metadata, while `source_snapshot` and `observation` identify the usage evidence. Byte counts and limits are exact decimal strings. A missing budget is null, and incomplete/provisional totals remain lower bounds. These calls never read notes, hydrate placeholders or modify limits or files. Saving and removing limits are human GUI actions only, with a strict native identity pin, operation exclusion and optimistic revision checks.

Snapshot schema 7 adds `observation`: actual core completion time, completeness, engine version and accounting revision. Snapshot `captured` remains save time. Legacy records retain unknown observations; reopening or saving an index does not create a new measurement. Forecasts examine at most 30 measurement days using bounded metadata queries, matching native root/folder identity, exact exclusions and accounting revision. At least six distinct days spanning a week are required, with no gap over seven days. Incomplete, stale, incompatible, decreasing or highly variable history has an explicit unavailable reason. An estimate's low/high interval-rate range is a heuristic, not a statistical confidence interval or a volume-full date; its slow bound is limited to 366 days.

Image-file equivalents use at most 128 positive-size indexed image-extension matches ordered by name in that folder scope. The response discloses sample count, total matching count, mean logical bytes and approximate equivalent count. This metadata sample is not random or a content classifier, and the equivalent is unavailable for incomplete coverage or an empty sample. It never assumes a generic image size or claims content is locally resident.

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

Verification status: transport, eleven analysis tools and six plan tools are implemented. Real CLI and official SDK clients check shared budget reads, exact-hash execution/cancellation intents, absence of a human channel, rejected approval injection, replay and unchanged owned source files. A core dispatch fixture exercises independent human approval, helper refusal, persisted result and closed-session denial. Native provisioning, review, event presentation and the full elevated/Shell route await manual acceptance. User clients are not configured.
