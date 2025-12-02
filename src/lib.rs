mod bucket;
mod fuzzy;
mod leaf;
mod node;

use node::Node;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// Tuning parameter: When a bucket exceeds this many bytes, burst it.
const DEFAULT_BURST_THRESHOLD: usize = 1024;

#[derive(Debug, Clone, Default)]
pub struct HatTrieStats {
  pub internal_nodes: usize,
  pub leaf_nodes: usize,
  pub total_items: usize,
  pub max_bucket_size: usize,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HatTrie<V> {
  root: Node<V>,
  len: usize,
  burst_threshold: usize,
}

impl<V> Default for HatTrie<V> {
  fn default() -> Self {
    Self {
      root: Node::default(),
      len: 0,
      burst_threshold: DEFAULT_BURST_THRESHOLD,
    }
  }
}

impl<V> HatTrie<V> {
  pub fn new() -> Self {
    Self::default()
  }

  /// Creates a new HAT-trie with a custom burst threshold in bytes.
  ///
  /// Smaller thresholds (e.g., 512, 1024) save memory for sparse data but increase CPU usage.
  /// Larger thresholds (e.g., 8192, 16384) improve cache locality for linear scans but
  /// may waste memory if buckets are partially full.
  pub fn with_threshold(bytes: usize) -> Self {
    Self {
      root: Node::default(),
      len: 0,
      burst_threshold: bytes,
    }
  }

  pub fn insert<K: AsRef<[u8]>>(&mut self, key: K, value: V) -> Option<V> {
    let result = self.root.insert(key.as_ref(), value, self.burst_threshold);
    // Only increment length if we inserted a NEW item (result is None)
    if result.is_none() {
      self.len += 1;
    }

    result
  }

  pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Option<&V> {
    self.root.get(key.as_ref())
  }

  pub fn get_mut<K: AsRef<[u8]>>(&mut self, key: K) -> Option<&mut V> {
    self.root.get_mut(key.as_ref())
  }

  pub fn remove<K: AsRef<[u8]>>(&mut self, key: K) -> Option<V> {
    let result = self.root.remove(key.as_ref());
    if result.is_some() {
      self.len -= 1;
    }
    result
  }

  pub fn len(&self) -> usize {
    self.len
  }

  pub fn is_empty(&self) -> bool {
    self.len == 0
  }

  /// Returns an iterator over the entries sorted by key (bytes).
  pub fn iter(&self) -> HatTrieIter<'_, V> {
    HatTrieIter::new(&self.root, Vec::new())
  }

  /// Returns a streaming iterator that avoids allocations by yielding borrowed key slices.
  /// The key slice is only valid until the next call to `next_item()`.
  pub fn streaming_iter(&self) -> HatTrieStreamingIter<V> {
    HatTrieStreamingIter::new(&self.root)
  }

  /// Returns a zero-allocation "segment" iterator.
  ///
  /// This is the fastest method for iterating over all entries. Instead of
  /// reconstructing and returning an owned `Vec<u8>` key on each step, this
  /// iterator yields a `KeySegments` struct, which contains borrowed slices
  /// pointing to the key's prefix and suffix components.
  ///
  /// The yielded `KeySegments` and its slices are only valid until the next
  /// call to `next()`.
  ///
  /// # Example
  /// ```ignore
  /// let mut iter = trie.iter_segments();
  /// while let Some((segments, value)) = iter.next() {
  ///     // segments.path is the prefix from the trie path
  ///     // segments.suffix is the part from the leaf bucket
  ///     if segments.path == b"http://" && segments.suffix.starts_with(b"google") {
  ///         // ...
  ///     }
  ///
  ///     // To get an owned key, you must allocate it yourself.
  ///     let full_key = [segments.path, segments.suffix].concat();
  /// }
  /// ```
  pub fn iter_segments(&self) -> SegmentIter<V> {
    SegmentIter::new(&self.root)
  }

  // --- ADVANCED TRIE FEATURES ---

