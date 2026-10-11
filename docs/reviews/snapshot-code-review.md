# Snapshot implementation review

Reviewed the core store, native commands and History component using Ponytail, Ponytail Review and Ponytail Audit.

The storage model retains a baseline and version intervals instead of copying unchanged entries. Native UTF-16 paths survive persistence. SQLite integers use checked signed conversion at the database boundary. Transactions roll back cancelled captures and conversion errors. Snapshot deletion preserves versions needed by surviving captures. Root identity and exclusion coverage prevent unrelated scans from producing false additions or deletions. Incomplete coverage labels missing entries as unobserved.

The native bridge serializes database work in blocking tasks, and the operation gate prevents loading or saving from racing scans. Loaded captures carry their historical origin; a new capture requires a rescan. Queries and comparisons cross IPC in bounded pages. Late frontend responses use request generations; the final empty page retains a Previous action. Source review found and resolved false empty/loading claims and keyboard focus loss during confirmation, pagination and Retry.

Five real-filesystem snapshot tests pass. Strict Clippy and native compilation pass. Initial browser checks cover empty History; the user requested an end to visual-review cycles and will handle visual acceptance. Populated native interaction and large-history performance remain unassessed. Hash-cache persistence, NTFS journaling and safe cleanup remain separate pending work.
