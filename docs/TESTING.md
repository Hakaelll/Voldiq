# Testing

The automated suite contains 8 tests, reduced from 178 on 2026-10-21 at the project owner's request. This retains 4.5% of the original test cases. The other 170 cases and their unused fixtures and modules were deleted.

## Automated checks

Run on Windows with the Rust MSVC toolchain, C++ Build Tools and project dependencies installed:

```powershell
./scripts/verify_project.ps1
```

The script checks Rust formatting, Svelte and TypeScript diagnostics and the frontend test. It then builds frontend assets before running the production-feature workspace Rust tests and strict Clippy. It reuses release artifacts and offline Cargo dependencies. On a fresh checkout, run `cargo fetch --locked` once while online to populate the dependency cache. The gate stops on failure and reports success only after all six stages pass; its report records the profile and script hash.

The [2026-10-31 release verification](../benchmarks/project-verification-20261031-103904-700.json) passed all six stages on the working tree based on `b78f022`: one frontend test and seven Rust tests, with zero Svelte diagnostics or Clippy warnings. The production frontend build also passed. This verifies the retained source gate, not the remaining native, UAC or installer acceptance requirements.

For individual checks:

```powershell
pnpm test
cargo test --offline --release --workspace --features voldiq/production
pnpm check
cargo clippy --offline --release --workspace --all-targets --features voldiq/production -- -D warnings
pnpm build
```

## Retained coverage

| Cases | Behavior |
|---|---|
| 1 native command test | The native production mode and embedded HTML/JavaScript/CSS must validate. Permanent and recycling actions require distinct exact confirmations and the same live index; each plan can be consumed only once. Cancelled snapshot publication retains the existing identity and Arc; subsequent publication can succeed. It rejects a stale publication identity even when a second real index reuses the same node ID, and checks canonical/transitional/legacy Docker accounting, known zero and unknown sizes. It does not move files or launch UAC. |
| 2 cleanup execution tests | Changed retained duplicate copies prevent deletion; cancellation preserves pending files; journal write failure stops further removal and preserves the partial report. |
| 1 cleanup journal test | Interrupted deletion preserves replacements and unrecorded outcomes. Fresh historical restoration checks parent identity, collision, source locking, session ownership, intent persistence and one-use completion with owned ordinary files. |
| 1 file disposition test | Replaced files and read-only targets are not removed. |
| 1 Windows handle test | An acquired parent directory cannot be renamed until its handle is released. |
| 1 snapshot integration test | Cancelled capture preserves saved history, and cancelled loading short-circuits before querying. Saved history survives reopening and schema-4 migration, compares actual file changes and preserves search results. Persistent duplicate hashes survive reopening, reject changed files and malformed rows, and retain the previous cache after a cancelled replacement. |
| 1 frontend selection test | Selection limits reject a whole page without partial changes. Navigation refreshes coalesce without releasing a newer owner. Indexed IPC captures the originating identity and rejects late replies after replacement. |

Rust fixtures use disposable temporary folders. The Windows cleanup tests remove only files they create. The frontend test runs directly with Node's test runner and requires no browser automation.

These cases cover selected data-safety boundaries. Broad scanner, query, resolver, treemap, virtual-list and migration coverage is no longer part of the automated suite. Existing benchmark and verification records describe historical runs, not the current test inventory.

## Manual application checks

```powershell
pnpm tauri dev
```

Choose a disposable fixture folder. Check scan progress, capacity and size totals, folder navigation, search, file filters, details and Explorer reveal. Check duplicate review, cleanup confirmation and snapshot comparison with known fixture contents. Cancel an active scan and verify the incomplete state. Check inaccessible folders, keyboard navigation, window resizing, hover navigation, both themes and reduced motion.

The browser preview has no filesystem bridge. Native interaction, performance targets and installed-app acceptance remain separate checks in [Product acceptance](ACCEPTANCE.md). Packaging evidence is in [Windows distribution](DISTRIBUTION.md).

