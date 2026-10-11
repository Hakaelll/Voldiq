# Implementation ledger

Voldiq is under active development. A task becomes DONE only after its acceptance checks pass. The supplied specification is preserved in `docs/reference/requirements.md`.

| ID | Task | Priority | Status | Dependencies | Acceptance criteria | Related files |
|---|---|---|---|---|---|---|
| V01 | Windows app and build foundation | P0 | DONE | None | Svelte checks, Rust build, real Windows window | src-tauri, frontend |
| V02 | Native volumes and conventional scan | P0 | IN_PROGRESS | V01 | Real fixtures, cancellation, errors, links, aggregation, bounded queries, partial results, calibrated concurrency | crates/voldiq-core |
| V03 | NTFS enumeration and metadata | P0 | TODO | V02 | Validated binary records, fallback, volume comparison benchmark | crates/voldiq-core/src/ntfs.rs |
| V04 | Visual explorer and search | P0 | IN_PROGRESS | V02 | Canvas navigation, virtual rows, real IPC data, keyboard navigation | frontend |
| V05 | Large files and duplicates | P1 | IN_PROGRESS | V02 | Filters, BLAKE3 content verification, cancellation, hard-link handling, reviewed selections, native interaction | crates/voldiq-core, frontend/components/Duplicates.svelte |
| V06 | Safe cleanup and developer storage | P0 | TODO | V05 | Preview, exclusions, identity validation, confirmation, controlled fixture execution | crates/voldiq-core |
| V07 | Persistent snapshots | P1 | TODO | V02 | SQLite transaction, restart, additions/deletions/growth comparison | crates/voldiq-core |
| V08 | Incremental NTFS index | P1 | TODO | V03, V07 | Journal continuity validation and full resynchronization | crates/voldiq-core |
| V09 | Preferences and accessibility | P1 | IN_PROGRESS | V04 | Persistent preferences, themes, reduced motion, keyboard checks | frontend, src-tauri |
| V10 | Benchmarks and quality audits | P0 | IN_PROGRESS | V02-V09 | Reproducible measurements, Ponytail review and audit, resolved defects | benchmarks, docs |
| V11 | Windows distribution | P0 | TODO | V10 | Installer build, offline smoke check, final icon, documentation | src-tauri, README.md |

## Delivery policy

Commit verified functional units with English messages and push to the configured repository. Preserve pending work and measured limitations. Never claim that the full acceptance list has passed while items remain open.

## First verified delivery, 2026-10-11

The native executable builds and starts a real window titled Voldiq. Six Rust filesystem tests, two treemap tests, Svelte diagnostics and workspace Clippy pass. The release scanner benchmark checks 10,000 generated files over three passes with no errors. Its observed scan range is 762.810-868.573 ms on this host; see `docs/PERFORMANCE.md` for methodology and limits.

The frontend includes a real command bridge, custom map, virtual explorer, search, large-file filters and persisted visual preferences. Browser verification covers initial and settings states; populated native interaction still needs verification. There are no duplicate, cleanup, snapshot or NTFS simulations.

Live partial queries now share one Rust index and update ancestor totals on insertion. A seventh Rust test checks live reads, partial totals and cancellation. Visible UI copy was reduced at the user's request. The updated benchmark is slower; its measured values and uncertainty are recorded in `docs/PERFORMANCE.md`.

Next: validate native IPC on controlled fixtures, calibrate scanner concurrency, and implement NTFS enumeration with metadata acquisition and explicit fallback. Duplicate analysis, cleanup, snapshots, incremental indexing and distribution remain open.

## Duplicate analysis delivery

The real BLAKE3 engine filters unique sizes, samples at most 8 KiB, verifies complete content, reuses stamps validated with file identity, modification time and native change time, and reports locked or vanished files. Five tests cover matching samples with different middle content, cache reuse and invalidation, hard links, unique sizes, sampling, cancellation and locked files. Duplicate groups and members cross IPC in bounded pages. Duplicate results retain their original index to avoid reinterpreting IDs after a rescan.

The duplicate view uses minimal labels and measured data. Management selection, confirmation and recycling remain part of the safe-cleanup work; no deletion command is exposed yet. Native populated interactions and cross-volume fixtures remain open.
