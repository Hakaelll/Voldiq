# Voldiq working rules

Preserve the current Rust core, Tauri 2, Svelte 5 and TypeScript stack. `PRODUCT.md` and `DESIGN.md` are the canonical product and visual references. Reuse existing components, tokens and artwork. The user's UI workflow applies; do not create a competing design system.

The user requests no subagents and no computer use. Use source checks and disposable filesystem fixtures. Visual acceptance, keyboard interaction and Windows scaling remain manual unless the user changes this instruction.

## Data safety

- GUI, CLI and MCP use `voldiq-core`; adapters do not implement separate scanners or destructive executors.
- Unknown allocation, dates, coverage or recovery are not zero or guarantees. Separate logical path totals, unique allocation and observed volume free space.
- Do not follow reparse points or hydrate cloud placeholders. Authorize native paths and identities, never string prefixes. Scope every result and aggregate.
- Agent credentials come from a human-controlled channel. Client names, announced roots and scan IDs do not grant permission. Do not commit credentials or log them.
- Agents may prepare exact immutable plans and request approval. Approval is human-only, bound to the manifest and principal, expiring and consumed once. No `--yes`, agent-callable approval, shell execution, permanent deletion or bin purge in MCP.
- Revalidate identities, permissions, protections and destination before execution. Changed evidence invalidates approval. Retry and crash recovery must not repeat an uncertain destructive operation.
- Keep protected user paths, system paths, Voldiq state, credentials, bin and recovery storage outside suggestions and execution.
- Tests that modify files use owned temporary fixtures only. Never clean real user data as verification.
- The user authorizes cache cleanup only for Voldiq build and test artifacts. Preserve active builds, running previews, application state and caches of Windows or other applications.

## Delivery

Implement the staged [workbench plan](docs/plans/storage-workbench.md), update [coverage](docs/WORKBENCH-COVERAGE.md), and keep the application buildable. Preserve pre-existing changes. The user explicitly requests all changes on `main` and removal of the other branches after their commits are preserved there. Continue directly on `main`; stage explicit paths, verify each unit, commit and push. Do not create additional branches or mark the full scope complete while coverage has pending rows.

Run relevant behavior tests, then `scripts/verify_project.ps1` for integration. Compilation does not prove visual acceptance or device-specific performance. Record actual evidence and limitations in `docs/WORKBENCH-STATUS.md`.

Keep the delivered footprint small. Once source verification, native preview and packaging for a unit have finished, use `scripts/clean_build_cache.ps1` and its `-Apply` mode to remove inactive release build/test artifacts while preserving verified deliverables and running previews. Do not clean an active build or regenerate caches merely to repeat checks that already passed. The default installer embeds the small WebView2 bootstrapper; the full offline runtime is an explicit configuration override.

The user reduced the routine suite to about eight meaningful behavior tests, roughly 5% of the former 164. Keep it small as work continues. Prefer a relevant existing flow or replace a lower-value case when coverage is needed; avoid helper/cosmetic tests and do not disguise a larger suite with ignored tests or grouped wrappers. Preserve runtime safety checks and the formatting, type, build and Clippy gates.
