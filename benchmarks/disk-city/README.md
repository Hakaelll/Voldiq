# Skyline verification, 0.2.0

The screenshots come from the native Windows Tauri/WebView2 application scanning disposable filesystem files. The baseline uses the previous Overview; the final captures use Explorer at the same root and viewport. No fixture data is bundled into production.

## Measured drawing cost

Five interleaved repetitions per variant and dataset on an Intel Core i7-1165G7, eight logical processors and 15.8 GiB OS-reported physical RAM. Background desktop/build load was uncontrolled. These are Canvas command-submission times, not GPU presentation latency or an optimization speedup.

| Dataset | Renderer | Blocks | Median ms | Range ms |
|---|---|---:|---:|---:|
| root | flat | 56 | 0.6 | 0.4–0.7 |
| root | basic | 56 | 2.1 | 2.0–2.9 |
| root | full | 56 | 3.3 | 2.9–5.5 |
| dense | flat | 251 | 1.7 | 1.4–2.0 |
| dense | basic | 251 | 9.8 | 8.8–12.2 |
| dense | full | 251 | 19.1 | 18.5–20.6 |

Root uses a 1366 × 768 viewport and dense uses 1920 × 1080, both at device-pixel ratio 1. Basic/full share geometry; full enables architectural detail and shadows. Layout, worker startup/preparation, projection, BSP ordering, shadows and JS heap samples are in `native-smoke.json`. Flat rows retain the last city geometry counters and must not be interpreted as new flat geometry work. The dense full-detail draw takes about 19 ms on this machine; basic detail reduces the measured draw cost. No 60 FPS or GPU latency guarantee is made.

The 600 ms settled idle check observed zero Canvas redraws. `processes.csv` and `processes.processes.csv` contain five one-second samples after the interaction run: seven owned native/WebView2 processes, no read errors and approximately 291 MiB combined private allocation. Summed working sets include shared pages and can double-count them; they are not unique physical memory. No before/after memory claim is made.

## Evidence and scope

- `source-gate.json`: Rust formatting, Svelte diagnostics, frontend tests/build, ten workspace Rust tests and strict Clippy. Its working-tree gate preceded the final UI polish.
- `frontend-final.json`: final source commit, twenty frontend tests, build and native smoke checks, preview identity and embedded-frontend hashes.
- `distribution.json`: final x64 executable/installer, version 0.2.0, decoded embedded assets, offline WebView2 configuration/vendor signature and icon sizes. The application and installer are unsigned.
- `native-smoke.json`: fourteen final captures, real query/view/selection/navigation checks, rapid metric changes, missing-worker fallback and a queued late-error cancellation fixture. Runtime errors: zero.
- `million-index.csv`: five repetitions on each of three synthetic in-memory million-file shapes using the production Rust overview module. No filesystem scan, UI or IPC latency is included.
- PNGs: light/dark, collapsed/expanded sidebar, inspector open/closed, narrow overlay, list selection, sparse and dense scenes. Pixel ratios 1, 1.25, 1.5 and 2 are emulated inside native WebView2.

Windows operating-system display scaling was not changed. Clean-machine installation, physical monitor DPI changes, screen-reader acceptance and GPU presentation timing remain unverified. The read-only fixture scan and existing cleanup tests did not remove user files.

The original Impeccable sidecar was already stale and was not regenerated. DESIGN.md remains the canonical visual record.
