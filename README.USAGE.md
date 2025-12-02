# Usage Guide: hat-trie

This guide provides a detailed overview of the `hat-trie` library, covering core concepts, configuration, and advanced query capabilities for high-performance string management.

## Core Concepts

The **HAT-trie** is a hybrid data structure combining a **Radix Tree** (also known as a Path-Compressed Trie) for its directory and **ArrayHash** tables for its leaves.

1.  **Radix Tree Directory:** The upper levels of the structure use Radix nodes to collapse chains of single-child nodes. This means a path like `"https://www."` can be represented by a single node, drastically reducing memory usage and pointer chasing for keys with long shared prefixes.
2.  **ArrayHash Leaves:** The leaves of the tree are not single values but small, cache-conscious hash tables. This provides $O(1)$ average-case insertion and lookup speed within a leaf.
3.  **Bursting:** When a leaf `ArrayHash` grows beyond the configured `burst_threshold`, it "bursts." It is replaced by a new `Radix` node (representing the longest common prefix of the leaf's contents) and a new `Internal` (branching) node. This dynamic restructuring keeps the tree balanced and efficient.

## Quick Start

### Basic Operations
The API mimics standard Rust collections. You can insert, retrieve, mutate, and remove items using string-like keys.

```rust
use hattrie::HatTrie;

fn main() {
    // Initialize with a recommended threshold for general use
    let mut trie = HatTrie::with_threshold(1024);

    // Insert data
    trie.insert("apple", 1);
    trie.insert("apricot", 2);

    // Retrieve data
    assert_eq!(trie.get("apple"), Some(&1));

    // Remove data
    trie.remove("apple");
    assert_eq!(trie.get("apple"), None);
}
```

### Sorted Iteration
The trie maintains lexicographical order. The default `iter()` is convenient but involves heap allocations to reconstruct keys. For maximum performance, use one of the zero-allocation iterators.

```rust
// Convenient but slower (allocates a Vec<u8> for each key)
for (key, value) in trie.iter() {
    println!("{:?}: {}", String::from_utf8_lossy(&key), value);
}
```

## Configuration

The primary tuning knob for a HAT-trie is the **Burst Threshold**. This determines the maximum size (in bytes) of a leaf hash map before it splits.

*   **Small Threshold (e.g., 512 - 1024 bytes):** Creates a deeper, more granular tree. This is optimal for **write-heavy workloads** with structured data (like URLs) and provides the best performance for **fuzzy search**.
*   **Large Threshold (e.g., 16384 bytes):** Creates a very shallow tree with large leaf hash maps. This is optimal for **read-heavy workloads**, especially for structured data, as it minimizes tree traversal.

The recommended default is **1024 bytes**, offering a strong balance for most use cases.

```rust
// Create a trie optimized for fast insertions and fuzzy search
let write_optimized_trie = HatTrie::with_threshold(1024);

// Create a trie optimized for the fastest possible lookups on static data
let read_optimized_trie = HatTrie::with_threshold(16384);
```

## Working with Basic API

The core interaction happens through the `HatTrie` struct.

*   `fn insert<K: AsRef<[u8]>>(&mut self, key: K, value: V) -> Option<V>`
    Inserts a key-value pair, returning the old value if the key already existed.

*   `fn get<K: AsRef<[u8]>>(&self, key: K) -> Option<&V>`
    Returns a reference to the value corresponding to the key.

*   `fn get_mut<K: AsRef<[u8]>>(&mut self, key: K) -> Option<&mut V>`
    Returns a mutable reference to the value, allowing in-place modification.

*   `fn remove<K: AsRef<[u8]>>(&mut self, key: K) -> Option<V>`
    Removes the key from the trie and returns the value if it existed.

## Zero-Allocation Iteration

For performance-critical code that scans many items, use the zero-allocation iterators to avoid the overhead of key reconstruction. **These are 2-3x faster than the standard `iter()`**.

### Streaming Iterator
The `streaming_iter` is the easiest zero-allocation iterator to use. It yields a temporary view struct (`StreamItem`) on each step.

*   `fn streaming_iter(&self) -> HatTrieStreamingIter<V>`
    Returns a streaming iterator. The key slice yielded by `item.key()` is only valid for the current loop iteration.

```rust
let mut iter = trie.streaming_iter();
while let Some(item) = iter.next() {
    // item.key() is a &[u8] slice valid only in this scope
    println!("Key: {:?}, Value: {}", item.key(), item.value());
}
```

### Segment Iterator
The `iter_segments` method is the absolute fastest way to traverse the trie. It exposes the key's internal components (`path` and `suffix`) directly.

*   `fn iter_segments(&self) -> SegmentIter<V>`
    Returns a segment iterator.

```rust
let mut iter = trie.iter_segments();
while let Some(item) = iter.next() {
    let segments = item.segments();
    // segments.path and segments.suffix are &[u8] slices
    if segments.path == b"http://" {
        // ...
    }
}
```

## Range Queries and Cursors

`hat-trie` supports arbitrary range queries, making it suitable for database-like workloads.

*   `fn range<'a, R, K>(&'a self, range: R) -> impl Iterator`
    Returns an iterator over a subset of the map. Supports `..`, `start..`, `..end`, and `start..end` syntax. The iterator efficiently seeks to the start of the range.

*   `fn lower_bound<'a, K: AsRef<[u8]>>(&'a self, key: K) -> Option<(Vec<u8>, &'a V)>`
    Finds the first entry where the key is greater than or equal to the provided key. Useful for finding the insertion point or the "next" item.

*   `fn upper_bound<'a, K: AsRef<[u8]>>(&'a self, key: K) -> Option<(Vec<u8>, &'a V)>`
    Finds the first entry where the key is strictly greater than the provided key.

```rust
// Iterate over all keys starting with 'b' up to 'd' (exclusive)
for (key, val) in trie.range("b".."d") {
    // ...
}
```

## Specialized Features

### Prefix Scanning
To implement features like autocomplete, use `scan_prefix`. This is more efficient than a range query because it traverses directly to the subtree matching the prefix.

*   `fn scan_prefix<'a, K: AsRef<[u8]>>(&'a self, prefix: K) -> impl Iterator`
    Returns an iterator over all keys that start with the given prefix.

### Fuzzy Search
The library includes a Levenshtein distance searcher. It uses the trie structure to aggressively prune search paths that cannot possibly meet the distance criteria.

*   `fn fuzzy_search(&self, key: &[u8], max_dist: usize) -> Vec<(Vec<u8>, &V)>`
    Returns a list of all key-value pairs within `max_dist` edits (insertions, deletions, or substitutions) of the target key.

### Longest Common Prefix
Useful for routing tables or filesystem paths.

*   `fn longest_common_prefix<K: AsRef<[u8]>>(&self, key: K) -> (usize, Option<&V>)`
    Returns the length of the longest prefix of `key` found in the trie, and its value if that specific prefix is a stored key.

## Ecosystem Integration

The library implements standard Rust traits to integrate seamlessly with the ecosystem.

*   **`IntoIterator`**: Consumes the trie and moves values out.
*   **`FromIterator` / `Extend`**: Allows constructing or extending a `HatTrie` from iterators (e.g., `vec.into_iter().collect()`).
*   **`Index` trait**: Allows access via `trie["key"]` syntax (panics if missing).
*   **`Serde`**: With the `serde` feature enabled, `HatTrie` structures can be serialized and deserialized (e.g., to JSON or Bincode).

## Diagnostics

For performance tuning, you can inspect the internal structure of the tree.

*   `fn stats(&self) -> HatTrieStats`
    Returns a struct containing counts of internal nodes, leaf nodes, total items, and the maximum bucket size encountered. This is useful for determining if your `burst_threshold` is appropriate for your data distribution.