  /// Returns an iterator over all entries whose keys start with the given prefix.
  pub fn scan_prefix<'a, K: AsRef<[u8]>>(
    &'a self,
    prefix: K,
  ) -> impl Iterator<Item = (Vec<u8>, &'a V)> {
    let p = prefix.as_ref();
    let mut node = &self.root;
    let mut depth = 0;
    let mut found = true; // Flag to track if the prefix path exists

    // 1. Traverse to the node representing the prefix
    while depth < p.len() {
      match node {
        Node::Internal { children, .. } => match &children[p[depth] as usize] {
          Some(child) => {
            node = child;
            depth += 1;
          }
          None => {
            found = false;
            break;
          }
        },
        Node::Radix {
          prefix: r_prefix,
          child,
        } => {
          let suffix = &p[depth..];
          let common = common_prefix_len(suffix, r_prefix);

          if common == suffix.len() {
            // The remaining query prefix matches the start of this node's prefix.
            // We found the valid subtree.
            // Stop descending; the iterator will start here.
            // (The iterator will naturally traverse the full Radix node)
            break;
          } else if common == r_prefix.len() {
            // This node's prefix is fully contained in the query.
            // Consume this node and continue descending.
            node = child;
            depth += r_prefix.len();
          } else {
            // Mismatch
            found = false;
            break;
          }
        }
        Node::Leaf(_) => break, // Hit a bucket before prefix finished, stop and filter there
      }
    }

    // 2. initialize iterator
    // If not found, we create an empty iterator.
    // If found, we create a real one starting at 'node'.
    let iter = if found {
      // We pass the prefix bytes we actually consumed (depth)
      HatTrieIter::new(node, p[0..depth].to_vec())
    } else {
      HatTrieIter::empty()
    };

    // 3. Apply Filter
    // We now apply .filter() to the 'iter' regardless of whether it's empty or full.
    // This ensures the return type is ALWAYS `Filter<HatTrieIter, Closure>`.
    let filter_prefix = p.to_vec();
    iter.filter(move |(k, _)| k.starts_with(&filter_prefix))
  }

  /// Finds the value associated with the longest prefix of `key`.
  /// Returns `(length_of_match, Option<&Value>)`.
  pub fn longest_common_prefix<K: AsRef<[u8]>>(&self, key: K) -> (usize, Option<&V>) {
    let k = key.as_ref();
    let mut node = &self.root;
    let mut depth = 0;
    let mut last_match = (0, None);

    loop {
      match node {
        Node::Internal { children, value } => {
          // Check if this internal node has a value (exact match at this depth)
          if let Some(v) = value {
            last_match = (depth, Some(v));
          }

          if depth == k.len() {
            break;
          } // Exact match of full key

          match &children[k[depth] as usize] {
            Some(child) => {
              node = child;
              depth += 1;
            }
            None => break, // No further path
          }
        }
        Node::Radix { prefix, child } => {
          // Check how much of the key matches this prefix
          let suffix = &k[depth..];
          let common = common_prefix_len(suffix, prefix);

          // Radix nodes don't usually store values themselves (values are in Internal nodes),
          // but if we consumed some bytes, we advance.
          depth += common;

          if common == prefix.len() {
            // Full match, continue
            node = child;
          } else {
            // Partial match, we are done
            break;
          }
        }
        Node::Leaf(bucket) => {
          // Check inside bucket for the longest match.
          // Since the bucket contains suffixes, we are looking for a key in the bucket
          // that is a prefix of `k[depth..]`.
          // Buckets store exact keys usually.
          // But if `k` extends past a key in the bucket?
          // Example: Trie has "foo". Query "foobar".
          // "foo" is in a bucket.
          // We need to iterate bucket keys and see if any is a prefix of `k[depth..]`.
          // Since we want the longest, and bucket is sorted, we can scan.
          let suffix = &k[depth..];

          // We search for the longest key in bucket that matches the start of `suffix`.
          // Optimization: We could binary search, but linear scan is fine for buckets.
          let mut best_len = 0;
          let mut best_val = None;

          for (b_suffix, val) in bucket.iter() {
            if suffix.starts_with(b_suffix) {
              if b_suffix.len() > best_len {
                best_len = b_suffix.len();
                best_val = Some(val);
              }
            }
          }

          if let Some(v) = best_val {
            return (depth + best_len, Some(v));
          }
          break;
        }
      }
    }
    last_match
  }

  /// Returns an iterator over the range.
  ///
  /// # Complexity
  /// - Start: O(k) to seek to the beginning (where k is key length).
  /// - End: Checked lazily during iteration.
  pub fn range<'a, R, K>(&'a self, range: R) -> impl Iterator<Item = (Vec<u8>, &'a V)>
  where
    R: RangeBounds<K>,
    K: AsRef<[u8]> + ?Sized,
  {
    let start_bytes = match range.start_bound() {
      Bound::Included(k) => Some(k.as_ref()),
      Bound::Excluded(k) => Some(k.as_ref()),
      Bound::Unbounded => None,
    };

    let iter = HatTrieIter::seek(&self.root, start_bytes);

    // Prepare Start Filter (Only for Excluded bounds)
    // We need to filter out the *exact* match of the start bound if it exists.
    // This only ever affects the very first item (since keys are unique).
    let start_filter = match range.start_bound() {
      Bound::Excluded(k) => Some(k.as_ref().to_vec()),
      _ => None,
    };

    // Prepare End Filter
    // We clone these to move them into the closure.
    let end_bound = match range.end_bound() {
      Bound::Included(k) => Bound::Included(k.as_ref().to_vec()),
      Bound::Excluded(k) => Bound::Excluded(k.as_ref().to_vec()),
      Bound::Unbounded => Bound::Unbounded,
    };

    // OPTIMIZATION:
    // 1. Filter the start (cheap, usually passes immediately).
    // 2. Take While the end condition is met (stops iteration early).
    iter
      .filter(move |(k, _)| {
        if let Some(ex) = &start_filter {
          if k == ex {
            return false;
          }
        }
        true
      })
      .take_while(move |(k, _)| match &end_bound {
        Bound::Included(end) => k <= end,
        Bound::Excluded(end) => k < end,
        Bound::Unbounded => true,
      })
  }

  /// Returns the first key-value pair where key >= target.
  pub fn lower_bound<'a, K: AsRef<[u8]>>(&'a self, key: K) -> Option<(Vec<u8>, &'a V)> {
    let mut iter = HatTrieIter::seek(&self.root, Some(key.as_ref()));
    iter.next()
  }

  /// Returns the first key-value pair where key > target.
  pub fn upper_bound<'a, K: AsRef<[u8]>>(&'a self, key: K) -> Option<(Vec<u8>, &'a V)> {
    // Upper bound is essentially lower_bound, but skip exact match.
    let mut iter = HatTrieIter::seek(&self.root, Some(key.as_ref()));
    let first = iter.next()?;
    if first.0 == key.as_ref() {
      iter.next()
    } else {
      Some(first)
    }
  }

  pub fn fuzzy_search(&self, key: &[u8], max_dist: usize) -> Vec<(Vec<u8>, &V)> {
    let searcher = fuzzy::FuzzySearcher::new(key, max_dist);
    searcher.search(&self.root)
  }

  // --- DIAGNOSTICS ---

  pub fn stats(&self) -> HatTrieStats {
    let mut stats = HatTrieStats::default();
    self.root.collect_stats(&mut stats);
    stats
  }
}

