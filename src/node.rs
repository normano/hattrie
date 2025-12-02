use crate::{HatTrieStats, bucket::MAX_BUCKET_KEY_LEN, leaf::ArrayHash};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Node<V> {
  Internal {
    #[cfg_attr(feature = "serde", serde(with = "children_serde"))]
    children: Box<[Option<Node<V>>; 256]>,
    value: Option<V>,
  },
  Radix {
    prefix: Vec<u8>,
    child: Box<Node<V>>,
  },
  Leaf(ArrayHash<V>),
}

impl<V> Default for Node<V> {
  fn default() -> Self {
    Node::Leaf(ArrayHash::default())
  }
}

impl<V> Node<V> {
  pub fn is_empty_leaf(&self) -> bool {
    match self {
      Node::Leaf(b) => b.is_empty(),
      _ => false,
    }
  }

  pub fn get(&self, key: &[u8]) -> Option<&V> {
    match self {
      Node::Internal { children, value } => {
        if key.is_empty() {
          return value.as_ref();
        }
        let idx = key[0] as usize;
        match &children[idx] {
          Some(child) => child.get(&key[1..]),
          None => None,
        }
      }
      Node::Radix { prefix, child } => {
        if key.starts_with(prefix) {
          child.get(&key[prefix.len()..])
        } else {
          None
        }
      }
      Node::Leaf(leaf) => leaf.get(key),
    }
  }

  pub fn get_mut(&mut self, key: &[u8]) -> Option<&mut V> {
    match self {
      Node::Internal { children, value } => {
        if key.is_empty() {
          return value.as_mut();
        }
        match &mut children[key[0] as usize] {
          Some(child) => child.get_mut(&key[1..]),
          None => None,
        }
      }
      Node::Radix { prefix, child } => {
        if key.starts_with(prefix) {
          child.get_mut(&key[prefix.len()..])
        } else {
          None
        }
      }
      Node::Leaf(bucket) => bucket.get_mut(key),
    }
  }

  pub fn remove(&mut self, key: &[u8]) -> Option<V> {
    match self {
      Node::Internal { children, value } => {
        if key.is_empty() {
          return value.take();
        }

        let idx = key[0] as usize;
        if let Some(child) = &mut children[idx] {
          let result = child.remove(&key[1..]);

          // Optimization: If leaf becomes empty, prune it.
          // This keeps the trie from accumulating empty buckets.
          if child.is_empty_leaf() {
            children[idx] = None;
          }

          return result;
        }
        None
      }
      Node::Radix { prefix, child } => {
        if key.starts_with(prefix) {
          let res = child.remove(&key[prefix.len()..]);
          // Heal: If child becomes empty leaf?
          if child.is_empty_leaf() {
            // We can't easily turn self into Empty here without changing type.
            // But since child is Box<Node>, we can check inside.
            // The parent call handles setting self to None if self becomes empty leaf.
          }
          return res;
        }
        None
      }
      Node::Leaf(bucket) => bucket.remove(key),
    }
  }

  pub fn insert(&mut self, key: &[u8], val: V, threshold: usize) -> Option<V> {
    match self {
      Node::Internal { children, value } => {
        if key.is_empty() {
          return value.replace(val);
        }

        let idx = key[0] as usize;
        // Initialize child if missing
        if children[idx].is_none() {
          children[idx] = Some(Node::Leaf(ArrayHash::new()));
        }

        // Recurse
        children[idx]
          .as_mut()
          .unwrap()
          .insert(&key[1..], val, threshold)
      }
      Node::Radix { prefix, child } => {
        // 1. Calculate shared length
        let common_len = common_prefix_len(prefix, key);

        if common_len == prefix.len() {
          // Full match on prefix: recurse down
          return child.insert(&key[common_len..], val, threshold);
        }

        // 2. Partial match: We must split the Radix node
        // Structure:
        // self (Radix) -> becomes -> Internal
        //    Index prefix[common_len]: Radix(remainder_of_prefix) -> original_child
        //    Index key[common_len]:    Leaf(new_key) OR recurse

        // Extract parts
        let prefix_remainder = prefix[common_len + 1..].to_vec(); // +1 to skip the branch char
        let branch_char_old = prefix[common_len];

        // Steal the child (ownership dance)
        let old_child = std::mem::replace(child, Box::new(Node::default()));

        // Create the node that preserves the existing path
        let path_continuation = if prefix_remainder.is_empty() {
          *old_child
        } else {
          Node::Radix {
            prefix: prefix_remainder,
            child: old_child,
          }
        };

        // Create the new Internal hub
        let mut children = empty_children_array();
        children[branch_char_old as usize] = Some(path_continuation);

        let mut new_internal = Node::Internal {
          children,
          value: None,
        };

        // Insert the NEW key into this new internal hub
        // The new key is key[common_len..].
        // The first byte of this slice will be the index in new_internal.
        // If key was exactly equal to common part, it goes into value.
        let new_key_suffix = &key[common_len..];
        let res = new_internal.insert(new_key_suffix, val, threshold);

        // Update self
        // If common_len > 0, we need a Radix node above the Internal node
        if common_len > 0 {
          *self = Node::Radix {
            prefix: prefix[0..common_len].to_vec(),
            child: Box::new(new_internal),
          };
        } else {
          *self = new_internal;
        }

        res
      }
      Node::Leaf(bucket) => {
        // Check if key fits in bucket format.
        // If not, we MUST burst to consume prefix, even if bucket isn't full.
        if key.len() > MAX_BUCKET_KEY_LEN {
          self.burst(threshold);
          // After burst, self is Internal.
          // Recursively call insert on self.
          // This consumes 1 byte (via Internal logic) and tries again.
          return self.insert(key, val, threshold);
        }

        let result = bucket.insert(key, val);

        // Check if we need to burst
        // We check size_in_bytes to respect cache lines
        if bucket.size_in_bytes() > threshold {
          self.burst(threshold);
        }

        result
      }
    }
  }

