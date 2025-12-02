# API Reference: hat-trie

## 1. Introduction & Core Concepts

The `hat-trie` library provides a high-performance, cache-conscious ordered map implementation optimized for string (or byte-sequence) keys.

### Core Types
*   **`HatTrie<V>`**: The primary entry point. It is a key-value collection that behaves similarly to `std::collections::BTreeMap` but uses a Hashed Array Tree Trie structure for improved cache locality and reduced memory usage with string keys.
*   **Keys**: All keys in the structure are treated as byte sequences (`Vec<u8>`). Methods accept any type that implements `AsRef<[u8]>` (e.g., `String`, `&str`, `&[u8]`).
*   **Values**: The structure is generic over the value type `V`.

### Architectural Principles
*   **Sorted Order**: Iteration and range queries always yield items in strict lexicographical (byte-wise) order.
*   **Bursting**: The structure uses a "burst threshold" to determine when to split leaf buckets into new trie nodes. This is the primary configuration knob for tuning performance vs. memory usage.

---

## 2. Configuration & Constants

Configuration is handled primarily through the constructor.

### Public Constants

```rust
pub const DEFAULT_BURST_THRESHOLD: usize = 4096;
```
*   **Type**: `usize`
*   **Value**: `4096`
*   **Description**: The default size limit (in bytes) for a bucket before it "bursts" into a new internal trie node. This fits within a standard 4KB OS page.

---

## 3. Main Types

### `HatTrie<V>`

The primary container struct.

#### Constructors

```rust
pub fn new() -> Self
```
Creates a new, empty `HatTrie` with the `DEFAULT_BURST_THRESHOLD`.

```rust
pub fn with_threshold(bytes: usize) -> Self
```
Creates a new, empty `HatTrie` with a custom burst threshold.
*   `bytes`: The size limit in bytes for leaf buckets. Smaller values (e.g., 256) reduce memory usage for sparse data; larger values (e.g., 16384) improve linear scan speeds and cache locality.

#### Basic Operations (CRUD)

```rust
pub fn insert<K: AsRef<[u8]>>(&mut self, key: K, value: V) -> Option<V>
```
Inserts a key-value pair into the trie.
*   `key`: The key to insert.
*   `value`: The value to associate with the key.
*   **Returns**: `Some(old_value)` if the key was already present, otherwise `None`.

```rust
pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Option<&V>
```
Returns a reference to the value corresponding to the key.
*   `key`: The key to look up.
*   **Returns**: `Some(&value)` if found, `None` otherwise.

```rust
pub fn get_mut<K: AsRef<[u8]>>(&mut self, key: K) -> Option<&mut V>
```
Returns a mutable reference to the value corresponding to the key.
*   `key`: The key to look up.
*   **Returns**: `Some(&mut value)` if found, `None` otherwise.

```rust
pub fn remove<K: AsRef<[u8]>>(&mut self, key: K) -> Option<V>
```
Removes a key from the trie, returning the value if the key was previously in the map.
*   `key`: The key to remove.
*   **Returns**: `Some(value)` if the key existed, `None` otherwise.

#### Status & Attributes

```rust
pub fn len(&self) -> usize
```
Returns the number of elements in the trie.

```rust
pub fn is_empty(&self) -> usize
```
Returns `true` if the trie contains no elements.

#### Iteration

```rust
pub fn iter(&self) -> HatTrieIter<V>
```
Returns an iterator over the entries of the trie, sorted by key.
*   **Yields**: `(Vec<u8>, &V)`

```rust
pub fn streaming_iter(&self) -> HatTrieStreamingIter<V>
```
Returns a high-performance streaming iterator that avoids heap allocations.
*   **Description**: This is significantly faster for full-table scans than `iter()`. It reuses an internal buffer for key reconstruction. The returned `StreamItem`'s key slice is only valid for the duration of one loop iteration and should not be stored.
*   **Yields**: `StreamItem`, which provides `.key()` and `.value()` methods.

```rust
pub fn iter_segments(&self) -> SegmentIter<V>
```
Returns the fastest possible iterator, which is a zero-allocation "segment" iterator.
*   **Description**: Instead of reconstructing a full key, this iterator yields a `SegmentItem` which provides access to the key's constituent parts: the prefix path from the trie's directory and the suffix stored in a leaf node. The yielded item and its slices are only valid until the next call to `next()`.
*   **Yields**: `SegmentItem`, which provides `.segments()` and `.value()` methods.

#### Advanced Queries

```rust
pub fn range<'a, R, K>(&'a self, range: R) -> impl Iterator<Item = (Vec<u8>, &'a V)>
where
    R: RangeBounds<K>,
    K: AsRef<[u8]> + ?Sized
```
Returns an iterator over a sub-range of elements in the trie. This operation efficiently "seeks" to the start of the range.
*   `range`: A `RangeBounds` object (e.g., `start..end`, `start..`, `..end`).
*   **Yields**: `(Vec<u8>, &V)`

```rust
pub fn lower_bound<'a, K: AsRef<[u8]>>(&'a self, key: K) -> Option<(Vec<u8>, &'a V)>
```
Finds the first key-value pair where the key is greater than or equal to the provided key.
*   `key`: The probe key.
*   **Returns**: `Some((key, value))` of the lower bound, or `None` if all keys in the trie are smaller.

```rust
pub fn upper_bound<'a, K: AsRef<[u8]>>(&'a self, key: K) -> Option<(Vec<u8>, &'a V)>
```
Finds the first key-value pair where the key is strictly greater than the provided key.
*   `key`: The probe key.
*   **Returns**: `Some((key, value))` of the upper bound, or `None`.