// --- RUST TRAITS ---

impl<V> FromIterator<(String, V)> for HatTrie<V> {
  fn from_iter<T: IntoIterator<Item = (String, V)>>(iter: T) -> Self {
    let mut trie = HatTrie::default();
    for (k, v) in iter {
      trie.insert(k, v);
    }
    trie
  }
}

impl<V> Extend<(String, V)> for HatTrie<V> {
  fn extend<T: IntoIterator<Item = (String, V)>>(&mut self, iter: T) {
    for (k, v) in iter {
      self.insert(k, v);
    }
  }
}

use std::ops::{Bound, Index, RangeBounds};

use crate::node::common_prefix_len;
impl<V> Index<&str> for HatTrie<V> {
  type Output = V;
  fn index(&self, index: &str) -> &Self::Output {
    self.get(index).expect("no entry found for key")
  }
}

// --- EQUALITY ---

// Manual implementation to ensure Tries with different thresholds
// but identical content are considered equal.
impl<V: PartialEq> PartialEq for HatTrie<V> {
  fn eq(&self, other: &Self) -> bool {
    if self.len() != other.len() {
      return false;
    }
    // Since iteration is deterministic and sorted, strict sequence comparison works.
    self.iter().eq(other.iter())
  }
}

impl<V: Eq> Eq for HatTrie<V> {}

// --- Iterator Implementation ---

/// An iterator that yields key-value pairs in lexicographical order.
/// It maintains a stack of nodes to traverse the trie depth-first.
pub struct HatTrieIter<'a, V> {
  // Stack items: (Node reference, Child Index / Bucket Iterator)
  stack: Vec<IterFrame<'a, V>>,
  prefix_buffer: Vec<u8>,
}

enum IterFrame<'a, V> {
  Internal {
    node: &'a Node<V>,
    child_idx: usize,
    yielded_value: bool,
    byte_on_stack: bool,
  },
  Radix {
    node: &'a Node<V>, // Holds the prefix
    visited: bool,     // Have we recursed yet?
  },
  Leaf {
    items: std::vec::IntoIter<(&'a [u8], &'a V)>,
  },
}

impl<'a, V> IterFrame<'a, V> {
  fn node(&self) -> &'a Node<V> {
    match self {
      IterFrame::Internal { node, .. } | IterFrame::Radix { node, .. } => *node,
      IterFrame::Leaf { .. } => panic!("Cannot get node from Leaf frame"),
    }
  }
}

impl<'a, V> HatTrieIter<'a, V> {
  fn new(root: &'a Node<V>, initial_prefix: Vec<u8>) -> Self {
    let mut iter = Self {
      stack: Vec::with_capacity(8),
      prefix_buffer: initial_prefix,
    };
    iter.push_node(root, None); // Normal push
    iter
  }

  fn empty() -> Self {
    Self {
      stack: vec![],
      prefix_buffer: vec![],
    }
  }

  /// Seek constructor
  fn seek(root: &'a Node<V>, start_key: Option<&[u8]>) -> Self {
    if start_key.is_none() {
      return Self::new(root, Vec::new());
    }
    let key = start_key.unwrap();

    let mut iter = Self {
      stack: Vec::with_capacity(8),
      prefix_buffer: Vec::new(),
    };

    // We traverse down matching the key.
    // If we diverge, we need to push the "next" siblings to the stack.
    iter.seek_recursive(root, key, 0);
    iter
  }

