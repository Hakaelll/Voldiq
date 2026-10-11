# First repository audit

What this repo does: Voldiq is a Windows storage analyzer with a Rust engine and a Svelte interface in Tauri. This audit assumes one local user and the specification's eventual million-file workload.

The audit read the entry points, build configuration, dependency manifests, scanner, Windows metadata boundary, index operations, frontend command callers, fixtures, README and delivery ledger. It checked cancellation ownership, handle closure, scan publication order, IPC limits and path handling. No destructive file command is exposed.

## Must fix before complete-product release

1. **Working scan results are not queryable yet** (`crates/voldiq-core/src/scanner.rs`, `src-tauri/src/main.rs`). A long scan reports counts, but the index reaches the interface only after completion or cancellation. Add a bounded partial-query publication mechanism and test the real command flow. Otherwise the requested partial explorer workflow remains unavailable.

2. **The main native interaction flow is not validated yet** (`frontend/App.svelte`, `src-tauri/src/main.rs`). The executable launches, engine fixtures pass and the browser shows initial states. A real scan must still be driven through the native UI to check map selection, navigation, scrolling, search and Explorer reveal. Otherwise a command or state bug could escape the engine-only tests.

## Should fix in the next engine phase

3. **Large-index queries need workload evidence** (`crates/voldiq-core/src/index.rs`). Each request filters the vector, and scoped membership walks parent links. A directory page on a million-entry index still examines a million entries, while deep scoped searches multiply that work by depth. Measure a million-entry fixture, then add direct child lookup and subtree membership data if the measured delay exceeds the query budget.

4. **Old query responses need scan-generation protection** (`frontend/App.svelte`). A folder-details request already in flight can finish after a new scan starts and repopulate old details. Increment the query generation when scans and directory scopes change, and discard responses from earlier generations. Otherwise old paths can briefly appear beside the new scan state.

Verdict: keep the first delivery labelled as development. Resolve the native flow and partial publication before closing the scanner/interface acceptance gate.

Not checked: NTFS fast enumeration, cleanup, duplicate hashing, snapshots and installer behavior. They are pending tasks, not hidden working implementations.
