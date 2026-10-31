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

## Delivery

Implement the staged [workbench plan](docs/plans/storage-workbench.md), update [coverage](docs/WORKBENCH-COVERAGE.md), and keep the application buildable. Preserve pre-existing changes. The user explicitly requests all changes on `main` and removal of the other branches after their commits are preserved there. Continue directly on `main`; stage explicit paths, verify each unit, commit and push. Do not create additional branches or mark the full scope complete while coverage has pending rows.

Run relevant behavior tests, then `scripts/verify_project.ps1` for integration. Compilation does not prove visual acceptance or device-specific performance. Record actual evidence and limitations in `docs/WORKBENCH-STATUS.md`.