  fn seek_recursive(&mut self, node: &'a Node<V>, target: &[u8], depth: usize) {
    match node {
      Node::Internal { children, value: _ } => {
        let should_yield_value = if depth < target.len() { false } else { true };
        let next_byte = if depth < target.len() {
          target[depth]
        } else {
          0
        };
        let next_idx = next_byte as usize;

        let frame_child_idx = if depth < target.len() && children[next_idx].is_some() {
          next_idx + 1
        } else if depth == target.len() {
          0
        } else {
          next_idx
        };

        // If we are descending, we MUST mark that the byte is on the stack
        // so the Internal frame knows to pop it when it eventually resumes.
        let is_descending = depth < target.len() && children[next_idx].is_some();

        self.stack.push(IterFrame::Internal {
          node,
          child_idx: frame_child_idx,
          yielded_value: !should_yield_value,
          byte_on_stack: is_descending, // <--- CRITICAL
        });

        if is_descending {
          if let Some(child) = &children[next_idx] {
            self.prefix_buffer.push(next_byte);
            self.seek_recursive(child, target, depth + 1);
          }
        }
      }
      Node::Radix { prefix, child } => {
        // Seek Logic for Radix
        // Compare prefix with target[depth..]

        // How much of the target remains?
        let target_suffix = if depth < target.len() {
          &target[depth..]
        } else {
          &[]
        };

        let common = common_prefix_len(prefix, target_suffix);

        // Decisions:
        // 1. Exact match or Target is extension of Prefix (common == prefix.len)
        //    -> Recurse into child.
        // 2. Prefix > Target (lexicographically)
        //    -> Target falls "before" this node's contents.
        //    -> We should iterate this whole node.
        // 3. Prefix < Target
        //    -> Target falls "after" this node.
        //    -> Skip this node entirely. (Return, stack unrolls to parent).

        if common == prefix.len() {
          // Match! Recurse.
          self.stack.push(IterFrame::Radix {
            node,
            visited: true,
          });
          self.prefix_buffer.extend_from_slice(prefix);
          self.seek_recursive(child, target, depth + prefix.len());
        } else {
          // Mismatch.
          // Comparison between the first differing byte
          let p_byte = prefix[common]; // Exists because common < prefix.len
          let t_byte = if common < target_suffix.len() {
            Some(target_suffix[common])
          } else {
            None // Target ran out, so Target is a prefix of Radix. Target < Radix.
          };

          match t_byte {
            Some(tb) => {
              if p_byte > tb {
                // Prefix is "greater" than target.
                // We are seeking the first key >= target.
                // Since this node > target, it IS the lower bound.
                // Iterate it fully.
                self.push_node(node, None);
              } else {
                // Prefix < target.
                // This node is entirely before the target. Skip it.
                // Do nothing.
              }
            }
            None => {
              // Target is shorter: "app" vs "apple".
              // "apple" > "app". This node is valid.
              self.push_node(node, None);
            }
          }
        }
      }
      Node::Leaf(_) => {
        let suffix = if depth < target.len() {
          Some(&target[depth..])
        } else {
          None
        };
        self.push_node(node, suffix);
      }
    }
  }

  fn push_node(&mut self, node: &'a Node<V>, seek_start: Option<&[u8]>) {
    match node {
      Node::Internal { .. } => {
        self.stack.push(IterFrame::Internal {
          node,
          child_idx: 0,
          yielded_value: false,
          byte_on_stack: false,
        });
      }
      Node::Radix { .. } => {
        self.stack.push(IterFrame::Radix {
          node,
          visited: false,
        });
      }
      Node::Leaf(leaf) => {
        // Collect and Sort
        let mut items: Vec<_> = leaf.iter().collect();
        items.sort_unstable_by(|a, b| a.0.cmp(b.0));

        // If seeking, skip items < suffix
        let iter = if let Some(suffix) = seek_start {
          // Optimization: binary search or skip_while
          // Since it's a vector now, we can binary search the start index
          let start_idx = items.partition_point(|(k, _)| *k < suffix);
          let mut vec_iter = items.into_iter();
          if start_idx > 0 {
            // Advance iterator (O(N) unfortunately for IntoIter, but simple)
            // Slice::into_iter doesn't exist for Refs, we have IntoIter over refs.
            for _ in 0..start_idx {
              vec_iter.next();
            }
          }
          vec_iter
        } else {
          items.into_iter()
        };

        self.stack.push(IterFrame::Leaf { items: iter });
      }
    }
  }
}

