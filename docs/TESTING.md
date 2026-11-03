# Testing

The routine Windows suite contains eight behavior tests: one frontend case and seven Rust cases. The owner requested this reduction on 2026-11-02, leaving about 4.9% of the former 164 tests. The other 156 cases and their unused fixture helpers were removed. They are not ignored, filtered out or combined into replacement wrappers. Runtime validation and file-operation safeguards remain in the application.

## Retained inventory

| Case | Existing source | Behavior |
|---|---|---|
| Frontend interaction | `frontend/lib/cleanup-selection.test.ts` | Atomic selection limits, navigation ownership, stale indexed IPC replies, bounded/interrupted map motion, Live lifecycle, CSV/XLSX format dispatch, report scope/paging separation, final receipt, late progress and disposal/cancellation ownership |
| Native authorization | `crates/voldiq-core/tests/agents.rs` | Allowed file identity, sibling and excluded-path denial, renamed exclusions, pinned directories and replaced roots |
| Saved storage evidence | `crates/voldiq-core/tests/snapshots.rs` | Reopened/cancelled snapshots, migration, playback/comparison/context, ranked remainders, coordinated folder pages, qualified budgets, duplicate hashes, reviewed retention, UTC/null/epoch/placeholder cohorts, retained-capture diary, Live lifecycle, quota/whole-volume counters, signed indexed reconciliation, VSS discovery/date/root evidence, complete CSV streaming and Windows publication/collision/cancellation/owned-temp/protection behavior and XLSX ZIP/XML/literal/integer/unknown/large-record evidence |
| Duplicate execution | `crates/voldiq-core/src/cleanup/execute/tests.rs` | A changed retained copy prevents deletion; cancellation preserves pending files and retained copies |
| Interrupted result recording | `crates/voldiq-core/src/cleanup/execute/tests.rs` | A journal write failure stops further removal and retains explicit partial/unknown outcomes |
| Human-approved dispatch | `crates/voldiq-core/src/plans/desktop/tests.rs` | Exact live grant, wrong-hash denial, helper refusal, persisted result, replay without another attempt and closed-session denial |
| CLI transport | `crates/voldiq-headless/tests/headless.rs` | Real binary, scoped exact-plan/space/budget/comparison reads, withheld volume context, reversed differences, foreign-scope denial, independent approval requests, cancellation and rejected approval bypasses |
| MCP transport | `crates/voldiq-headless/tests/mcp.rs` | Official SDK stdio negotiation, real scan/query/history/space/budget/comparison calls, withheld volume context, both-source authorization, exclusion coverage, page caps, strict arguments, exact plan intents and revocation |

These are retained existing cases. They do not preserve the former suite's full parser, geometry, race, migration, protection, attribution or metadata edge-case coverage. Earlier green receipts remain historical evidence, not proof that deleted cases still run. See [workbench status](WORKBENCH-STATUS.md) and the [coverage matrix](WORKBENCH-COVERAGE.md).

Rust fixtures modify only their own disposable temporary folders. The two execution cases may remove files they created; they never clean user data. The desktop dispatch case refuses its helper without launching UAC or moving source files. Frontend tests use Node without a browser. The full Windows inventory includes the Windows-only desktop dispatch case.

The saved-storage flow also checks ranked report preparation over 10,005 matching records: stable top-50 ordering, explicit omissions, exact observed file-path totals beyond u64, independent unknown/partial/alias counters, cancellation and a nonnumeric overlapping-folder state. On Windows it writes a synthetic PDF through the safe destination owner, using mixed Latin/Arabic text and a long unbroken name, and checks pre-cancellation and cancellation without publication. Independent PDF parsing/rendering is recorded separately in workbench status. Developer opt-in `VOLDIQ_KEEP_PDF_FIXTURE=1` retains only that owned temporary PDF directory for this review; normal runs remove it.

The retained frontend flow also paints an actual treemap and lens through the SVG target, checking literal rectangle dimensions, escaped names, local clips/full-circle arcs, absence of embedded images/active markup and rejection of external paint URLs. The developer-only `scripts/render_svg_fixture.mjs` produces all five map representations through both Canvas and SVG using the bundled Canvas runtime, without adding routine test declarations or production packages. Independent XML inspection and rendered pair review remain separate evidence.
## Run the source gate

Use the Rust MSVC toolchain, C++ Build Tools and installed project dependencies:

```powershell
./scripts/verify_project.ps1
```

The unchanged script runs all remaining cases, with no test-name filters. Its six stages are Rust formatting, Svelte/TypeScript diagnostics, frontend behavior, production frontend build, production-feature workspace Rust tests and strict Clippy. It retains the release profile and offline Cargo dependencies. A fresh dependency cache requires `cargo fetch --locked` while online.

The desktop binary contains no remaining test declarations. Its Cargo target disables only the empty binary test harness; no existing case is skipped or ignored. Core and real CLI/MCP tests still run, while strict Clippy checks the native adapter and production native builds remain a separate delivery check. A new desktop unit-test case would require enabling that target's harness again. No build-time speedup is claimed.

Individual behavior checks:

```powershell
pnpm test
cargo test --offline --release --workspace --features voldiq/production
```

[The reduced-suite receipt](../benchmarks/workbench-eight-tests-source-20261102.json) records the integration gate. Source checks do not establish native rendering, keyboard/focus, Windows scaling, device performance or installer acceptance. The user's no-computer-use instruction leaves those checks manual.

Keep the routine suite around eight meaningful cases as development continues. Add assertions only to a relevant retained flow or replace a less useful case when another needs coverage. Avoid tests for presentation helpers and do not restore a large suite behind one test declaration.

## Native and historical evidence

The actual application preview and its exact embedded-asset/startup evidence are recorded in [workbench status](WORKBENCH-STATUS.md). Existing windows and in-use build artifacts are preserved during cache cleanup. Build/test cleanup never includes application state, Windows caches or other applications.

Optional owned-profile lifecycle diagnostics remain available separately:

```powershell
cargo build --offline --release -p voldiq --features production --example window_lifecycle_probe
./scripts/probe_window_lifecycle.ps1
./scripts/probe_window_lifecycle.ps1 -ReturningLoop
```

These diagnostics observe an isolated hidden window and its process; they are not additional declared tests or proof of populated application behavior. A timeout preserves the process for observation rather than killing it. Do not run browser/computer-use automation under the current user instruction.

Historical release verification, packaging, shutdown observations and the older unresolved process incident retain their original scope in [acceptance](ACCEPTANCE.md), [distribution](DISTRIBUTION.md), [performance](PERFORMANCE.md) and [workbench status](WORKBENCH-STATUS.md). They do not establish new visual, privileged or hardware acceptance.
