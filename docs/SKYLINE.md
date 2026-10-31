# Skyline

Skyline is Voldiq's isometric storage view. It uses the same measured rectangles as Treemap. The name and architectural detail follow the user's supplied city reference; the implementation does not copy its code or assets.

## Shared data and layout

`App.svelte` owns the loaded index identity, current directory, refresh revision, search, filters, metric and selected details. `StorageMap.svelte` consumes that state in Overview and Explorer. Its representation buttons change local rendering state; they do not invoke an index query or scan. The paged file list retains access to every indexed entry, including empty files and contents represented by aggregates.

`storage-map.ts` lays out a fixed 1000 × 700 plane. Its frontier contains files, collapsed directories and identified remainders. Expanded directory boundaries are separate metadata, never additional byte-bearing blocks. Every child and remainder stays inside its parent rectangle. Changing the viewport, camera, color or relief leaves these coordinates unchanged. Layout revisions describe the measured frontier; the application refresh revision is local and is not presented as a native snapshot revision.

The existing overview query sends at most 250 direct children and twelve branches of eight children. Expansion stops at a 350-block budget and fixed layout dimensions. Remainders conserve bytes rather than dropping small files or replacing regions with overlapping bounding boxes. The display reports represented files, drawn blocks and blocks outside the viewport separately from the application's indexed-entry count.

## Meaning of the geometry

Area represents logical bytes or unique physical allocation, according to the selected metric. Unknown allocation coverage stays explicit. Allocation is not guaranteed recoverable space; existing hard-link, sparse-file and compression accounting remains in the engine. A zero-byte file receives no artificial area.

The initial height is age since modification. File age uses the indexed modification timestamp. Directory age uses the newest known regular descendant file timestamp, together with known/total coverage. The cancellable native aggregation uses a bounded 4096-directory ancestry cache; it never substitutes the directory's own timestamp. Remainder age remains unknown when the bounded response cannot establish it.

Age height is `5 + 125 × log1p(min(ageDays, 3652.5)) / log1p(3652.5)`, capped at ten years. File count uses `log1p(count) / log1p(folderMaximum)` and counts all indexed regular descendants. Depth is relative to the current folder: direct children have depth one. File count and depth normalize within the folder. Unknown dates keep a low block and an explicit description. Relief multiplies visual height without changing the metric. Apparent volume does not represent bytes. Last access is unavailable because this index has no access-time reliability signal; the application changes no Windows policy.

File categories share a muted amber, terracotta, olive, teal, violet and neutral palette across both renderers, list and legend. Size uses a folder-relative logarithmic teal scale. Age uses fixed 30-day, 180-day, one-year and ten-year bands. There is no invented cleanup score or safety palette.

## Projection and visibility

`disk-city.ts` projects orthographically using `u = cos(30°) × (x − y)` and `v = (x + y) / 2 − z`. The view direction has kernel `(1, 1, 1)`. Camera fitting includes the elevated bounds.

Visible roofs and positive-X/positive-Y facades enter an axis-aligned binary space partition. Bounding intervals classify faces before geometric clipping. Crossing polygons split at the partition plane; traversal visits the far half, coplanar faces, then the near half. Every viewing ray crosses each plane at most once, so this also resolves arrangements that would cycle under whole-object ordering. A fragment budget produces a recoverable error instead of an arbitrary draw order. Picking uses projected spatial bins and the exact reverse face order, including elevated facades.

`map-painter.ts` uses one light direction. Physical prism shadows project onto receiving roofs, facades and ground, with spatial candidate bins and face clipping. The geometry cache is independent of zoom, color and selection. Overlapping shadow silhouettes fill once per receiver. Detail consists of roof edges, deterministic window panels, contained rooftop equipment and aggregate hatching. Pixel thresholds and a window budget remove detail before it becomes noise. Decoration has no separate selection target or byte weight. Labels use screen-space collision boxes and test roof visibility before drawing.

## Interaction and safety

Click selects; double-click or Enter explores directories and inspects files. Files are never executed. Dragging begins after a five-pixel threshold. Wheel zoom stays centered on the cursor. Visible Fit, zoom and relief controls supplement keyboard arrows, plus/minus and zero. Escape on the map closes selection before returning to the parent. Cameras are retained by index, folder and representation with a bounded cache.

Geometry preparation runs in a small worker when supported. New work terminates the previous worker; generation ownership rejects obsolete results. Synchronous preparation is the fallback. Canvas paints only on invalidation and coalesces requests into one animation frame. There is no ambient animation or idle render loop. OffscreenCanvas was considered but is not needed for the bounded scene; the worker does not transfer Canvas ownership.

The existing details panel is dismissible, uses a 300-pixel desktop column and overlays the map below 1200 pixels. The file table remains the keyboard-accessible alternative. Review cleanup opens the existing native preview for a concrete file; folders cannot authorize descendant cleanup. Execution still requires the existing human confirmation and native revalidation. No graphical path adds deletion authority.

## Verification

Run `pnpm test`, `pnpm check` and `./scripts/verify_project.ps1`. Geometry tests compare picking with an independent analytic ray-box reference, including cyclic, narrow, tall and seeded scenes. Shadow tests compare receiving-surface silhouettes with independent light-ray intersections. Layout tests verify byte/area conservation, disjoint frontiers, determinism, unknown metrics, scaling and text contrast. Native tests verify descendant timestamps, coverage, bounded-cache eviction and cancellation.

For repeatable Windows UI checks, launch an owned native preview with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`, then run `node scripts/disk_city_smoke.mjs`. The script creates disposable files only under `target/disk-city-fixture`, uses the real scanner and stores screenshots and measured samples under `benchmarks/disk-city`. Close that debug-enabled preview after testing. The ordinary installer enables no debugging port.

Viewport sizes and device-pixel ratios are emulated inside native Windows WebView2. They do not establish operating-system display-scaling behavior, clean-machine installation, screen-reader acceptance or GPU presentation latency. Canvas timings measure command submission on this hardware. They are not a universal performance guarantee. The million-file Rust benchmark uses in-memory synthetic indexes; the native screenshots use real disposable filesystem data. No test deletes user files.