enum IterAction<'a, V> {
  Yield(Vec<u8>, &'a V),
  PushLeaf, // Already handled inside get_action logic but good for symmetry
  PushRadixChild(&'a Node<V>, &'a [u8]), // Child Node, Prefix to extend
  PopRadix(usize), // Len to truncate
  PushInternalChild(&'a Node<V>, u8), // Child Node, byte to push
  PopInternalByte,
  PopFrame,
  None,
}

impl<'a, V> Iterator for HatTrieIter<'a, V> {
  type Item = (Vec<u8>, &'a V);

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      // 1. Inspect the top of the stack to determine action
      let action = match self.stack.last_mut() {
        Some(IterFrame::Leaf { items }) => {
          if let Some((suffix, val)) = items.next() {
            let mut full = self.prefix_buffer.clone();
            full.extend_from_slice(suffix);
            IterAction::Yield(full, val)
          } else {
            IterAction::PopFrame
          }
        }
        Some(IterFrame::Radix { node, visited }) => {
          if !*visited {
            *visited = true;
            if let Node::Radix { prefix, child } = node {
              // Copy ref out to avoid borrow issue
              IterAction::PushRadixChild(child, prefix)
            } else {
              unreachable!()
            }
          } else {
            if let Node::Radix { prefix, .. } = node {
              IterAction::PopRadix(prefix.len())
            } else {
              unreachable!()
            }
          }
        }
        Some(IterFrame::Internal {
          node,
          child_idx,
          yielded_value,
          byte_on_stack,
        }) => {
          // Logic:
          // 1. If byte_on_stack, we returned from a child. Need to pop byte first.
          // 2. If value not yielded, yield value.
          // 3. Find next child.

          if *byte_on_stack {
            *byte_on_stack = false;
            IterAction::PopInternalByte
          } else if let Node::Internal { children, value } = node {
            if !*yielded_value {
              *yielded_value = true;
              if let Some(v) = value {
                IterAction::Yield(self.prefix_buffer.clone(), v)
              } else {
                IterAction::None // Loop again to find child
              }
            } else {
              // Find next child
              let mut next_action = IterAction::PopFrame; // Default if exhausted

              while *child_idx < 256 {
                let idx = *child_idx;
                *child_idx += 1;
                if let Some(child) = &children[idx] {
                  *byte_on_stack = true; // Mark that we are pushing a byte
                  next_action = IterAction::PushInternalChild(child, idx as u8);
                  break;
                }
              }
              next_action
            }
          } else {
            unreachable!()
          }
        }
        None => return None,
      };

      // 2. Perform the action (mutable access to self allowed here)
      match action {
        IterAction::Yield(k, v) => return Some((k, v)),
        IterAction::PopFrame => {
          self.stack.pop();
        }
        IterAction::None => continue,

        // Radix Actions
        IterAction::PushRadixChild(child, prefix) => {
          self.prefix_buffer.extend_from_slice(prefix);
          self.push_node(child, None);
        }
        IterAction::PopRadix(len) => {
          self.stack.pop();
          let new_len = self.prefix_buffer.len() - len;
          self.prefix_buffer.truncate(new_len);
        }

        // Internal Actions
        IterAction::PopInternalByte => {
          self.prefix_buffer.pop();
        }
        IterAction::PushInternalChild(child, byte) => {
          self.prefix_buffer.push(byte);
          self.push_node(child, None);
        }
        _ => {}
      }
    }
  }
}

// --- CONSUMING ITERATOR (IntoIter) ---

impl<V> IntoIterator for HatTrie<V> {
  type Item = (Vec<u8>, V);
  type IntoIter = HatTrieIntoIter<V>;

  fn into_iter(self) -> Self::IntoIter {
    HatTrieIntoIter::new(self.root)
  }
}

pub struct HatTrieIntoIter<V> {
  stack: Vec<IntoIterFrame<V>>,
  prefix_buffer: Vec<u8>,
}

enum IntoIterFrame<V> {
  Internal {
    children: std::vec::IntoIter<Option<Node<V>>>,
    child_idx: usize, // Tracks the index (0..255) for the current child
    value: Option<V>,
    byte_on_stack: bool, // NEW: Explicitly track if we pushed to prefix_buffer
  },
  Radix {
    prefix: Vec<u8>,
    child: Box<Node<V>>,
    visited: bool,
  },
  Leaf {
    items: std::vec::IntoIter<(Vec<u8>, V)>,
  },
}

// Helper enum to circumvent borrow checker rules during match
enum IntoIterAction<V> {
  Yield(Vec<u8>, V),
  PopFrame,

  // Radix Ops
  PushRadixChild(Vec<u8>, Node<V>),
  PopRadixPrefix(usize),

  // Internal Ops
  PushInternalChild(u8, Node<V>),
  PopInternalByte,

  None, // Used to continue the loop
}

impl<V> HatTrieIntoIter<V> {
  fn new(root: Node<V>) -> Self {
    let mut iter = Self {
      stack: Vec::with_capacity(8),
      prefix_buffer: Vec::new(),
    };
    iter.push_node(root);
    iter
  }

  fn push_node(&mut self, node: Node<V>) {
    match node {
      Node::Internal { children, value } => {
        let children_vec = (children as Box<[Option<Node<V>>]>).into_vec();
        self.stack.push(IntoIterFrame::Internal {
          children: children_vec.into_iter(),
          child_idx: 0,
          value,
          byte_on_stack: false, // Init false
        });
      }
      Node::Radix { prefix, child } => {
        self.stack.push(IntoIterFrame::Radix {
          prefix,
          child,
          visited: false,
        });
      }
      Node::Leaf(mut leaf) => {
        let mut items: Vec<_> = leaf.drain_all().collect();
        items.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        self.stack.push(IntoIterFrame::Leaf {
          items: items.into_iter(),
        });
      }
    }
  }
}

impl<V> Iterator for HatTrieIntoIter<V> {
  type Item = (Vec<u8>, V);

