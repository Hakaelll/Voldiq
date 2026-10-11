# Architecture

Voldiq has one reusable engine crate and one desktop adapter. The engine can run without Tauri through its scan example and integration tests.

## Engine

`scanner` performs an iterative directory walk with cancellation and at most one metadata operation in flight. This conservative I/O policy is the initial baseline, not a claim of optimal performance on every drive. It excludes requested paths and records reparse points without following them.

The index stores compact 32-bit node IDs, parent IDs and names. Full paths are reconstructed only when requested. Parents precede children, allowing a reverse pass to aggregate directory totals. Allocation deduplication uses volume identity and file identity. Logical totals retain each path name.

Queries return at most 500 entries. The frontend renders at most 80 table rows per virtual window and requests at most 250 direct children for the Canvas. Unrepresented bytes appear in a smaller-items region linked to the complete virtual list.

The initial query implementation filters and sorts the in-memory index. This is measurable baseline code; a directory adjacency index and bounded ranking will be added if measurements justify them. Progress is emitted no more frequently than once per 100 ms, plus the terminal update.

## Desktop adapter

Tauri exposes only volume discovery, native folder selection, scanning, cancellation, bounded queries, details, file types, scan issues and Explorer reveal. No arbitrary frontend filesystem API or shell command is exposed.

Scanning and queries run in background tasks. The engine owns an immutable completed index behind an `Arc`. A new scan owns its working index, and publishes it before emitting its terminal progress. A second scan cannot start until the first releases the active operation slot.

## Interface

Svelte owns interaction state, preferences and bounded query results. A custom squarified treemap redraws when geometry, selection or hover changes. There is no persistent animation loop or idle polling. Layout uses Windows system fonts and local artwork.

## Pending architecture work

NTFS enumeration, partial-index publication, duplicate analysis, persistent snapshots, cleanup plans and incremental journal validation remain tracked in `NEXT_TODOS.md`. No production behavior is simulated for those modules.