```rust
pub fn scan_prefix<'a, K: AsRef<[u8]>>(&'a self, prefix: K) -> impl Iterator<Item = (Vec<u8>, &'a V)>
```
Returns an iterator over all entries whose keys start with the given prefix. This is more efficient than a range query as it traverses directly to the relevant subtree.
*   `prefix`: The byte sequence to match.
*   **Yields**: `(Vec<u8>, &V)`

```rust
pub fn fuzzy_search(&self, key: &[u8], max_dist: usize) -> Vec<(Vec<u8>, &V)>
```
Performs an approximate search using Levenshtein distance.
*   `key`: The target pattern to match against.
*   `max_dist`: The maximum allowed edits (insertions, deletions, substitutions).
*   **Returns**: A `Vec` containing all matching key-value pairs.

```rust
pub fn longest_common_prefix<K: AsRef<[u8]>>(&self, key: K) -> (usize, Option<&V>)
```
Walks the trie using the provided key and finds the longest prefix of that key that exists as a stored key in the trie.
*   `key`: The input key to match against the trie's contents.
*   **Returns**: A tuple `(length, Option<&value>)`. `length` is the length of the matching prefix in bytes.

#### Diagnostics

```rust
pub fn stats(&self) -> HatTrieStats
```
Returns statistics about the internal structure of the trie.

---

### `HatTrieStats`

A struct containing diagnostic information about the trie's memory layout.

#### Public Fields

*   `pub internal_nodes: usize`: The count of pointer-based trie nodes.
*   `pub leaf_nodes: usize`: The count of bucket nodes (flat arrays).
*   `pub total_items: usize`: The total number of items stored.
*   `pub max_bucket_size: usize`: The number of items in the most populated bucket.

---

## 4. Iterators

### `HatTrieIter<'a, V>`

The iterator produced by `HatTrie::iter()`. It yields borrowed values.

*   **Implements**: `Iterator<Item = (Vec<u8>, &'a V)>`

### `HatTrieIntoIter<V>`

The consuming iterator produced by `HatTrie::into_iter()`. It yields owned values.

*   **Implements**: `Iterator<Item = (Vec<u8>, V)>`

### `HatTrieStreamingIter<'a, V>`

A high-performance iterator produced by `HatTrie::streaming_iter()`. This iterator should be consumed with a `while let` loop.

```rust
// Usage:
let mut iter = trie.streaming_iter();
while let Some(item) = iter.next() {
    let key_slice = item.key();
    let value_ref = item.value();
    // ...
}
```
*   **`next(&mut self) -> Option<StreamItem<'_, 'a, V>>`**: Advances the iterator and returns a view to the next item.

### `StreamItem<'iter, 'a: 'iter, V>`

A temporary view of an item yielded by the `HatTrieStreamingIter`.
*   **`key(&self) -> &'iter [u8]`**: Returns the key for the current item as a byte slice.
*   **`value(&self) -> &'a V`**: Returns a reference to the value for the current item.

### `SegmentIter<'a, V>`

A zero-allocation iterator produced by `HatTrie::iter_segments()`. This iterator should be consumed with a `while let` loop.

```rust
// Usage:
let mut iter = trie.iter_segments();
while let Some(item) = iter.next() {
    let segments = item.segments();
    // segments.path and segments.suffix are &[u8]
    // ...
}
```
*   **`next(&mut self) -> Option<SegmentItem<'_, 'a, V>>`**: Advances the iterator and returns a view to the next item's segments.

### `SegmentItem<'iter, 'a: 'iter, V>`

A temporary view of a key's components and its value, yielded by `SegmentIter`.
*   **`segments(&self) -> KeySegments<'_>`**: Returns the key's components for the current item.
*   **`value(&self) -> &'a V`**: Returns a reference to the value for the current item.

### `KeySegments<'a>`

A view of a key's constituent parts.
*   **`pub path: &'a [u8]`**: A slice representing the prefix of the key from the trie's directory path.
*   **`pub suffix: &'a [u8]`**: A slice representing the suffix of the key from the leaf node.

---

## 5. Trait Implementations

The `HatTrie` struct implements the following standard Rust traits:

### `std::default::Default`
```rust
fn default() -> Self
```
Creates a `HatTrie` with the default burst threshold.

### `std::clone::Clone`
```rust
fn clone(&self) -> Self
```
Creates a deep copy of the trie. Available only if `V` implements `Clone`.

### `std::cmp::PartialEq` / `std::cmp::Eq`
```rust
fn eq(&self, other: &Self) -> bool
```
Checks for logical equality. Two tries are considered equal if they contain the exact same key-value pairs in the same order, regardless of internal structural differences (e.g., different burst thresholds).

### `std::iter::IntoIterator`
```rust
fn into_iter(self) -> HatTrieIntoIter<V>
```
Consumes the trie and returns an iterator over owned key-value pairs.

### `std::iter::FromIterator`
```rust
fn from_iter<T>(iter: T) -> Self
```
Creates a `HatTrie` from an iterator of `(String, V)` tuples. Uses default configuration.

### `std::iter::Extend`
```rust
fn extend<T>(&mut self, iter: T)
```
Inserts all items from an iterator of `(String, V)` tuples into the existing trie.

### `std::ops::Index`
```rust
fn index(&self, index: &str) -> &V
```
Allows accessing values using the bracket syntax `trie["key"]`. Panics if the key is not found.

---

## 6. Error Handling

The API primarily uses the `Option` type for error handling:
*   **Lookups**: `get`, `get_mut`, `lower_bound`, etc., return `None` if the key is not found.
*   **Insertion**: `insert` returns `None` if the key was new, or `Some(old_value)` if a value was overwritten.
*   **Removal**: `remove` returns `None` if the key did not exist.

There are no custom `Result` types or public Error enums defined in the library's runtime API. Panics may occur only if using the `Index` trait (`trie["missing"]`).