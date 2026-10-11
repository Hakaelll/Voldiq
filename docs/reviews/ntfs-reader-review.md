# NTFS reader review

Reviewed the binary parser, native handle ownership and cursor logic using Ponytail and Ponytail Review.

Parsing checks record lengths before fields, validates major versions, honors filename offsets and preserves raw UTF-16. The callback cannot advance beyond the validated record slice. The Windows boundary checks returned byte counts. Native handles use standard Rust ownership. Metadata opens request attributes only and avoid following the final reparse point.

The change reader verifies volume, journal identity and lower/upper cursor bounds before reading and rechecks continuity afterward. Limits must not discard records while claiming a completed cursor; the final-buffer overflow flag and `ReadState` prevent that. Cancellation and limits have separate states without resumable checkpoints.

Four tests and strict Clippy pass. The raw-volume probe receives access denied and the conventional disposable hard-link fixture passes. No privileged NTFS operation is claimed tested. Live index construction, complete alias coverage and snapshot checkpoint migration remain pending.
