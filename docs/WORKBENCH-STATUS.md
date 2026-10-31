# Storage workbench status

Task branch: `codex/storage-workbench`, starting at `fae47a0` (also the fetched `origin/main` at audit time).

The complete task remains active. See the [coverage matrix](WORKBENCH-COVERAGE.md) and [execution plan](plans/storage-workbench.md). This status does not declare pending capabilities implemented.

## Audit

- Current application: Rust/Tauri 2, Svelte 5, TypeScript, SQLite, BLAKE3 and custom Canvas, version 0.2.0.
- Existing subsystems: incremental NTFS/directory scan, bounded native queries, volume metrics, logical/unique allocation, exclusions, exact duplicates, developer-cache evidence, native cleanup preview/execution/recycling/restoration, compact snapshot history, treemap and Skyline.
- Missing product adapters: CLI and MCP. The desktop currently owns operation coordination; it must become shared without bypassing index ownership.
- No repository or ancestor `AGENTS.md` existed at baseline. The new file retains the user's instructions and adds task safety invariants.
- Pre-existing untracked paths `interface/prompts.json` and `output/` are preserved and excluded from task commits.
- `PRODUCT.md` records the same no-subagent/no-computer-use preference stated in this chat. No new UI capture, interaction or scaling acceptance is claimed. Existing release artifacts and `DESIGN.md` record the baseline identity.
- `docs/ARCHITECTURE.md` and `docs/SECURITY.md` contain historical statements that recycling is unavailable; current source and `docs/ACCEPTANCE.md` record later activation. Audit does not treat old prose as a guarantee of current behavior.

## Verification

The initial sandboxed source gate passed formatting, Svelte check (zero errors/warnings), 20 frontend tests and the production frontend build. The native Rust fixture failed in the pre-existing pnpm fallback-path assertion (`pnpm_config.rs:384`). The sandbox temporary path contains `sandbox.{UUID}`; `expand_path` stops at the first closing brace and rejects the opening brace in that fallback. No task product code had changed. A repeat outside sandbox redirection is running to distinguish baseline behavior from that environment-specific case; Clippy is still pending.

The user supplied five visual references during the audit: treemap controls/export menu, app scatter/list, trends, split history comparison and operational dashboard. They guide information hierarchy and continuity within Voldiq's existing tokens, not TreeMap branding, macOS controls or synthetic values.

Run the gate with `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify_project.ps1`. The process-scoped option permits this existing script without changing the machine's execution policy.

## Next executable block

Finish the baseline gate and audit commit, then implement core authorization with real temporary-directory fixtures. No agent tools or additional destructive entry points are exposed before permission and plan checks exist.
