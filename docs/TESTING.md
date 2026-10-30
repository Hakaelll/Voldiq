# Testing

The automated suite contains 8 tests, reduced from 178 on 2026-10-21 at the project owner's request. This retains 4.5% of the original test cases. The other 170 cases and their unused fixtures and modules were deleted.

## Automated checks

Run on Windows with the Rust MSVC toolchain, C++ Build Tools and project dependencies installed:

```powershell
./scripts/verify_project.ps1
```

The script checks Rust formatting, Svelte and TypeScript diagnostics, the frontend test, all workspace Rust tests, strict Clippy and the production frontend build. It stops on failure and reports success only after all six stages pass.

For individual checks:

```powershell
pnpm test
cargo test --workspace
pnpm check
cargo clippy --workspace --all-targets -- -D warnings
pnpm build
```

## Retained coverage

| Cases | Behavior |
|---|---|
| 1 native command test | Cleanup requires the exact confirmation and live index; a plan can be consumed only once. |
| 2 cleanup execution tests | Changed retained duplicate copies prevent deletion; cancellation preserves pending files; journal write failure stops further removal and preserves the partial report. |
| 1 cleanup journal test | Recovery after an interrupted deletion does not retry a replacement file or claim unrecorded success. |
| 1 file disposition test | Replaced files and read-only targets are not removed. |
| 1 Windows handle test | An acquired parent directory cannot be renamed until its handle is released. |
| 1 snapshot integration test | Saved history survives reopening, compares actual file changes and preserves case-insensitive ASCII/Unicode search results through both query methods. |
| 1 frontend selection test | The cleanup selection limit rejects a whole page without partial changes. |

Rust fixtures use disposable temporary folders. The Windows cleanup tests remove only files they create. The frontend test runs directly with Node's test runner and requires no browser automation.

These cases cover selected data-safety boundaries. Broad scanner, query, resolver, treemap, virtual-list and migration coverage is no longer part of the automated suite. Existing benchmark and verification records describe historical runs, not the current test inventory.

## Manual application checks

```powershell
pnpm tauri dev
```

Choose a disposable fixture folder. Check scan progress, capacity and size totals, folder navigation, search, file filters, details and Explorer reveal. Check duplicate review, cleanup confirmation and snapshot comparison with known fixture contents. Cancel an active scan and verify the incomplete state. Check inaccessible folders, keyboard navigation, window resizing, hover navigation, both themes and reduced motion.

The browser preview has no filesystem bridge. Native interaction, performance targets and installed-app acceptance remain separate checks in [Product acceptance](ACCEPTANCE.md). Packaging evidence is in [Windows distribution](DISTRIBUTION.md).
