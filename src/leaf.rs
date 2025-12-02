use crate::bucket::Bucket;
use ahash::RandomState;

#[cfg(feature = "serde")]
use serde::{Serialize, Deserialize, Serializer, Deserializer, ser::SerializeSeq, de::{Visitor, SeqAccess}};

#[cfg(feature = "serde")]
use std::fmt;

#[derive(Debug, Clone)]
pub struct ArrayHash<V> {
  buckets: Vec<Bucket<V>>,
  count: usize,
  hasher: RandomState,
  mask: usize,
}

fn default_hasher() -> RandomState {
  RandomState::new()
}

impl<V> Default for ArrayHash<V> {
  fn default() -> Self {
    // Start with 4 buckets. Must be power of 2.
    let initial_buckets = 4;
    let mut buckets = Vec::with_capacity(initial_buckets);
    for _ in 0..initial_buckets {
      buckets.push(Bucket::new());
    }
    Self {
      buckets,
      count: 0,
      hasher: default_hasher(),
      mask: initial_buckets - 1,
    }
  }
}

impl<V> ArrayHash<V> {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn len(&self) -> usize {
    self.count
  }

  pub fn is_empty(&self) -> bool {
    self.count == 0
  }

  pub fn size_in_bytes(&self) -> usize {
    self.buckets.iter().map(|b| b.size_in_bytes()).sum()
  }

  fn hash(&self, key: &[u8]) -> u64 {
    self.hasher.hash_one(key)
  }

  pub fn get(&self, key: &[u8]) -> Option<&V> {
    let idx = (self.hash(key) as usize) & self.mask;
    self.buckets[idx].get(key)
  }

  pub fn get_mut(&mut self, key: &[u8]) -> Option<&mut V> {
    let idx = (self.hash(key) as usize) & self.mask;
    self.buckets[idx].get_mut(key)
  }

  pub fn insert(&mut self, key: &[u8], value: V) -> Option<V> {
    // Grow if average load > 8 items per bucket (heuristic)
    // This keeps the linear scan in buckets very short.
    if self.count > self.buckets.len() * 8 {
      self.grow();
    }

    let idx = (self.hash(key) as usize) & self.mask;
    let res = self.buckets[idx].insert(key, value);
    if res.is_none() {
      self.count += 1;
    }
    res
  }

  pub fn remove(&mut self, key: &[u8]) -> Option<V> {
    let idx = (self.hash(key) as usize) & self.mask;
    let res = self.buckets[idx].remove(key);
    if res.is_some() {
      self.count -= 1;
    }
    res
  }

  fn grow(&mut self) {
    let new_size = self.buckets.len() * 2;
    let mut new_buckets = Vec::with_capacity(new_size);
    for _ in 0..new_size {
      new_buckets.push(Bucket::new());
    }

    let new_mask = new_size - 1;

    // Redistribute all items
    for old_bucket in &mut self.buckets {
      for (k, v) in old_bucket.drain() {
        let h = self.hasher.hash_one(&k);
        let idx = (h as usize) & new_mask;
        new_buckets[idx].insert(&k, v);
      }
    }

    self.buckets = new_buckets;
    self.mask = new_mask;
  }

  /// Drain all items (order undefined)
  pub fn drain_all(&mut self) -> impl Iterator<Item = (Vec<u8>, V)> + '_ {
    self.buckets.iter_mut().flat_map(|b| b.drain())
  }

  /// Iterate all items (order undefined)
  pub fn iter(&self) -> impl Iterator<Item = (&[u8], &V)> + '_ {
    self.buckets.iter().flat_map(|b| b.iter())
  }

  /// Calculates the longest common prefix of all keys in this leaf.
  pub fn longest_common_prefix_all(&self) -> Vec<u8> {
    if self.count == 0 {
      return Vec::new();
    }

    let mut iter = self.iter().map(|(k, _)| k);
    let first = match iter.next() {
      Some(k) => k,
      None => return Vec::new(),
    };

    let mut lcp = first.to_vec();

    for key in iter {
      // Truncate lcp to the length of the current match
      let len = key.len().min(lcp.len());
      lcp.truncate(len);

      // Compare bytes
      for i in 0..lcp.len() {
        if lcp[i] != key[i] {
          lcp.truncate(i);
          break;
        }
      }

      if lcp.is_empty() {
        return lcp;
      }
    }
    lcp
  }
}

// --- Manual Serde Implementation ---

#[cfg(feature = "serde")]
impl<V: Serialize> Serialize for ArrayHash<V> {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    // Serialize as a sequence of (Key, Value) tuples.
    // We do not serialize buckets directly because hashing is non-deterministic across sessions.
    let mut seq = serializer.serialize_seq(Some(self.count))?;
    for (k, v) in self.iter() {
      seq.serialize_element(&(k, v))?;
    }
    seq.end()
  }
}

#[cfg(feature = "serde")]
impl<'de, V: Deserialize<'de>> Deserialize<'de> for ArrayHash<V> {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    struct ArrayHashVisitor<V>(std::marker::PhantomData<V>);

    impl<'de, V: Deserialize<'de>> Visitor<'de> for ArrayHashVisitor<V> {
      type Value = ArrayHash<V>;

      fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a sequence of key-value pairs")
      }

      fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
      where
        A: SeqAccess<'de>,
      {
        // Rebuild the hash map from scratch.
        // This ensures items are hashed correctly with the new session's random seed.
        let mut map = ArrayHash::new();
        while let Some((k, v)) = seq.next_element::<(Vec<u8>, V)>()? {
          map.insert(&k, v);
        }
        Ok(map)
      }
    }

    deserializer.deserialize_seq(ArrayHashVisitor(std::marker::PhantomData))
  }
}
