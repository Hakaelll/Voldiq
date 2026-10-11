# Cleanup previews

The current delivery supports manual selection and a read-only preview. It does not expose recycling or permanent deletion yet. A preview is not approval to execute a destructive operation.

## Manual file rule

| Field | Contract |
|---|---|
| ID | `manual-files` |
| Name | Selected regular files |
| Discovery | Explicit IDs from the current native index |
| Selection | 1–500 distinct, complete regular files |
| Exclusions | Historical captures, root directories, reparse points, recall-marked files, excluded ancestors and protected system locations |
| Risk | Review required |
| Permissions | Current user's metadata access; later execution must obtain delete access |
| Planned policy | Windows Recycle Bin, subject to confirmed execution support |
| Verification | Scanned identity/change time, actual metadata, captured ancestors and native handle checks |

Scans retain file identity and native change time alongside entry data. The evidence stays in memory and does not cross the row IPC boundary or become authority for a loaded historical snapshot. Preview creation rejects replacements since scanning even when size and modification time match. Windows protected locations come from `SHGetKnownFolderPath`; resolution failures abort the preview. Protection compares native directory identities so case aliases do not remove it.

The preview reports logical size and conservative potential allocation. A file with multiple hard links contributes zero potential because removing one visible name may leave other links. Moving a file to the Recycle Bin does not itself free that allocation.

## Verification handles

`Plan::verify` opens and pins ancestors from the volume root inward, validates their recorded identities and rejects redirected or recalled directories. Its leaf handle blocks new writes and renames while open and compares exact preview identity, size, modification time and change time. Controlled tests verify replacement before preview, changes after preview and blocked writes/renames while these handles remain alive.

These checks do not yet prove a recycling implementation. A later executor must preserve object binding through the native operation; simply closing the handles and calling a path-based deletion API would reopen a race. It must also handle unsupported recycling without silently deleting permanently, preserve recoverability, record partial failure and refresh the index. Concurrency tests for ancestor redirection and the complete recycle transaction remain pending.

## Interface and command boundary

The virtual explorer and large-file list expose native checkboxes without automatic selection. Review selection requests an immutable backend plan; the modal shows full paths, logical size, potential allocation and hard-link labels. It contains one Close action. Native dialog behavior supplies modal keyboard focus and Escape dismissal. Its populated visual acceptance remains with the user.

The backend retains one plan session. Discard identifies that session, so a stale discard cannot remove a newer plan. Starting a scan or loading a snapshot clears frontend selection and preview. Before future execution, the backend must also verify that the plan still belongs to the active index and consume the confirmed session once.

Temporary-file rules, compatible cache discovery, duplicate copy retention, developer tool commands, confirmed recycling, result logging and undo integration are still open work.
