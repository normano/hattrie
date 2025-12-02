use hattrie::HatTrie;
use proptest::prelude::*;
use std::collections::BTreeMap;

// ============================================================================
// 1. Deletion & Structure Cleanup
// ============================================================================

#[test]
fn test_remove() {
  let mut trie = HatTrie::new();
  trie.insert("apple", 1);
  trie.insert("apricot", 2);

  assert_eq!(trie.remove("apple"), Some(1));
  assert_eq!(trie.get("apple"), None);
  assert_eq!(trie.get("apricot"), Some(&2));
  assert_eq!(trie.len(), 1);

  // Remove non-existent
  assert_eq!(trie.remove("banana"), None);
  assert_eq!(trie.len(), 1);
}

#[test]
fn test_remove_bucket_middle() {
  let mut trie = HatTrie::new();
  // Ensure these end up in the same bucket
  trie.insert("A-1", 1);
  trie.insert("A-2", 2);
  trie.insert("A-3", 3);

  // Remove middle
  assert_eq!(trie.remove("A-2"), Some(2));

  // Verify neighbors
  assert_eq!(trie.get("A-1"), Some(&1));
  assert_eq!(trie.get("A-3"), Some(&3));
  assert_eq!(trie.get("A-2"), None);

  // Verify iteration order is still correct
  let items: Vec<_> = trie.iter().map(|(k, v)| (String::from_utf8(k).unwrap(), *v)).collect();
  assert_eq!(items, vec![("A-1".to_string(), 1), ("A-3".to_string(), 3)]);
}

#[test]
fn test_remove_internal_value() {
  let mut trie = HatTrie::new();
  // "app" is a prefix of "apple".
  // In a HAT-trie, "app" might live in a value slot of an Internal node
  // if "apple" forced a burst.
  trie.insert("apple", 100);
  trie.insert("app", 50);

  // Burst it to ensure structure is complex
  for i in 0..1000 {
    trie.insert(format!("appendage-{}", i), i);
  }

  assert_eq!(trie.remove("app"), Some(50));

  // "apple" must still exist
  assert_eq!(trie.get("apple"), Some(&100));
  // "app" must be gone
  assert_eq!(trie.get("app"), None);
}

#[test]
fn test_hollow_tree_cleanup() {
  let mut trie = HatTrie::with_threshold(1024);
  let n = 10_000;

  // 1. Fill
  for i in 0..n {
    trie.insert(format!("key-{}", i), i);
  }
  assert_eq!(trie.len(), n);

  // 2. Empty
  for i in 0..n {
    assert_eq!(trie.remove(format!("key-{}", i)), Some(i));
  }

  assert_eq!(trie.len(), 0);
  assert!(trie.is_empty());

  // 3. Verify via Iter
  assert_eq!(trie.iter().count(), 0);
}

// ============================================================================
// 2. Advanced Queries (LCP & Scan)
// ============================================================================

#[test]
fn test_longest_common_prefix_logic() {
  let mut trie = HatTrie::new();

  // Insert keys that form a chain
  trie.insert("http", 1);
  trie.insert("http://google", 2);
  trie.insert("http://google.com/maps", 3);

  // 1. Exact match
  let (len, val) = trie.longest_common_prefix("http");
  assert_eq!(len, 4);
  assert_eq!(val, Some(&1));

  // 2. Overshoot (Prefix match)
  // Should match "http://google", len 13
  let (len, val) = trie.longest_common_prefix("http://google.com/images");
  assert_eq!(len, 13);
  assert_eq!(val, Some(&2));

  // 3. Undershoot (No match deep enough)
  // Matches "http", len 4
  let (len, val) = trie.longest_common_prefix("http://yahoo.com");
  assert_eq!(len, 4);
  assert_eq!(val, Some(&1));

  // 4. No match
  let (len, val) = trie.longest_common_prefix("ftp://ftp.com");
  assert_eq!(len, 0);
  assert_eq!(val, None);
}

#[test]
fn test_scan_prefix_correctness() {
  let mut trie = HatTrie::new();
  trie.insert("foo", 1);
  trie.insert("foot", 2);
  trie.insert("football", 3);
  trie.insert("food", 4);
  trie.insert("f", 0); // Short prefix

  // Scan "foot" -> should get "foot", "football"
  // Should NOT get "foo", "food", "f"
  let results: Vec<String> = trie
    .scan_prefix("foot")
    .map(|(k, _)| String::from_utf8(k).unwrap())
    .collect();

  // Order matters (lexicographical)
  assert_eq!(results, vec!["foot", "football"]);
}

// ============================================================================
// 3. Mutation
// ============================================================================

#[test]
fn test_get_mut() {
  let mut trie = HatTrie::new();
  trie.insert("counter", 10);

  if let Some(val) = trie.get_mut("counter") {
    *val += 1;
  }

  assert_eq!(trie.get("counter"), Some(&11));
}

#[test]
fn test_get_mut_modification() {
  let mut trie = HatTrie::new();
  trie.insert("list", vec![1, 2, 3]);

  if let Some(list) = trie.get_mut("list") {
    list.push(4);
  }

  assert_eq!(trie.get("list"), Some(&vec![1, 2, 3, 4]));
}

// ============================================================================
// 4. The Grand Unified Fuzzer (Insert + Remove)
// ============================================================================

#[derive(Debug, Clone)]
enum Op {
  Insert(Vec<u8>, u32),
  Remove(Vec<u8>),
  Get(Vec<u8>), // Used to verify state during sequence
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(500))]

  #[test]
  fn prop_insert_delete_equivalence(
    ops in proptest::collection::vec(
      prop_oneof![
        // 60% chance Insert
        (any::<Vec<u8>>(), any::<u32>()).prop_map(|(k, v)| Op::Insert(k, v)),
        // 30% chance Remove
        any::<Vec<u8>>().prop_map(|k| Op::Remove(k)),
        // 10% chance Get (check consistency)
        any::<Vec<u8>>().prop_map(|k| Op::Get(k)),
      ],
      0..200
    )
  ) {
    let mut trie = HatTrie::with_threshold(128); // Low threshold to force bursts
    let mut ref_map = BTreeMap::new();

    for op in ops {
      match op {
        Op::Insert(k, v) => {
          trie.insert(&k, v);
          ref_map.insert(k, v);
        },
        Op::Remove(k) => {
          let t_res = trie.remove(&k);
          let m_res = ref_map.remove(&k);
          assert_eq!(t_res, m_res, "Remove result mismatch for key {:?}", k);
        },
        Op::Get(k) => {
          assert_eq!(trie.get(&k), ref_map.get(&k), "Get mismatch for key {:?}", k);
        }
      }

      // Check Invariant: Length
      assert_eq!(trie.len(), ref_map.len(), "Length mismatch after op");
    }

    // Final Invariant: Full iteration check
    let trie_iter: Vec<_> = trie.iter().map(|(k, v)| (k, *v)).collect();
    let map_iter: Vec<_> = ref_map.iter().map(|(k, v)| (k.clone(), *v)).collect();
    assert_eq!(trie_iter, map_iter, "Final iteration mismatch");
  }
}