  fn next(&mut self) -> Option<Self::Item> {
    loop {
      // 1. Determine Action by peeking at mutable stack
      let action = match self.stack.last_mut() {
        // --- LEAF ---
        Some(IntoIterFrame::Leaf { items }) => {
          if let Some((suffix, val)) = items.next() {
            let mut full = self.prefix_buffer.clone();
            full.extend_from_slice(&suffix);
            IntoIterAction::Yield(full, val)
          } else {
            IntoIterAction::PopFrame
          }
        }

        // --- RADIX ---
        Some(IntoIterFrame::Radix {
          prefix,
          child,
          visited,
        }) => {
          if !*visited {
            *visited = true;
            // Move child out. We replace with default to steal ownership.
            let child_node = *std::mem::replace(child, Box::new(Node::default()));
            // Clone prefix to push to buffer
            IntoIterAction::PushRadixChild(prefix.clone(), child_node)
          } else {
            IntoIterAction::PopRadixPrefix(prefix.len())
          }
        }

        // --- INTERNAL ---
        Some(IntoIterFrame::Internal {
          children,
          child_idx,
          value,
          byte_on_stack,
        }) => {
          // 1. Cleanup previous child byte if needed
          if *byte_on_stack {
            *byte_on_stack = false;
            IntoIterAction::PopInternalByte
          }
          // 2. Yield value (only once)
          else if let Some(v) = value.take() {
            IntoIterAction::Yield(self.prefix_buffer.clone(), v)
          }
          // 3. Find next child
          else {
            let mut next_action = IntoIterAction::PopFrame; // Default if exhausted

            while let Some(child_opt) = children.next() {
              let idx = *child_idx;
              *child_idx += 1;

              if let Some(child) = child_opt {
                *byte_on_stack = true; // Mark that we are pushing a byte
                next_action = IntoIterAction::PushInternalChild(idx as u8, child);
                break; // Stop loop, execute action
              }
              // If None (empty slot), just loop and increment idx
            }
            next_action
          }
        }
        None => return None,
      };

      // 2. Execute Action (mutating stack and buffer)
      match action {
        IntoIterAction::Yield(k, v) => return Some((k, v)),
        IntoIterAction::PopFrame => {
          self.stack.pop();
        }

        // Radix Execution
        IntoIterAction::PushRadixChild(prefix, child) => {
          self.prefix_buffer.extend_from_slice(&prefix);
          self.push_node(child);
        }
        IntoIterAction::PopRadixPrefix(len) => {
          self.stack.pop();
          let new_len = self.prefix_buffer.len() - len;
          self.prefix_buffer.truncate(new_len);
        }

        // Internal Execution
        IntoIterAction::PopInternalByte => {
          self.prefix_buffer.pop();
          // Loop again immediately to process next child or finish node
        }
        IntoIterAction::PushInternalChild(byte, child) => {
          self.prefix_buffer.push(byte);
          self.push_node(child);
        }

        IntoIterAction::None => continue,
      }
    }
  }
}

// --- STREAMING (ZERO-ALLOCATION) ITERATOR ---

/// A temporary view of an item yielded by the `HatTrieStreamingIter`.
///
/// This struct holds a borrow of the iterator and provides access to the
/// current key and value. The key is a slice of the iterator's internal,
/// reusable buffer.
pub struct StreamItem<'iter, 'a: 'iter, V> {
  iter: &'iter HatTrieStreamingIter<'a, V>,
}

impl<'iter, 'a, V> StreamItem<'iter, 'a, V> {
  /// Returns the key for the current item as a byte slice.
  pub fn key(&self) -> &'iter [u8] {
    &self.iter.key_buffer
  }

  /// Returns a reference to the value for the current item.
  pub fn value(&self) -> &'a V {
    // We need to store the current value reference inside the iterator
    self.iter.current_value.unwrap()
  }
}

enum StreamAction<'a, V> {
  Yield(&'a V),
  PushNode(&'a Node<V>),
  PopFrame,
  None,
}

/// A streaming iterator that yields borrowed key slices to avoid allocations.
///
/// This iterator is significantly faster for full-table scans than the standard
/// `iter()` because it reuses an internal buffer for key reconstruction.
///
/// **Crucially, the returned `&[u8]` key slice is only valid until the next
/// call to `next_item()`.** Do not store this slice; consume it immediately.
///
/// # Example
/// ```ignore
/// let mut iter = trie.streaming_iter();
/// while let Some((key, value)) = iter.next_item() {
///     // `key` is a &[u8] that is only valid for this loop iteration.
///     process(key, value);
/// }
/// ```
pub struct HatTrieStreamingIter<'a, V> {
  /// The stack of nodes to visit for the DFS traversal.
  stack: Vec<IterFrame<'a, V>>,
  /// A buffer that holds the current prefix path as we descend the trie.
  prefix_buffer: Vec<u8>,
  /// The single, reusable buffer for assembling the final key.
  key_buffer: Vec<u8>,
  // Store the value from the last successful traversal step
  current_value: Option<&'a V>,
}

impl<'a, V> HatTrieStreamingIter<'a, V> {
  /// Creates a new streaming iterator starting at the given root node.
  fn new(root: &'a Node<V>) -> Self {
    let mut iter = Self {
      stack: Vec::with_capacity(16),
      prefix_buffer: Vec::with_capacity(64),
      key_buffer: Vec::with_capacity(64),
      current_value: None,
    };
    iter.push_node(root);
    iter
  }