  /// Transforms a Leaf into an Internal node, distributing the bucket's contents.
  fn burst(&mut self, threshold: usize) {
    let old_node = std::mem::replace(self, Node::default());

    if let Node::Leaf(mut leaf) = old_node {
      // 1. Calculate LCP of all keys in this bucket
      let lcp = leaf.longest_common_prefix_all();

      // 2. Create the Internal node that will do the branching
      let mut internal = Node::Internal {
        children: empty_children_array(),
        value: None,
      };

      // 3. Redistribute items
      for (key, val) in leaf.drain_all() {
        // Strip LCP. The internal insert logic will handle the branching char.
        // If lcp is "http", and key is "https", we insert "s" into internal.
        internal.insert(&key[lcp.len()..], val, threshold);
      }

      // 4. Reassemble
      if !lcp.is_empty() {
        *self = Node::Radix {
          prefix: lcp,
          child: Box::new(internal),
        };
      } else {
        *self = internal;
      }
    } else {
      unreachable!("Attempted to burst non-Leaf");
    }
  }
}

// --- STATS HELPER ---

impl<V> Node<V> {
  pub(super) fn collect_stats(&self, stats: &mut HatTrieStats) {
    match self {
      Node::Internal { children, .. } => {
        stats.internal_nodes += 1;
        for child in children.iter().flatten() {
          child.collect_stats(stats);
        }
      }
      Node::Radix { child, .. } => {
        // Radix nodes technically count as internal structure
        stats.internal_nodes += 1;
        child.collect_stats(stats);
      }
      Node::Leaf(bucket) => {
        stats.leaf_nodes += 1;
        stats.total_items += bucket.len();
        if bucket.len() > stats.max_bucket_size {
          stats.max_bucket_size = bucket.len();
        }
      }
    }
  }
}

// Helper to initialize the array of 256 Nones
fn empty_children_array<V>() -> Box<[Option<Node<V>>; 256]> {
  // We can't verify Copy on V easily here for standard array init,
  // so we use a vector and convert it to boxed slice.
  let mut vec = Vec::with_capacity(256);
  for _ in 0..256 {
    vec.push(None);
  }
  vec
    .into_boxed_slice()
    .try_into()
    .unwrap_or_else(|_| panic!("Should be 256"))
}

// Helper for Radix Insert
pub(crate) fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
  a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

#[cfg(feature = "serde")]
mod children_serde {
  use super::*;
  use serde::{
    Deserializer, Serializer,
    de::{SeqAccess, Visitor},
    ser::SerializeSeq,
  };
  use std::fmt;

  pub fn serialize<S, V>(
    children: &Box<[Option<Node<V>>; 256]>,
    serializer: S,
  ) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
    V: Serialize,
  {
    // Serialize as a sequence (essentially a Vec)
    let mut seq = serializer.serialize_seq(Some(256))?;
    for child in children.iter() {
      seq.serialize_element(child)?;
    }
    seq.end()
  }

  pub fn deserialize<'de, D, V>(deserializer: D) -> Result<Box<[Option<Node<V>>; 256]>, D::Error>
  where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
  {
    struct ChildrenVisitor<V>(std::marker::PhantomData<V>);

    impl<'de, V> Visitor<'de> for ChildrenVisitor<V>
    where
      V: Deserialize<'de>,
    {
      type Value = Box<[Option<Node<V>>; 256]>;

      fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a sequence of 256 Option<Node<V>>")
      }

      fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
      where
        A: SeqAccess<'de>,
      {
        let mut vec = Vec::with_capacity(256);
        while let Some(elem) = seq.next_element()? {
          vec.push(elem);
        }

        if vec.len() != 256 {
          return Err(serde::de::Error::invalid_length(vec.len(), &self));
        }

        // Convert Vec back to Box<[T; 256]>
        vec
          .into_boxed_slice()
          .try_into()
          .map_err(|_| serde::de::Error::custom("Failed to convert Vec to Box<[T; 256]>"))
      }
    }

    deserializer.deserialize_seq(ChildrenVisitor(std::marker::PhantomData))
  }
}
