# Recycling transaction design

Status: implementation design, not an enabled executor. The separate admission and Shell fixtures have passed; the combined two-process transaction has not been verified.

## Implemented transport

[`windows::cleanup_channel`](../crates/voldiq-core/src/windows/cleanup_channel.rs) provides a one-connection local message pipe. Its protected DACL admits the current process user and SYSTEM, and the pipe rejects remote clients. Both endpoints compare Windows-reported pipe peer IDs with a live, retained process handle supplied by the caller. An unexpected peer closes the connection. The client requests identification-only security quality of service rather than the default impersonation level; see [Microsoft's named-pipe impersonation contract](https://learn.microsoft.com/en-us/windows/win32/ipc/impersonating-a-named-pipe-client).

Messages are bounded to 4 KiB. Empty/oversized outbound messages are rejected before I/O. Read/write/connect operations use overlapped I/O with a five-second wait. A timeout cancels and drains that specific operation before releasing its stack/buffer, then closes the channel. Cancellation completion itself is not promised to finish within five seconds. A failed receive cannot be resumed as a fresh protocol message.

The [initial normal-process observation](../benchmarks/cleanup-channel-2026-10-30.json) runs the actual module through [this executable fixture](../crates/voldiq-core/examples/cleanup_channel_probe.rs). It verifies a maximum-size message and reply, rejected outbound sizes, a rejected client process, a rejected server process, and cancellation/closure of an idle read. That run starts and reaps only its own child processes; it never elevates, relocates files or calls Shell. Reproduce the current extended fixture with `cargo run --release -p voldiq-core --example cleanup_channel_probe` on Windows. The retained eight-test suite is unchanged.

This is the transport portion of the first implementation gate below. The launcher still has to establish executable, user, role and elevation expectations before giving the channel its trusted process handles. Cross-integrity handle transfer, malicious inbound frames, the staging protocol and the combined privileged transaction remain unverified. No desktop command exposes this channel.

### Source-handle transfer

`Channel::offer_source` borrows an existing file until the receiver acknowledges its native volume/file identity and granted access. `receive_source` pulls a duplicate from the authenticated peer process with `DUPLICATE_SAME_ACCESS`. It never treats a received numeric handle as locally owned authority, never closes the sender's original and never requests additional file rights. The channel retains its own process-handle reference for this exchange. Receiving requires `PROCESS_DUP_HANDLE` on that peer process. The stage design below avoids requiring the normal process to pull a directory handle from an elevated process.

The versioned fixed-size offer includes the source-process handle number, volume serial, full 128-bit file ID and granted-access mask. Both ends require disk files with delete/attribute-read rights and permit only optional data-read/synchronize rights. Writable/security-changing handles, directories, reparses and offline/recall-marked sources are rejected. The receiver independently queries the duplicate before acknowledging matching evidence. A malformed or failed transfer closes the channel and releases only receiver-owned resources. This transfers authority; it does not replace live-plan, metadata, content, ancestor or confirmation checks and does not authorize a filesystem mutation.

The [extended normal-process observation](../benchmarks/cleanup-source-transfer-2026-10-30.json) repeats the original transport checks and transfers one owned fixture. The receiving process cannot write through its duplicate. After the sender closes its original, renaming the source fails with sharing violation 32; after the receiver releases its copy, rename succeeds and the bytes remain unchanged. A crafted offer referring to a real read-only sender handle is rejected, the channel closes, and the sender can still read through its original handle. The fixture then removes its own files and directory. There is no elevation, protected staging or Shell call. Other malicious inbound forms, oversized inbound frames and cross-integrity transfer remain separate proof gates.

### Stage-parent transfer

`offer_stage_parent` and `receive_stage_parent` use a distinct frame/acknowledgement kind for directory authority. Native validation requires directory-listing, attribute-read, create-subdirectory and delete-child rights already present on the sender's handle. Only optional read-control, read-extended-attribute, traverse and synchronize rights are accepted. The receiver verifies directory type, full volume/file identity and the unchanged granted-access mask after `DUPLICATE_SAME_ACCESS`. This does not permit replacing a source-file message with a directory message or using a listing-only parent as evidence of ordinary create/remove authority.

The [stage-parent observation](../benchmarks/cleanup-stage-parent-transfer-2026-10-30.json) repeats the earlier channel/source checks and transfers a real parent directory between normal processes. The receiver independently acquires the existing protected stage through the duplicated parent and checks its expected identity and permissions. After the sender closes both creator handles, parent and stage renames still fail with sharing violation 32. The parent can be renamed after the receiver releases both handles. A parent lacking create/remove rights and an attempt to offer the directory as a source file are rejected before transmission. Owned directories are then removed. This verifies normal-process authority transfer and cross-process namespace pinning, not elevated admission or the complete cleanup transaction.

### Protected namespace pins

[`windows::cleanup_stage`](../crates/voldiq-core/src/windows/cleanup_stage.rs) creates an empty protected directory relative to an already opened parent. The backend chooses a validated `StageName` before creation so it can record that name before mutation. Creation uses `FILE_CREATE`, never an open-existing fallback. The protected descriptor applies from the first instant: enabled Administrators have full access; the current user and OWNER RIGHTS have only read/list/traverse access. The explicit owner-rights entry suppresses implicit owner permission to rewrite the DACL.

`StagePin::acquire` independently opens an existing stage through its parent handle, checks its full native identity and compares the actual owner/protected DACL with the expected policy. Both creation and acquisition retain directory-listing handles without delete sharing. Invalid names, collisions, wrong identities, reparses and unexpected permissions fail closed. The implementation limits stages to fixed local NTFS volumes with persistent ACLs and a stable volume-GUID path. A failure after native creation can leave the already named empty directory for explicit reconciliation; no source has been relocated.

The [normal-token pin observation](../benchmarks/protected-stage-pin-2026-10-30.json) exercises [the actual module](../crates/voldiq-core/examples/protected_stage_probe.rs). A second read-only pin preserves the directory name after the creator pin closes. Child creation and rewriting the unchanged DACL fail with access denied; rename fails while pinned and succeeds after release, preserving identity. Wrong identities, duplicate creation and an ordinary directory with incorrect permissions are rejected. The fixture removes its own empty directories. The same record reruns the channel/source fixture after sharing the Windows security-descriptor implementation.

The initial directory-create attempt combined directory creation with `FILE_OPEN_NO_RECALL` and returned `STATUS_INVALID_PARAMETER` on this host. Creation now uses no-follow directory creation, while acquisition uses a type-neutral no-follow/no-recall open followed by directory validation. No fallback follows reparses or relaxes the DACL. See the [native create options](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntcreatefile).

## Existing execution path

`Plan` owns the reviewed selection, source stamps, ancestor identities and retained duplicate copies. `consume_plan` in the desktop adapter checks the live index and consumes confirmation once. The permanent executor retains verified handles, calls `Journal::prepare_permanent`, commits `start_item` before disposition, and records the result afterward. The current journal accepts only permanent operations. Reopening it marks interrupted in-flight results unknown. Recycling must extend this path without weakening its confirmations or treating an unknown result as success.

The original requirements allow elevation for one explained operation and prohibit elevating the whole application. They require race protection, partial-result reporting and a distinction between recycling and permanent deletion. They do not authorize silently substituting permanent deletion when recycling fails.

## Chosen boundary

Keep review, journal ownership, protected-stage creation/pinning, Shell calls and restoration in the ordinary desktop process. Use one short-lived elevated helper per explicitly confirmed batch only to admit the already verified files into those stages. No resident service, scheduled task or permanent administrator setting is installed.

The helper must receive source authority through duplicated handles acquired by the ordinary process. It must not accept a pathname and reopen it with administrator permissions. `DuplicateHandle` retains the same underlying object; its numeric value belongs to the receiving process and is never durable recovery data. See [Microsoft's handle contract](https://learn.microsoft.com/windows/win32/api/handleapi/nf-handleapi-duplicatehandle).

Create each staging directory with its protected security descriptor from the first instant. Normal callers may read/list/traverse but may not add children, change security or install reparse metadata. Administrators retain admission authority. The normal process already owns the read-only pin; no administrator security-modification handle crosses back into it. Keep the directory pinned against deletion and rename throughout handoff and Shell execution. A writable parent must not permit replacing the directory through parent delete-child authority.

Use stable same-volume addressing and independently verify volume, directory and source identities. Start with supported local NTFS volumes. Reject unsupported permissions, filesystem behavior, paths or identity representations before relocation. Do not infer a successful protected namespace from a random directory name. New-object security must be explicit rather than inherited from a writable parent. See [security descriptors for new objects](https://learn.microsoft.com/en-us/windows/win32/secauthz/security-descriptors-for-new-objects).

## Ownership and sequence

1. Consume the confirmed live plan and acquire the existing strict source/ancestor guards. Keep duplicate retained-copy authority alive for the batch.
2. Commit original paths, native identities, stamps and intended stage locations to the journal before any source relocation. Create/pin each stage as the ordinary user and commit its actual identity. Require ordinary authority to create and remove empty directories beneath the stage parent, and the same volume as the source. If any check or commit fails, do not move files or start Shell operations.
3. Launch the one-use helper through Windows consent. Authenticate both IPC endpoints, constrain the protocol to this batch, reject remote clients, and bind the actual peer process and user. A pipe name or caller-supplied PID alone is not authentication. Declined consent leaves sources unchanged.
4. The helper authenticates the source and stage-parent capabilities, validates the existing stage's identity/policy and waits for journal readiness. It may not create a directory with administrator permissions or reopen a source by pathname. Only then may it enroll a source through its duplicated handle. The ordinary process retains the stage pin and strict source guard across this step.
5. In the ordinary process after relocation, verify the staged object and acquire the read content guard through `ReOpenFile` on the retained source object, before releasing the strict leaf guard. The elevated helper must not introduce read rights that the ordinary caller could not acquire. Keep ancestors, stage and content pinned. Account for metadata changes caused by the application's own rename; do not waive identity or content validation because a pathname matches.
6. Commit recycling intent. On an STA worker, queue only the verified staged item with recycle flags. Require the per-item callback and an actual returned Recycle Bin item. A successful queue or aggregate HRESULT is insufficient. Never retry as permanent deletion.
7. Persist the returned bin-item locator, native identity and original-location mapping before reporting success. If this write fails, stop the batch and retain an unresolved result. Do not retry the native action.
8. Release per-item guards only after recording the outcome. The ordinary process removes only verified empty staging directories using its existing parent authority. No actor recursively deletes stage contents. Refresh the index after the batch; moving to the bin does not itself establish recovered allocation.

Cancellation stops admitting new files. An admitted file needs a recorded outcome or a verified move back; it must not disappear from activity because the user canceled. Helper exit, IPC loss, journal failure and application shutdown follow the same rule.

## Journal and recovery

Extend the existing database with an atomic, versioned migration that preserves permanent-operation history. Model recycling separately from permanent removal. Persist exact original and stage names, volume identity, file identity, stage-directory identity and a recoverable Shell locator. Store no raw process handles, lossy path conversions or assumed-success flags.

| Durable state | Next permitted action |
|---|---|
| Prepared source | Validate and stage, or cancel without touching it |
| Protected stage recorded | Admit only after the ordinary process owns its pin |
| Relocation in flight | Reconcile identities at original/stage locations; do not blindly retry |
| Staged | Verify and recycle, or explicitly restore the verified original |
| Recycling in flight | Reconcile stage and recorded Shell evidence; otherwise remain unknown |
| Recycled with locator | Offer restoration of this recorded item |
| Restoration in flight | Reconcile identities before reporting a result or another attempt |
| Restored | Report its verified destination and refresh the index |

Unknown outcomes stay unknown when evidence is missing or contradictory. Startup performs read-only reconciliation; it does not automatically move or delete anything. Reacquire and verify every recovery object before an explicitly requested action. A path reoccupied by another file must remain untouched.

Shell records the staging location as the deletion origin, so the application must retain the actual original-location mapping. Restoration acts on the returned bin item and this mapping, not the session-wide Undo stack. Check destination authority, refuse overwrite, and offer a separately confirmed destination when the original is unavailable. A restored stage path is not the user's original location.

## Implementation order and proof gates

1. Complete the authenticated helper exchange and demonstrate source/stage-parent capability transfer and source admission between the actual elevated and normal processes. Normal-process transport, source duplication and stage creation/pinning now have executable fixtures. Cross-integrity admission, reparse/replacement attempts, helper failure and declined consent remain open.
2. Connect the already verified content guard and Shell callback flow to that exchange. Preserve the exact source through recycling and restoration while competing operations attempt substitution. A Shell-only fixture does not close this gate.
3. Add the journal migration and transaction states together with their callers. Extend the retained journal regression rather than recreating the deleted suite. Inject failure before/after relocation, before/after Shell, and before/after result commits. Restart and prove that replacement files survive reconciliation and attempted recovery.
4. Connect reviewed recycling, bounded activity and per-item restoration to the desktop command boundary. Keep permanent removal under its separate typed confirmation. Report failures, cancellation and unresolved results without claiming a complete batch succeeded.
5. Verify the installed application and refresh the distribution build. Visual interaction remains the user's acceptance task.

The first two gates need a new, specifically authorized UAC run once a reviewable two-process fixture is ready. The earlier authorization covered one admission probe, which has already run. It does not authorize another elevated experiment. Until all gates pass, the application continues to expose the existing advanced permanent action and Windows-owned bin access, with default recycling explicitly unavailable.
