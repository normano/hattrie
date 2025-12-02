use hat_trie::HatTrie;
use proptest::prelude::*;
use std::collections::BTreeMap;

// ============================================================================
// 1. Functional Equivalence Tests
// ============================================================================

fn create_test_trie() -> HatTrie<usize> {
  let mut trie = HatTrie::with_threshold(128); // Force complex structure

  // Add a mix of data
  trie.insert("apple", 1);
  trie.insert("app", 2);
  trie.insert("apricot", 3);
  trie.insert("banana", 4);
  trie.insert("bandana", 5);
  trie.insert("", 0); // Empty key

  // Add URL-like keys to test Radix nodes
  trie.insert("http://google.com", 100);
  trie.insert("http://google.com/maps", 101);

  trie
}

#[test]
fn test_streaming_iter_equivalence() {
  let trie = create_test_trie();

  // FIX: Dereference the value here to match the type of stream_results
  let expected: Vec<_> = trie.iter().map(|(k, v)| (k, *v)).collect();

  let mut stream_results = Vec::new();
  let mut stream_iter = trie.streaming_iter();
  while let Some(item) = stream_iter.next() {
    stream_results.push((item.key().to_vec(), *item.value()));
  }

  assert_eq!(expected, stream_results);
}

#[test]
fn test_segment_iter_equivalence() {
  let trie = create_test_trie();

  // FIX: Dereference the value here to match the type of segment_results
  let expected: Vec<_> = trie.iter().map(|(k, v)| (k, *v)).collect();

  let mut segment_results = Vec::new();
  let mut segment_iter = trie.iter_segments();
  while let Some(item) = segment_iter.next() {
    let segments = item.segments();
    let full_key = [segments.path, segments.suffix].concat();
    segment_results.push((full_key, *item.value()));
  }

  assert_eq!(expected, segment_results);
}

// ============================================================================
// 2. Boundary Condition Tests
// ============================================================================

#[test]
fn test_streaming_on_empty() {
  let trie: HatTrie<usize> = HatTrie::new();

  let mut stream_iter = trie.streaming_iter();
  assert!(stream_iter.next().is_none());

  let mut segment_iter = trie.iter_segments();
  assert!(segment_iter.next().is_none());
}

#[test]
fn test_streaming_on_single_item() {
  let mut trie = HatTrie::new();
  trie.insert("key", 100);

  let mut stream_iter = trie.streaming_iter();
  let item = stream_iter.next().unwrap();
  assert_eq!(item.key(), b"key");
  assert_eq!(*item.value(), 100);
  assert!(stream_iter.next().is_none());

  let mut segment_iter = trie.iter_segments();
  let item = segment_iter.next().unwrap();
  let segments = item.segments();
  let full_key = [segments.path, segments.suffix].concat();
  assert_eq!(full_key, b"key");
  assert_eq!(*item.value(), 100);
  assert!(segment_iter.next().is_none());
}

// ============================================================================
// 3. Lifetime Safety Test (Compile-Time Check)
// ============================================================================

// This test is designed to fail compilation if lifetimes are incorrect.
// It verifies that you cannot store the borrowed key slice outside the loop.
#[test]
#[cfg(feature = "compile-fail-tests")]
fn test_streaming_iter_lifetime_escapes() {
  let trie = create_test_trie();
  let mut leaked_key_slice: &[u8] = &[];

  let mut iter = trie.streaming_iter();
  while let Some(item) = iter.next() {
    // This line should cause a compile error:
    // `item` does not live long enough
    leaked_key_slice = item.key();
  }

  println!("{:?}", leaked_key_slice);
}

// ============================================================================
// 4. Property-Based Fuzzing
// ============================================================================

proptest! {
  #[test]
  fn prop_streaming_iter_vs_btreemap(
    keys in proptest::collection::vec(proptest::collection::vec(0..=255u8, 0..32), 0..500)
  ) {
    let mut trie = HatTrie::new();
    let mut map = BTreeMap::new();

    for (i, key) in keys.iter().enumerate() {
      trie.insert(key, i);
      map.insert(key.clone(), i);
    }

    // Simultaneously iterate both
    let mut stream_iter = trie.streaming_iter();
    let mut btree_iter = map.iter();

    loop {
      let stream_next = stream_iter.next();
      let btree_next = btree_iter.next();

      match (stream_next, btree_next) {
        (Some(s_item), Some((b_key, b_val))) => {
          assert_eq!(s_item.key(), b_key.as_slice());
          assert_eq!(s_item.value(), b_val);
        },
        (None, None) => {
          // Both finished, success
          break;
        },
        _ => {
          // One finished before the other, failure
          panic!("Iterators finished at different times");
        }
      }
    }
  }

  #[test]
  fn prop_segment_iter_vs_btreemap(
    keys in proptest::collection::vec(proptest::collection::vec(0..=255u8, 0..32), 0..500)
  ) {
    let mut trie = HatTrie::new();
    let mut map = BTreeMap::new();

    for (i, key) in keys.iter().enumerate() {
      trie.insert(key, i);
      map.insert(key.clone(), i);
    }

    let mut segment_iter = trie.iter_segments();
    let mut btree_iter = map.iter();

    loop {
      let seg_next = segment_iter.next();
      let btree_next = btree_iter.next();

      match (seg_next, btree_next) {
        (Some(s_item), Some((b_key, b_val))) => {
          let segments = s_item.segments();
          let full_key = [segments.path, segments.suffix].concat();

          assert_eq!(full_key, *b_key);
          assert_eq!(s_item.value(), b_val);
        },
        (None, None) => break,
        _ => panic!("Iterators finished at different times"),
      }
    }
  }
}
