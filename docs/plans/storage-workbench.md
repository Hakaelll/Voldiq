# Storage workbench implementation plan

**Goal:** Implement the supplied [requirements](../reference/storage-workbench-requirements.txt) without replacing Voldiq's identity or weakening its filesystem guarantees.

**Architecture:** Keep `voldiq-core` as the shared scanner, index, history and executor. Add authorization, durable jobs and immutable agent plans there. A separate Rust headless binary uses that core and the official MCP Rust SDK over stdio; it does not depend on Tauri or a production Node runtime. Desktop provisioning and approval remain outside agent tools.

**Stack:** Rust 2024, SQLite, BLAKE3, Tauri 2, Svelte 5, TypeScript, custom Canvas 2D. `PRODUCT.md` and `DESIGN.md` remain canonical.

Execution is inline, without subagents or computer use, as the user requested. The detailed user specification already selects the architecture and A–H order. Routine implementation choices are recorded here and in the status file without additional approval gates.

## Constraints and review focus

- Preserve untracked `interface/prompts.json` and `output/`.
- The user subsequently requests all commits on `main`, with other branches removed after integration. Continue on `main` without new branches.
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
  Core coordination and all twelve native operation guards now share one OS lease with headless scans, retaining it until worker exit. The headless service retains its canonical state coordinator across cached queries and clones; Windows directory-name pins use directory access rather than attribute-only handles. Model/provenance extensions and GUI durable-job/index mirroring remain open.
- [ ] Verify wrong/revoked credentials, sibling paths, links, overlapping roots, concurrency, cancellation and restart. Commit and push.

## C. Plans, headless and MCP

Execution requests and cooperative cancellation persist in plan-store schema 2. Requests are data, never grants. The desktop dispatches only requests bound to its current human session, consuming the live grant under the shared operation lease. Restart requires a fresh explicit request after renewed approval. CLI/MCP expose exact execution/cancel intents; full elevated/Shell acceptance remains manual.

Runtime execution uses one borrowed worker context containing the operation lease, cancellation token and STA apartment. The prepared batch retains those borrows, so they cannot be released or exchanged before it finishes. Complete native evidence validation remains in preflight; bounded authority checks run before helper launch, each admission and each recycling attempt. Use owned normal-helper denial fixtures for non-interactive verification. Elevated admission and Shell acceptance remain separate evidence.

Dispatch unit: one dedicated desktop STA worker consumes current-session requests under the existing native operation guard. Headless execute/cancel only enqueue exact-hash intents and report status. Human provisioning grants execution separately, off by default. Closing the review session releases availability and prevents reacquisition by that session; a second instance may retry acquisition. Replayed or uncertain execution is never dispatched again. Invalidate the current native analysis with its exact index ID when agent work starts, so delayed events cannot clear a newer analysis.

- [ ] Extend the existing cleanup preview/executor with immutable serializable manifests and explicit lifecycle states. Persist hash, principal, exact identities, policy version, expiry and audit. No agent approval endpoint.
  Core manifests, lifecycle, CLI/MCP planning and execution/cancel intents, desktop review/dispatch, complete preflight, journal association, shared coordination, protections, retained worker resources and runtime admission/recycling checks are implemented. Elevated agent acceptance remains open.
- [ ] Add a separate headless crate and CLI for scan/query/stats/history/plan/jobs/export, with JSON envelopes, structured errors and a read-only default.
  Scan/query/stats/history/details/jobs/cancel, exact plan prepare/get/request/execute/cancel/audit and versioned errors are implemented and subprocess-tested. Export remains open.
- [ ] Add official `rmcp` stdio transport, bounded schemas/results and tools that route through authorization and shared services. Verify protocol behavior with the SDK client against the real executable.
  Transport plus nine analysis and six recycling-plan tools are implemented and verified with `rmcp` 3.8.0's real client. The broader catalogue remains open.
- [ ] Add desktop connection management, separate human approval and revocation. Unavailable independent approval returns `needs_human_approval` without changing user data.
  Native management/review and separate confirmation are implemented. Human facade and request behavior are fixture-tested; actual dialog/keyboard/visual acceptance remains manual.
- [ ] Test mutation, expiry, replay, concurrent execution and restart in temporary fixtures. Document generic stdio and current Codex configuration. Commit and push.

## D. Dashboard and explorer

Radial views reuse the exact bounded rectangular frontier and add its explicit parent relation. Sunburst uses equal-area depth bands so colored frontier sectors retain one byte-to-area scale across depths. Neutral parent bands are structural. Nested circles use a global square-root byte scale; parent circles enclose packed descendants and are outlines, not additional size encodings. Packing gaps are disclosed and never labelled free disk space. Both views reuse root/filter/selection/details/metric/color and cached pan/zoom, without rescanning. Verify area, topology, containment, nonoverlap and analytic picking through pure fixtures; rendering/scaling remain manual.

