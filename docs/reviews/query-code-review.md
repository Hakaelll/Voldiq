# Ranked query review

Reviewed `Index::query`, `in_scope` and their scanner, duplicate and native query callers using Ponytail and Ponytail Review. Query validation still occurs before scope filtering. Partition boundaries handle empty results, offsets beyond the final row and limits above the IPC cap. Unique node IDs break sorting ties, preserving stable pages across requests. Only the page is cloned; the temporary match vector holds references.

ASCII comparison preserves lowercase lexical order without allocations. Non-ASCII names use the existing Unicode lowercase behavior. Root scope is valid for every indexed node; other scopes retain parent walking. Two actual-filesystem tests cover ranked pagination and literal ASCII/Unicode ordering. The million-entry fixture measures only generated in-memory queries, with no fabricated UI or filesystem results.

Global name ranking and matching-reference memory remain measurable costs. Directory filtering still scans the index, and deep scopes still walk parents. The benchmark records unchanged root-query and no-match-search costs instead of claiming improvement for all queries. Actual million-file scanning, concurrent-query latency and total application memory remain open.