  /// Internal helper to push a new node onto the traversal stack.
  fn push_node(&mut self, node: &'a Node<V>) {
    match node {
      Node::Internal { .. } => {
        self.stack.push(IterFrame::Internal {
          node,
          child_idx: 0,
          yielded_value: false,
          byte_on_stack: false,
        });
      }
      Node::Radix { .. } => {
        self.stack.push(IterFrame::Radix {
          node,
          visited: false,
        });
      }
      Node::Leaf(leaf) => {
        let mut items: Vec<_> = leaf.iter().collect();
        items.sort_unstable_by(|a, b| a.0.cmp(b.0));
        self.stack.push(IterFrame::Leaf {
          items: items.into_iter(),
        });
      }
    }
  }

  /// Advances the iterator and returns a view to the next item.
  pub fn next(&mut self) -> Option<StreamItem<'_, 'a, V>> {
    loop {
      // 1. Determine action
      let action = match self.stack.last_mut() {
        Some(IterFrame::Leaf { items }) => {
          if let Some((suffix, val)) = items.next() {
            self.key_buffer.clear();
            self.key_buffer.extend_from_slice(&self.prefix_buffer);
            self.key_buffer.extend_from_slice(suffix);
            StreamAction::Yield(val)
          } else {
            StreamAction::PopFrame
          }
        }
        Some(IterFrame::Radix { node, visited }) => {
          if !*visited {
            *visited = true;
            if let Node::Radix { prefix, child } = node {
              self.prefix_buffer.extend_from_slice(prefix);
              StreamAction::PushNode(child)
            } else {
              unreachable!()
            }
          } else {
            if let Node::Radix { prefix, .. } = self.stack.pop().unwrap().node() {
              let new_len = self.prefix_buffer.len() - prefix.len();
              self.prefix_buffer.truncate(new_len);
            }
            StreamAction::None // Continue loop
          }
        }
        Some(IterFrame::Internal {
          node,
          child_idx,
          yielded_value,
          byte_on_stack,
        }) => {
          if *byte_on_stack {
            self.prefix_buffer.pop();
            *byte_on_stack = false;
            StreamAction::None
          } else if let Node::Internal { children, value } = node {
            if !*yielded_value {
              *yielded_value = true;
              if let Some(v) = value {
                self.key_buffer.clear();
                self.key_buffer.extend_from_slice(&self.prefix_buffer);
                StreamAction::Yield(v)
              } else {
                StreamAction::None
              }
            } else {
              // Find next child
              let mut next_child = None;
              while *child_idx < 256 {
                let idx = *child_idx;
                *child_idx += 1;
                if let Some(child) = &children[idx] {
                  next_child = Some((child, idx));
                  break;
                }
              }

              if let Some((child_node, byte_val)) = next_child {
                self.prefix_buffer.push(byte_val as u8);
                *byte_on_stack = true;
                StreamAction::PushNode(child_node)
              } else {
                StreamAction::PopFrame
              }
            }
          } else {
            unreachable!()
          }
        }
        None => return None,
      };

      // 2. Execute action
      match action {
        StreamAction::Yield(val) => {
          self.current_value = Some(val);
          return Some(StreamItem { iter: self });
        }
        StreamAction::PushNode(node) => {
          self.push_node(node);
        }
        StreamAction::PopFrame => {
          self.stack.pop();
        }
        StreamAction::None => {}
      }
    }
  }
}

// --- SEGMENT (ZERO-ALLOCATION) ITERATOR ---

/// A view of a key's components, yielded by the `SegmentIter`.
///
/// This struct provides zero-allocation access to a key by exposing its
/// constituent parts: the prefix path from the trie's directory and the
/// suffix stored in a leaf bucket. The full key can be reconstructed by
/// concatenating `path` and `suffix`.
#[derive(Debug, PartialEq, Eq)]
pub struct KeySegments<'a> {
  /// A slice representing the prefix of the key, formed by traversing the trie's internal and radix nodes.
  pub path: &'a [u8],
  /// A slice representing the suffix of the key, stored directly in a leaf node.
  pub suffix: &'a [u8],
}

/// A temporary view of a key's components, yielded by the `SegmentIter`.
///
/// This struct holds a borrow of the iterator and provides access to the
/// current key's path and suffix segments.
pub struct SegmentItem<'iter, 'a: 'iter, V> {
  iter: &'iter SegmentIter<'a, V>,
  suffix: &'a [u8],
  value: &'a V,
}

impl<'iter, 'a, V> SegmentItem<'iter, 'a, V> {
  /// Returns the key's components for the current item.
  pub fn segments(&self) -> KeySegments<'_> {
    KeySegments {
      path: &self.iter.prefix_buffer,
      suffix: self.suffix,
    }
  }

  /// Returns a reference to the value for the current item.
  pub fn value(&self) -> &'a V {
    self.value
  }
}

/// A zero-allocation iterator that yields key "segments" (`KeySegments`).
///
/// The yielded item's key segments are only valid for the duration of the loop iteration.
pub struct SegmentIter<'a, V> {
  stack: Vec<IterFrame<'a, V>>,
  prefix_buffer: Vec<u8>,
}