Depth is one application-owned value, shared by Overview and Explorer across every map representation. Restrict it to the six explicit selector values and collapse only the common frontier. Preserve byte totals and the supplied tree; do not rescan, clear selection or promise unloaded levels. If selection is outside the drawn frontier, keep details available and explain the missing shape. Verify the depth boundary and rectangular/radial conservation through pure fixtures.

Grid extends the existing FileTable renderer and cancellation pipeline. Use one to eight responsive columns, bounded pages up to 500 entries, compressed scrolling and two-dimensional keyboard movement. Preserve index identity, root, filters, inspection, cleanup checks and current snapshot. Show metadata in equal tiles with an explicit area legend. Share logical/allocated display and known-allocation ordering with the core query; unknown and partial allocation remain explicit. Loading/cancellation/error never become an empty-result claim. Verify geometry, paging, extreme counts, resize anchors and metric semantics with owned fixtures; native focus/render/scaling acceptance remains manual.

Voronoi uses a flat weighted power diagram of the same depth-limited frontier. Compare first-order weight updates with a damped Newton solve before choosing the solver. Fix deterministic interior sites, clip convex cells in normalized coordinates, and measure both total area discrepancy and worst relative cell error. Retain at most 120 cells, grouping smaller frontier entries with exact membership/byte totals and an explicit list route. The synthetic group ID never reaches a native command. Public geometry returns bounded polygons, anchors, source memberships and quality/work counters; rendering and picking share those polygons. Run production calculation only in a disposable worker with work, iteration and deadline limits; terminate it on mode/root/depth changes and ignore late replies. No main-thread solver fallback. Precision or worker failure offers exact Treemap/List routes. This adapts power-distance and target-minus-measured-area equations from [Caplan's paper](https://www.cs.middlebury.edu/~pcaplan/docs/Caplan_2021_Higher-dimensional_power_diagrams_for_semi-discrete_optimal_transport.pdf), without importing an implementation or dependency.

Inspection is application-owned, separate from cleanup-review IDs and bound to the exact index. Plain click replaces, Ctrl/Meta toggles and Shift adds; focused item buttons support Ctrl/Meta+Space. Clear and Escape cancel pending inspection requests. Reject invalid/synthetic IDs and results beyond 500 entries atomically. Preserve selection across scope, filters, depth, metric and representation within one analysis. Project selected folders onto the common frontier through parent IDs and native scope ancestry, without summing overlapping byte totals. All renderers subscribe to this projected set, including changes that leave the primary details ID unchanged. Verify set operations, index isolation, limits and actual depth/scope projections with pure fixtures. Native interaction acceptance remains open.

Treemap lasso uses a bounded freehand polyline and positive polygon/region area intersection. Concave outlines, disconnected clipped pieces and tiny covered regions retain their actual geometry. Border-only contact is excluded. Reject self-crossing/degenerate traces without partial selection; report skipped synthetic regions rather than substituting their parent. Keep at most 256 displayed points with visible simplification and at most 350 candidate frontier regions. Capture the exact pointer, camera, analysis and reactive geometry context; cancel on context/camera/focus/capture changes and check the expected analysis again in the application batch callback. Lens 2× uses the existing painter, cached geometry and a clipped doubled-scale camera. It does not copy a bitmap, mutate the base camera or add an idle frame loop. Verify clipping, budgets, pan/zoom, frontier identity and lens transforms with pure fixtures; native gesture, focus, rendering, scaling and performance acceptance remains manual.

- [ ] Extend existing summary/details and shared map state; add Sunburst, area-correct circle packing, bounded approximate Voronoi, virtual grid and advanced treemap selection/lens controls.
- [ ] Preserve the existing treemap/Skyline geometry and selection. Complete depth, metric, color legends and cross-view context without rescans.
- [ ] Verify byte conservation, layout limits, picking and shared selection through pure behavior fixtures; leave visual/scaling acceptance manual. Commit and push.

## E. Search, applications and assisted cleanup

- [ ] Add one strict core search parser and deterministic offline translation; connect GUI, CLI, MCP, saved views and policies.
- [ ] Add notes/protections, budgets and favorites to shared persistence. User-only protection removal.
  Folder notes, descendant protection, exact-version human removal, native/agent plan enforcement, rule/duplicate filtering, scoped content-gated note reads and tooltip previews are implemented. Budgets and favorites remain open; native UI acceptance remains manual.
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
