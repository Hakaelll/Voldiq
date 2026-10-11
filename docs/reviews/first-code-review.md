# First delivery code review

What this change does: The application detects Windows volumes and performs real read-only scans. It publishes a Rust-owned index to bounded queries and provides a Svelte explorer, Canvas map, search and settings.

Assumed load: one desktop user, with a future requirement for millions of files. The current measured fixture contains 10,000 files. The review traced scanning, cancellation, index queries, path reconstruction, metadata handles, Tauri commands and their frontend callers.

## Resolved before delivery

- Drive-type constants originally came from the wrong Windows module and acted as pattern bindings. The correct Windows constants now classify local, removable, network and optical drives.
- Canonicalizing a reparse-point root hid its original link status. The scanner now checks the requested root before canonicalizing it, and the junction-loop fixture tests rejection.
- A new scan could show results from the previous index under the new path. The interface now clears visible results when a scan starts.
- Late detail and directory responses could restore stale data after a scan or navigation change. Request generations now discard those results and their errors.
- The terminal scan event could race index publication. The adapter installs the completed index and clears the active slot before sending terminal progress.
- Frontend diagnostics and all Clippy findings were corrected. File names and paths cross IPC as data; Explorer invocation never uses a command interpreter.

## Required next checks

1. **Populated native interactions need verification.** Browser captures cover empty states and preferences. The executable launches a real Windows window, but scan-to-map, virtual scrolling, search and reveal still need an end-to-end native interaction pass. Until that pass runs, V04 remains IN_PROGRESS.
2. **Partial queries are implemented.** A shared Rust index is published before progress, and insertion propagates ancestor totals. The live-read test confirms consistent totals and cancellation before completion. Large-query lock contention and calibrated concurrency remain open.
3. **Large-index measurements remain open.** Queries currently filter and sort an in-memory vector. The 10,000-file baseline is fast, but it cannot establish acceptable behavior at a million entries. V10 tracks that workload and any resulting query changes.
4. **The product acceptance gate is still open.** NTFS, duplicates, cleanup, snapshots and release validation remain TODO. The interface does not expose simulated versions of them.

Verdict: the initial scan foundation is ready to commit. The application is not ready for a complete-product release.

Not checked: destructive operations, NTFS journal behavior, populated native UI and final distribution, because those are pending implementation or validation.
