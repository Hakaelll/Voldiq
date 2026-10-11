# Duplicate analysis review

Scope: real read-only analysis, hash caching, bounded IPC pages and its interface. The review traced candidate selection, metadata validation, sampling, full hashing, cancellation, hard-link identity, result ownership and error reporting.

Resolved: result queries retain the original index, preventing old node IDs from being interpreted against a new scan. Byte progress includes completed reads even when hashing is cancelled. File change time joins modification time in cache validation. The cache has a bounded identity count. Late page responses are discarded.

Evidence: five real-filesystem duplicate tests and eight scanner tests pass. Svelte reports zero errors and warnings; workspace Clippy passes with warnings denied; production frontend builds. The Impeccable detector reports no findings in the new component. The Windows tests preserve an unpaired UTF-16 filename in the native index and avoid copying its lossy display form as a path.

Limits: selections and destructive operations remain unimplemented. Cross-volume copies, large duplicate workloads, cache persistence and populated native interactions remain open. Linux's final-link check uses O_NOFOLLOW; the full Windows target is the supported delivery platform. Existing reparse points are excluded; content analysis does not establish a future destructive path-validation policy.

Verdict: the read-only duplicate engine and bounded view are ready as a development increment. This is not complete cleanup or product acceptance.
