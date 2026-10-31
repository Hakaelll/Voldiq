# Storage workbench implementation plan

**Goal:** Implement the supplied [requirements](../reference/storage-workbench-requirements.txt) without replacing Voldiq's identity or weakening its filesystem guarantees.

**Architecture:** Keep `voldiq-core` as the shared scanner, index, history and executor. Add authorization, durable jobs and immutable agent plans there. A separate Rust headless binary uses that core and the official MCP Rust SDK over stdio; it does not depend on Tauri or a production Node runtime. Desktop provisioning and approval remain outside agent tools.

**Stack:** Rust 2024, SQLite, BLAKE3, Tauri 2, Svelte 5, TypeScript, custom Canvas 2D. `PRODUCT.md` and `DESIGN.md` remain canonical.

Execution is inline, without subagents or computer use, as the user requested. The detailed user specification already selects the architecture and A–H order. Routine implementation choices are recorded here and in the status file without additional approval gates.

## Constraints and review focus

- Preserve untracked `interface/prompts.json` and `output/`.
- Agent scope applies to queries, errors, aggregates, exports, history and operations, not only scans.
- Directory rename/reparse races and Windows aliases must fail closed.
- Disconnects, competing clients, crashes and retries must never repeat uncertain changes.
- Incomplete scans and incompatible history must not imply deletions or fabricated forecasts.
- Optional services must remain off until configured, with actual capability states.

## A. Establish the baseline

- [x] Read the complete prompt, source instructions, product/design documents and current architecture.
- [x] Pin and inspect TreeMap source without running its code or installing its dependencies.
- [x] Create coverage for every requested product area, distinguishing source evidence from current verification.
- [x] Run the existing six-stage source gate and record results. Visual baseline is the existing release and source tokens; new captures are excluded by the user's instruction.
- [x] Commit and push the audit on `codex/storage-workbench` (`ff38f66`).

## B. Shared model, authorization and jobs

- [x] Add `crates/voldiq-core/src/agents.rs` and focused fixtures for explicit capabilities, canonical root authorization, exclusions, revocation, bounded policies and non-disclosing errors. Actual query/result filtering remains in the shared-service task.
- [x] Add durable SQLite agent/job state with transactional migrations, human provisioning, credential rotation and a single cross-process scan lease. Reuse the existing snapshot store for indexes.
- [ ] Extract reusable operation coordination from desktop ownership without dropping current `IndexId` guarantees. Add provenance, cloud state and unknown metric contracts where the existing index lacks them.
- [ ] Verify wrong/revoked credentials, sibling paths, links, overlapping roots, concurrency, cancellation and restart. Commit and push.

## C. Plans, headless and MCP

- [ ] Extend the existing cleanup preview/executor with immutable serializable manifests and explicit lifecycle states. Persist hash, principal, exact identities, policy version, expiry and audit. No agent approval endpoint.
- [ ] Add a separate headless crate and CLI for scan/query/stats/history/plan/jobs/export, with JSON envelopes, structured errors and a read-only default.
  Scan/query/stats/details/jobs/cancel and versioned errors are implemented and subprocess-tested. History/plan/export remain open.
- [ ] Add official `rmcp` stdio transport, bounded schemas/results and tools that route through authorization and shared services. Verify protocol behavior with the SDK client against the real executable.
  Transport plus seven analysis tools are implemented and verified with `rmcp` 3.8.0's real client. The broader catalogue remains open.
- [ ] Add desktop connection management, separate human approval and revocation. Unavailable independent approval returns `needs_human_approval` without changing user data.
- [ ] Test mutation, expiry, replay, concurrent execution and restart in temporary fixtures. Document generic stdio and current Codex configuration. Commit and push.

## D. Dashboard and explorer

- [ ] Extend existing summary/details and shared map state; add Sunburst, area-correct circle packing, bounded approximate Voronoi, virtual grid and advanced treemap selection/lens controls.
- [ ] Preserve the existing treemap/Skyline geometry and selection. Complete depth, metric, color legends and cross-view context without rescans.
- [ ] Verify byte conservation, layout limits, picking and shared selection through pure behavior fixtures; leave visual/scaling acceptance manual. Commit and push.

## E. Search, applications and assisted cleanup

- [ ] Add one strict core search parser and deterministic offline translation; connect GUI, CLI, MCP, saved views and policies.
- [ ] Add notes/protections, budgets and favorites to shared persistence. User-only protection removal.
- [ ] Add evidence-based app/game/media attribution, shared/unattributed buckets, review scoring and common cleanup tray. Preserve existing verified developer rules.
- [ ] Extend duplicate review with perceptual image comparison, explicit similarity and manual selection; do not weaken exact duplicate retention.
- [ ] Verify parser failures, protection precedence, accounting and unsafe suggestions. Commit and push.

## F. History and diagnosis

- [ ] Extend compact existing snapshots with engine/coverage provenance and retention, compatible comparisons, bounded forecast, calendar, diary and playback.
- [ ] Add grouped Live watchers with overflow resynchronization and explicit observed net growth.
- [ ] Add measured space reconciliation, unknown residuals, Windows-specific diagnostics and read-only recovery discovery.
- [ ] Verify changed coverage, insufficient history, lost events and unsupported access. Commit and push.

## G. Transfer, recovery and supervised policies

- [ ] Implement exact transfer and restore manifests, stable destination volume identity, no-overwrite copy/readback verification and approved original handling.
- [ ] Add bounded recovery catalogue, quotas, retention and collision recovery. No agent purge.
- [ ] Add scheduled authorized scans and prepare-only policies with limits/cooldowns/history; each modifying batch needs a new human approval.
- [ ] Verify partial copies, destination loss, changed originals, capacity, collisions and interrupted execution. Commit and push.

## H. Optional integrations and finish

- [ ] Add bounded archive inspection, provenance, safe previews, Git diagnostics, optional SMART/cost/video capabilities and a lightweight VS Code adapter.
- [ ] Add opt-in cloud metadata connectors and authenticated LAN summaries with revocation; do not expose internal IPC or deletion over LAN.
- [ ] Add protected CSV/XLSX/PDF and supported graphic/history exports lazily, with scope/coverage metadata and cancellation.
- [ ] Add optional desktop integration, onboarding/help and portable/ephemeral preferences without silent activation.
- [ ] Run final source/behavior gates and real synthetic/headless measurements. Record hardware-dependent and manual acceptance limits. Review the whole diff inline, update all coverage rows, commit and push.

## Decisions

- A separate headless binary avoids loading Tauri/WebView. Its executable name will be explicit in documentation rather than silently replacing the existing desktop executable.
- No TreeMap code is copied. Its services are research references, not security guarantees for Voldiq.
- Existing acceptance files refer to older releases and sometimes contradict newer source. This work uses fresh checks and keeps historical evidence intact.