impl<'a, V> SegmentIter<'a, V> {
  /// Creates a new segment iterator starting at the given root node.
  fn new(root: &'a Node<V>) -> Self {
    let mut iter = Self {
      stack: Vec::with_capacity(16),
      prefix_buffer: Vec::with_capacity(64),
    };
    // The `push_node` helper can be shared or copied from other iterators.
    // For self-containment, let's assume it's defined here.
    iter.push_node(root);
    iter
  }

  /// Internal helper to push a new node onto the traversal stack.
  fn push_node(&mut self, node: &'a Node<V>) {
    match node {
      Node::Internal { .. } => {
        self.stack.push(IterFrame::Internal {
          node,
          child_idx: 0,
          yielded_value: false,
          byte_on_stack: false,
        });
      }
      Node::Radix { .. } => {
        self.stack.push(IterFrame::Radix {
          node,
          visited: false,
        });
      }
      Node::Leaf(leaf) => {
        let mut items: Vec<_> = leaf.iter().collect();
        items.sort_unstable_by(|a, b| a.0.cmp(b.0));
        self.stack.push(IterFrame::Leaf {
          items: items.into_iter(),
        });
      }
    }
  }

  /// Advances the iterator and returns a view to the next item's segments.
  pub fn next(&mut self) -> Option<SegmentItem<'_, 'a, V>> {
    loop {
      let frame = self.stack.last_mut()?;

      match frame {
        IterFrame::Leaf { items } => {
          if let Some((suffix, val)) = items.next() {
            // Found an item. Return the view struct which borrows self.
            return Some(SegmentItem {
              iter: self,
              suffix,
              value: val,
            });
          } else {
            self.stack.pop();
          }
        }
        IterFrame::Internal {
          node,
          child_idx,
          yielded_value,
          byte_on_stack,
        } => {
          // This logic is identical to HatTrieStreamIter's `next` method.
          if *byte_on_stack {
            self.prefix_buffer.pop();
            *byte_on_stack = false;
            continue;
          }

          if let Node::Internal { children, value } = node {
            if !*yielded_value {
              *yielded_value = true;
              if let Some(v) = value {
                return Some(SegmentItem {
                  iter: self,
                  suffix: &[],
                  value: v,
                });
              }
            }

            let start_idx = *child_idx;
            let children_ref = children;
            let mut found_child = false;

            for i in start_idx..256 {
              if let Some(child) = &children_ref[i] {
                if let Some(IterFrame::Internal {
                  child_idx,
                  byte_on_stack,
                  ..
                }) = self.stack.last_mut()
                {
                  *child_idx = i + 1;
                  *byte_on_stack = true;
                }
                self.prefix_buffer.push(i as u8);
                self.push_node(child);
                found_child = true;
                break;
              }
            }

            if found_child {
              continue;
            } else {
              self.stack.pop();
            }
          } else {
            unreachable!()
          }
        }
        IterFrame::Radix { node, visited } => {
          if !*visited {
            *visited = true;
            if let Node::Radix { prefix, child } = node {
              self.prefix_buffer.extend_from_slice(prefix);
              self.push_node(child);
            }
          } else {
            if let Node::Radix { prefix, .. } = self.stack.pop().unwrap().node() {
              let new_len = self.prefix_buffer.len() - prefix.len();
              self.prefix_buffer.truncate(new_len);
            }
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::bucket::Bucket;

  use super::*;

  struct NoDefault {
    _x: i32,
  }

  #[test]
  fn test_no_default_trait_required() {
    // This line would fail to compile with derive(Default)
    let mut trie: HatTrie<NoDefault> = HatTrie::new();

    trie.insert("foo", NoDefault { _x: 10 });
    assert!(trie.len() == 1);
  }

  #[test]
  fn test_basic_ops() {
    let mut trie = HatTrie::new();
    trie.insert("apple", 1);
    trie.insert("apricot", 2);
    trie.insert("banana", 3);

    assert_eq!(trie.get("apple"), Some(&1));
    assert_eq!(trie.get("apricot"), Some(&2));
    assert_eq!(trie.get("banana"), Some(&3));
    assert_eq!(trie.get("carrot"), None);
  }

  #[test]
  fn test_bursting() {
    // Force a burst by inserting many items sharing a prefix
    let mut trie = HatTrie::new();

    // This relies on the BURST_THRESHOLD in node.rs
    // We will insert enough data to trigger it.
    for i in 0..5000 {
      let key = format!("key:{}", i);
      trie.insert(key, i);
    }

    assert_eq!(trie.get("key:100"), Some(&100));
    assert_eq!(trie.get("key:4999"), Some(&4999));
  }

  #[test]
  fn test_sorted_bucket_insert() {
    let mut b = Bucket::new();
    b.insert(b"zebra", 1);
    b.insert(b"apple", 2);
    b.insert(b"muffin", 3);

    let items: Vec<_> = b.iter().collect();
    assert_eq!(items[0].0, b"apple"); // 2
    assert_eq!(items[1].0, b"muffin"); // 3
    assert_eq!(items[2].0, b"zebra"); // 1
  }
}
