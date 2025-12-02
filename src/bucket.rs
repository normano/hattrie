use std::cmp::Ordering;
// Conditional compilation for Serde
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// Define the hard limit imposed by the storage format (u8 length prefix)
pub(crate) const MAX_BUCKET_KEY_LEN: usize = 255;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Bucket<V> {
  /// Contiguous storage: [len][byte][byte]...
  keys: Vec<u8>,
  values: Vec<V>,
  count: usize,
}

impl<V> Default for Bucket<V> {
  fn default() -> Self {
    Self {
      keys: Vec::with_capacity(64),
      values: Vec::with_capacity(4),
      count: 0,
    }
  }
}

impl<V> Bucket<V> {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn size_in_bytes(&self) -> usize {
    self.keys.len() + (self.values.len() * std::mem::size_of::<V>())
  }

  pub fn len(&self) -> usize {
    self.count
  }

  pub fn is_empty(&self) -> bool {
    self.count == 0
  }
  // --- READ ---

  pub fn get(&self, search_key: &[u8]) -> Option<&V> {
    self.find_index(search_key).map(|idx| &self.values[idx])
  }

  pub fn get_mut(&mut self, search_key: &[u8]) -> Option<&mut V> {
    self.find_index(search_key).map(|idx| &mut self.values[idx])
  }

  /// Inserts while maintaining Sorted Order.
  pub fn insert(&mut self, key: &[u8], value: V) -> Option<V> {
    let mut ptr = 0;
    let mut val_idx = 0;

    // Assertion remains to catch internal logic errors,
    // but Node should now prevent this from triggering.
    assert!(
      key.len() <= MAX_BUCKET_KEY_LEN,
      "Key suffix too long for bucket storage"
    );

    // 1. Find insertion point
    while ptr < self.keys.len() {
      let len = self.keys[ptr] as usize;
      let current_key_start = ptr + 1;
      let current_key_end = current_key_start + len;

      let stored_key = &self.keys[current_key_start..current_key_end];

      match stored_key.cmp(key) {
        Ordering::Equal => {
          // Exact match: Update value
          return Some(std::mem::replace(&mut self.values[val_idx], value));
        }
        Ordering::Greater => {
          // We found a key "larger" than us.
          // We insert *before* this current record.

          // Insert value at specific index
          self.values.insert(val_idx, value);

          // Insert [len, bytes...] into raw buffer
          // We use splice to inject bytes efficiently
          let mut entry = Vec::with_capacity(1 + key.len());
          entry.push(key.len() as u8);
          entry.extend_from_slice(key);

          // ptr points to the start of the current record (the length byte)
          // so we insert right there.
          self.keys.splice(ptr..ptr, entry);

          self.count += 1;
          return None;
        }
        Ordering::Less => {
          // Keep scanning
          ptr = current_key_end;
          val_idx += 1;
        }
      }
    }

    // 2. If we reached here, the key is larger than everything else. Append.
    self.keys.push(key.len() as u8);
    self.keys.extend_from_slice(key);
    self.values.push(value);
    self.count += 1;
    None
  }

  pub fn remove(&mut self, key: &[u8]) -> Option<V> {
    let mut ptr = 0;
    let mut val_idx = 0;

    while ptr < self.keys.len() {
      let len = self.keys[ptr] as usize;
      let start = ptr + 1;
      let end = start + len;
      let stored_key = &self.keys[start..end];

      match stored_key.cmp(key) {
        Ordering::Equal => {
          let val = self.values.remove(val_idx);
          self.keys.drain(ptr..end);
          self.count -= 1;
          return Some(val);
        }
        Ordering::Greater => return None, // Sorted: passed it
        Ordering::Less => {
          ptr = end;
          val_idx += 1;
        }
      }
    }
    None
  }

  /// Returns an iterator starting at the first key >= start_bound.
  /// If start_bound is None, starts at the beginning.
  pub fn iter_from<'a>(&'a self, start_bound: Option<&[u8]>) -> BucketIter<'a, V> {
    let mut ptr = 0;
    let mut val_idx = 0;

    if let Some(bound) = start_bound {
      // Linear scan to find the starting point
      while ptr < self.keys.len() {
        let len = self.keys[ptr] as usize;
        let key_start = ptr + 1;
        let key_end = key_start + len;
        let key = &self.keys[key_start..key_end];

        // If key >= bound, we found our start.
        if key >= bound {
          break;
        }

        ptr = key_end;
        val_idx += 1;
      }
    }

    BucketIter {
      bucket: self,
      ptr,
      val_idx,
    }
  }

  // --- HELPERS ---

  /// Helper to find value index for a key
  fn find_index(&self, key: &[u8]) -> Option<usize> {
    let mut ptr = 0;
    let mut val_idx = 0;
    while ptr < self.keys.len() {
      let len = self.keys[ptr] as usize;
      let start = ptr + 1;
      let end = start + len;
      match self.keys[start..end].cmp(key) {
        Ordering::Equal => return Some(val_idx),
        Ordering::Greater => return None,
        Ordering::Less => {
          ptr = end;
          val_idx += 1;
        }
      }
    }
    None
  }

  pub fn drain(&mut self) -> BucketDrainIter<V> {
    let keys = std::mem::take(&mut self.keys);
    let values = std::mem::take(&mut self.values);
    self.count = 0;
    BucketDrainIter {
      keys,
      values_iter: values.into_iter(),
      ptr: 0,
    }
  }

  pub fn iter(&self) -> BucketIter<V> {
    BucketIter {
      bucket: self,
      ptr: 0,
      val_idx: 0,
    }
  }
}

// Iterator support
pub struct BucketDrainIter<V> {
  keys: Vec<u8>,
  values_iter: std::vec::IntoIter<V>,
  ptr: usize,
}

impl<V> Iterator for BucketDrainIter<V> {
  type Item = (Vec<u8>, V);
  fn next(&mut self) -> Option<Self::Item> {
    if self.ptr >= self.keys.len() {
      return None;
    }
    let len = self.keys[self.ptr] as usize;
    self.ptr += 1;
    let key = self.keys[self.ptr..self.ptr + len].to_vec();
    self.ptr += len;
    let val = self.values_iter.next().unwrap();
    Some((key, val))
  }
}

pub struct BucketIter<'a, V> {
  bucket: &'a Bucket<V>,
  ptr: usize,
  val_idx: usize,
}

impl<'a, V> Iterator for BucketIter<'a, V> {
  type Item = (&'a [u8], &'a V);
  fn next(&mut self) -> Option<Self::Item> {
    if self.ptr >= self.bucket.keys.len() {
      return None;
    }

    let len = self.bucket.keys[self.ptr] as usize;
    let start = self.ptr + 1;
    let end = start + len;
    let key = &self.bucket.keys[start..end];
    let val = &self.bucket.values[self.val_idx];
    self.ptr = end;
    self.val_idx += 1;

    Some((key, val))
  }
}
