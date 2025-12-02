# Performance Matrix & Tuning Guide

**Architecture:** Path Compressed HAT-trie (Radix Directory + ArrayHash Leaves)  
**Dataset:** 100,000 Keys (Random Bytes vs URLs), 10,000 for Fuzzy Search.  
**Hardware:** Apple M-Series (ARM64)

## Executive Summary

The `hat-trie` library, with Path Compression enabled, provides a superior alternative to Rust's standard `BTreeMap` for most string-based workloads. The optimal `burst_threshold` depends heavily on the specific use case, offering a clear trade-off between write speed, read speed, and fuzzy search performance.

*   **`1024` threshold:** The best all-around performer, achieving near-parity with `BTreeMap` on URL insertions while dominating on random data writes. It is the clear winner for Fuzzy Search.
*   **`16384` threshold:** The read-performance champion. It provides the fastest lookups for structured data (URLs) by minimizing tree depth, making it ideal for static, read-heavy workloads.
*   **`512` threshold:** A specialized configuration for write-heavy structured data, achieving the fastest URL insertion times at the cost of slower random writes and iteration.

## Quick Summary Matrix

| Metric | HatTrie (512) | **HatTrie (1024)** | HatTrie (4096) | HatTrie (16k) | BTreeMap |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Random Insert** | 67.2 ms | 🚀 **13.0 ms** | 14.7 ms | 22.7 ms | 23.1 ms |
| **URL Insert** | 🚀 **23.0 ms** | 26.3 ms | 34.5 ms | 42.5 ms | 27.6 ms |
| **Random Get** | 8.8 ms | ✅ 5.3 ms | ✅ 5.3 ms | ✅ 5.4 ms | 15.3 ms |
| **URL Get** | 9.4 ms | 10.0 ms | 9.1 ms | 🚀 **8.1 ms** | 19.7 ms |
| **Streaming Iter** | 7.9 ms | **2.7 ms** | 🚀 **2.7 ms** | 🚀 **2.7 ms** | N/A |
| **Allocating Iter** | 12.5 ms | 🚀 **5.9 ms** | 6.0 ms | 6.0 ms | 🚀 **0.3 ms** |
| **Fuzzy-1 Search** | 🚀 **160 µs** | 🚀 **159 µs** | 516 µs | 633 µs | N/A |
| **Fuzzy-2 Search** | ✅ **968 µs** | ✅ **966 µs** | ✅ 972 µs | ✅ 971 µs | N/A |

*   **🚀**: Best in Class (among Ordered Maps)
*   **✅**: Excellent Performance (Beats BTreeMap)
*   **N/A**: Not Supported

---

## Detailed Analysis

### 1. Insertion Strategy (The "Radix" Win)
Path Compression has solved the URL insertion bottleneck. The choice of burst threshold now offers a clear trade-off:

*   **Random Data:** Small thresholds (like `512`) are slow because they force the trie to burst constantly, even for non-repeating prefixes. The **`1024`** threshold is the sweet spot, beating `BTreeMap` by ~40%.
*   **Structured Data (URLs):** A **`512`** threshold is fastest, even beating `BTreeMap`. By bursting very early, it aggressively creates `Radix` nodes for shared prefixes, minimizing the work done inside large leaf hash tables.

### 2. Lookup / Read Speed
`HatTrie` is consistently **2x to 3x faster than `BTreeMap`** for all lookup workloads.
*   **Random Data:** Performance is excellent across all thresholds from `1k` to `16k`. The `512` threshold is slightly slower due to increased tree depth.
*   **Structured Data (URLs):** The **`16384`** threshold is the undisputed winner. By keeping buckets large, it minimizes tree depth. A lookup for `https://www.google.com/search` might only involve one or two node traversals before hitting a massive, cache-hot `ArrayHash` leaf.

### 3. Fuzzy Search (Levenshtein)
Performance is inversely proportional to the burst threshold for small edit distances.
*   **Distance 1 (Fuzzy-1):** Small thresholds (`512`/`1024`) are **3-4x faster**. A granular tree with many small `Radix` and `Internal` nodes allows the search algorithm to prune invalid paths much earlier.
*   **Distance 2 (Fuzzy-2):** Performance is consistent across all thresholds. The search space grows exponentially, so the algorithm spends most of its time exploring valid paths rather than pruning invalid ones, making the tree structure less relevant.

### 4. Iteration & Range Queries
`BTreeMap` remains the winner due to its zero-allocation iteration.
*   **`HatTrie`** performance is consistent across `1k-16k` thresholds but degrades with the `512` threshold due to the overhead of traversing a much deeper tree.
*   The **`streaming_iter()`** provides a zero-allocation alternative that is **~2x faster** than the standard allocating `iter()`.

---

## Final Tuning Recommendations

### ✅ Default Choice: `1024` bytes
**Use for:** General purpose, dynamic workloads.
*   It offers the best balance: beats `BTreeMap` on random inserts, is competitive on URL inserts, has excellent lookup speed, and maintains top-tier fuzzy search performance.

### ⚡ Read-Heavy / Static Data: `16,384` bytes
**Use for:** Caches, Routing Tables, Dictionaries (Write-Once, Read-Many).
*   This configuration provides the **fastest possible `get()` performance** for structured keys by minimizing tree depth and maximizing the use of `ArrayHash` leaves.

### ✍️ Write-Heavy Structured Data: `512` bytes
**Use for:** Ingesting URLs, log parsing, filesystem indexing.
*   If your primary concern is the speed of inserting keys with long shared prefixes, this threshold offers the best performance, even surpassing `BTreeMap`. Be aware of the trade-off in slower reads and iteration.

### 🔍 Autocomplete / Search: `512` or `1024` bytes
**Use for:** Real-time search, spell-checking.
*   The deep, granular tree created by small burst thresholds provides the fastest fuzzy search performance by maximizing the algorithm's pruning opportunities, especially for small edit distances.

---
## Benchmark Details

*   **Insert:** Total time to insert 100,000 keys into an empty structure.
*   **Get:** Total time to perform 100,000 successful lookups with keys in random order.
*   **Iteration:** Total time to scan all 100,000 keys.
*   **Range:** Total time to seek to a midpoint and iterate the next 100 keys.
*   **Fuzzy Search:** Total time to find all keys within a Levenshtein distance of 1 or 2 from a single target key in a 10,000-item trie.