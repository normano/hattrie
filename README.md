# hat-trie

[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-brightgreen.svg)](https://opensource.org/licenses/MPL-2.0)
[![Crates.io](https://img.shields.io/crates/v/hat-trie?label=hat-trie)](https://crates.io/crates/hat-trie)

**hat-trie** is a high-performance, cache-conscious, ordered string map implementation for Rust. It addresses the performance bottlenecks of traditional Tries and the memory overhead of standard HashMaps by combining a Trie-based index with flat, cache-friendly array buckets. This structure allows for fast, sorted access to variable-length string keys while minimizing pointer chasing and memory fragmentation.

## Key Features

### Cache-Conscious Architecture
Unlike standard Tries or Burst-tries that rely on linked lists, the HAT-trie (Hashed Array Trie) implements leaf nodes as contiguous byte arrays. This design maximizes CPU cache locality during searches and iteration, significantly reducing cache misses on modern processors.

### Sorted Access and Iteration
The structure maintains keys in strict lexicographical order. It supports efficient forward iteration, allowing the data structure to serve as a drop-in replacement for `BTreeMap` in scenarios where string keys are dominant.

### Advanced Range Queries and Cursors
Beyond simple lookups, the library provides database-grade query capabilities. Users can perform arbitrary range queries (unbounded, inclusive, or exclusive), seek specific lower/upper bounds, and efficiently scan prefixes.

### Built-in Fuzzy Search
The library includes a highly optimized Levenshtein distance searcher. It utilizes the Trie structure to prune search branches early, allowing for efficient approximate string matching (fuzzy search) across large datasets.

### Configurable Memory Tuning
Developers can tune the "burst threshold"—the size limit at which a bucket splits into a new trie node. This allows for precise control over the trade-off between memory usage (larger buckets) and search speed (deeper trees).

## Installation

Add `hat-trie` to your `Cargo.toml` dependencies:

```toml
[dependencies]
hat-trie = "0.9.0"
```

To enable serialization support via `serde`, add the feature flag:

```toml
[dependencies]
hat-trie = { version = "0.9.0", features = ["serde"] }
```

## Getting Started

To begin using the library, simply create a new instance and start inserting keys.

```rust
use hattrie::HatTrie;

fn main() {
    let mut trie = HatTrie::new();
    trie.insert("key", 42);
    
    if let Some(val) = trie.get("key") {
        println!("Found: {}", val);
    }
}
```

For a detailed guide on advanced queries, configuration, and API overview, please see the [Usage Guide](README.USAGE.md).