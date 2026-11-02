# Filesystem safety

Scanning is read-only and uses the user's current Windows permissions. Voldiq does not elevate the main application or install a resident service.

## Agent boundary

The workbench CLI/MCP adapter supports authenticated analysis and exact recycling-plan lifecycle requests. Every connection has an OS-random individual credential, explicit capabilities, verified allowed roots, exclusions and limits. Only credential digests are persisted. Existing clients lose access on revocation or rotation. Windows metadata authorization pins ancestors, rejects reparse traversal and compares native identity; query totals come only from an authorized snapshot with sufficient exclusion coverage.

Provisioning, rotation and plan approval require separate desktop/native human confirmation. Agents may prepare immutable manifests, request approval and submit exact execution/cancellation intents. Desktop dispatch requires the current, expiring, single-use human grant bound to the manifest and principal, plus fresh scope and native evidence checks. No agent transport exposes provisioning, approval, arbitrary shell execution, permanent deletion, bin purge or history-retention removal. Complete elevated agent/Shell acceptance remains manual; transport and core fixtures do not establish that route as verified. See [the plan boundary](AGENT-PLANS.md).

Stdio sends metadata to the local client, which may send it to an AI provider. These checks govern operations through Voldiq and do not sandbox a process with independent same-account filesystem access. Local state is not claimed tamper-proof. See [configuration, limits and tested boundaries](MCP.md).

## Existing desktop guarantees

Reparse points are not traversed. The scanner rejects a reparse-point scan root and records child reparse points as incomplete coverage. Metadata handles request file attributes, allow normal sharing and use `FILE_FLAG_OPEN_REPARSE_POINT`.

Duplicate analysis rejects offline and recall-marked files before opening content and rechecks the opened handle. Windows content handles also use `FILE_FLAG_OPEN_NO_RECALL`, following the [native open flag contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew). A controlled fixture sets the offline attribute on two files and verifies zero content bytes read and no duplicate group. This does not certify every third-party cloud provider.

Physical allocation comes from Windows file information. If a handle cannot be opened or its metadata cannot be read, the error is reported and the result becomes incomplete.

The webview loads local packaged resources. Content Security Policy excludes remote scripts, frames and objects. The command bridge owns the filesystem paths; the interface cannot execute arbitrary filesystem or shell commands.

Advanced permanent removal is available through an explicitly reviewed backend-owned plan and an exact `DELETE` confirmation. The plan belongs to the current live index and can be consumed only once. Execution validates scanned identity/change-time evidence, exclusions and protected locations, pins ancestor names and retains verified leaf handles through removal. Duplicate removal also retains a verified copy of each selected group. A durable journal records per-item intent and results; cancellation or a journal failure stops further removal, and interrupted in-flight items remain unknown rather than being retried automatically. The native command invalidates the loaded analysis before execution so stale totals cannot remain authoritative.

Default Windows recycling and Undo remain unimplemented. Permanent removal cannot be undone. The retained automated suite checks selected confirmation, replacement, read-only target, ancestor-handle, cancellation and journal-failure boundaries with disposable files; it is not a complete filesystem security audit. See [cleanup scope and guarantees](CLEANUP.md) and [current test coverage](TESTING.md). Destructive verification must use controlled fixtures.

Explorer reveal uses a fixed executable with one argument derived from the index. It does not pass user text through a command interpreter.

Persistent duplicate hashes remain read-only analysis evidence. Snapshot schema 5 stores bounded fixed-size identity, metadata and digest fields without paths or content. Every reuse opens the ordinary file and compares identity, length, exact modification time and native change time while retaining the existing content-handle sharing restrictions. The decoder rejects incompatible SQLite types and lengths before copying fields. Cache persistence does not grant cleanup authority; duplicate cleanup still performs fresh content and retained-copy validation.
