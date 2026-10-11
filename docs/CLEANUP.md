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

## Duplicate selection review

Duplicate checkboxes start empty and remain selected across result pages. The frontend and backend both reject selecting every member of a group. The backend rejects selections outside the current analysis and analysis results from an older index. For every touched group it chooses one unselected copy to retain, reads the full content of that copy and every selected file with BLAKE3, and compares the original group digest. These reads bypass the hash cache and check the scanned file identity and native change time. The review can be cancelled during content reads.

The preview shows retained paths under Keep. This is evidence at preview creation, not a guarantee at a future deletion time. The executor must repeat retained-copy validation and preserve that copy through the destructive transaction. No recycling command is exposed in this delivery.

Temporary-file rules, compatible cache discovery, developer tool commands, confirmed recycling, result logging and undo integration are still open work.

## Native recycling handoff investigation

The documented [Shell deletion interface](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-deleteitem) accepts a Shell item rather than an existing file handle. Keeping a handle that denies delete sharing prevents Shell deletion; releasing it before a path-based operation permits substitution. Ancestor handles pin directory names, but do not by themselves prevent reparse metadata changes. A native disposition call would permanently delete and is not a recycling fallback.

`benchmarks/recycle-binding-probe.ps1` tests one possible sealed staging handoff on a disposable x64 Windows fixture. It creates a directory with a restrictive DACL from its first instant, including owner-right restrictions and denial of child creation, attribute writes and security changes. It then attempts a handle-based rename using an already-open verified source and the staging directory handle. No personal files or Recycle Bin items are touched.

On this host, atomic directory creation succeeded and new children were denied. The requested access was `0x001600a2`, but the returned handle had `0x001200a2`, without the requested DACL modification right. Native relative rename returned `STATUS_ACCESS_DENIED` (`0xc0000022`); the Win32 attempt returned error 87. Native ACL restoration through the created handle also returned access denied. The fixture was removed using its parent permissions. This disproves that enrollment approach on this host; it does not prove that every possible Windows design is unavailable.

Creating an initially writable staging directory and sealing it afterward is also insufficient: previously granted mutation handles remain authorized. A future implementation needs a proven namespace handoff and durable recovery before any source relocation. A staged item also needs an original-path restoration mechanism because Shell would record the staging path. Per-item [PostDeleteItem](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperationprogresssink-postdeleteitem) results must contain an actual Recycle Bin item before reporting recycling, and aborted operations and partial failures must remain distinct. This investigation has not authorized a destructive executor.