The standalone [historical restoration fixture](../benchmarks/run_historical_restore_probe.ps1) verifies ordinary Shell recycling, release/reacquisition of actual bin authority, no-overwrite restoration and durable replay refusal with an owned four-byte file. It adds no declared test and does not exercise protected admission or desktop commands. The [completed receipt](../benchmarks/historical-bin-restore-20261031-112253-343.json) includes source/image hashes and scope.

The [desktop restoration source gate](../benchmarks/project-verification-restoration-20261031.json) passed all six stages on the working tree based on `110e452`: eight retained tests, strict workspace Clippy, Svelte diagnostics and the production frontend build. The activity regression checks that only versioned recycled receipts are eligible and restored receipts are not. Native modal and lease acceptance remain manual.

The [prepared recycling source gate](../benchmarks/project-verification-recycling-integration-20261031.json) passes all six stages and retains eight tests. Native recycling execution remains blocked by its verification gate while the user defers the combined UAC fixture.

The existing frontend case also checks live-navigation refresh ownership: progress refreshes defer and coalesce, old completions cannot release a newer navigation, and invalidation discards abandoned ownership. An unconditional-refresh counterexample fails the case; the verified implementation passes. No declared test was added. See [live navigation verification](../benchmarks/live-navigation-refresh-20261031.json).

The [index ownership source gate](../benchmarks/project-verification-index-ownership-20261031.json) passes all six stages with eight declared tests. The retained native case checks publication replacement, stable identity for the same Arc, clearing and reinstalling. The frontend case uses a controlled Tauri transport reply. Disabling either native identity matching or frontend late-reply rejection makes the corresponding case fail. See [counterexample evidence](../benchmarks/index-ownership-20261031.json). Native scan/snapshot interaction remains for manual acceptance.

The retained snapshot case also checks persistent duplicate hashes with two ordinary 16 KiB files. It verifies schema-4 migration without losing snapshots, exact BLAKE3 groups, zero content reads after reopening, re-reading one malformed record, same-size edits with preserved modification time, cancelled replacement rollback and explicit cache clearing. Omitting native change-time validation makes the existing case fail with a false duplicate group. This is correctness and byte-accounting evidence for the fixture, not a measured application speedup. See [persistent hash verification](../benchmarks/persistent-hashes-20261031.json).

The final [persistent-hash source gate](../benchmarks/project-verification-persistent-hashes-20261031.json) passes formatting, frontend diagnostics, the one frontend case, all seven Rust cases, strict all-target Clippy and production frontend build. The native adapter's cache-failure fallback is source reviewed rather than a proven desktop interaction.

The retained frontend case now checks literal map zoom geometry, same-scope and absent-anchor fallback, 160 ms termination, cancellation of frame ID zero, idle no-op stopping and stale callback rejection using a deterministic frame clock. Removing the callback-owner guard makes the case fail. This proves the scheduler and geometry, not rendered motion or a 60 FPS target. Pointer origin is captured by the owning navigation request; keyboard actions keep their immediate path. No declared test was added.

The corrected [standalone source gate](../benchmarks/project-verification-standalone-ui-20261031.json) passes all six stages with `voldiq/production` and eight declared tests. A real no-feature desktop check fails at the compile-time guard with exit 101. The [executable probe](../scripts/read_embedded_ui.ps1) validates decoded production assets before preview or distribution acceptance. [Standalone evidence](../benchmarks/standalone-ui-fix-20261031.json) and [motion evidence](../benchmarks/treemap-motion-20261031.json) retain the scope and counterexamples. No visual acceptance is inferred from this gate.

Snapshot cancellation extends the retained native and snapshot cases. The snapshot case verifies cancelled capture without a new record, pre-cancelled loading with both valid and absent IDs, and successful loads afterward. The native case verifies unchanged publication identity/Arc after cancellation and replacement on a later successful load. Ignoring the load flag makes the retained snapshot case fail. Channel acknowledgement, Cancel focus and in-flight SQLite/mutex latency still require native interaction acceptance. See [snapshot cancellation evidence](../benchmarks/snapshot-cancellation-20261031.json).

The [snapshot cancellation source gate](../benchmarks/project-verification-snapshot-cancellation-20261031.json) passes all six production stages with the unchanged eight-test inventory. The Core load signature change is also compiled through the snapshot benchmark example by all-target Clippy.
