use hat_trie::HatTrie;
use proptest::prelude::*;
use std::collections::BTreeMap;

// ============================================================================
// 1. Functional Correctness (The Public API)
// ============================================================================

#[test]
fn test_basic_crud() {
  let mut trie = HatTrie::new();

  assert_eq!(trie.get("foo"), None);

  trie.insert("foo", 1);
  assert_eq!(trie.get("foo"), Some(&1));

  // Overwrite
  trie.insert("foo", 2);
  assert_eq!(trie.get("foo"), Some(&2));
  assert_eq!(trie.len(), 1); // Length shouldn't increase on overwrite

  // Insert different key
  trie.insert("bar", 10);
  assert_eq!(trie.get("bar"), Some(&10));
  assert_eq!(trie.get("foo"), Some(&2));
  assert_eq!(trie.len(), 2);
}

#[test]
fn test_large_keys_behavior() {
  // Regression test: Ensure keys larger than the burst threshold
  // don't cause infinite recursion or panics.
  // They should effectively create a deep trie chain.
  let threshold = 128;
  let mut trie = HatTrie::with_threshold(threshold);

  // Create a key significantly larger than the threshold
  let large_key = "a".repeat(threshold * 2);
  trie.insert(&large_key, 999);

  assert_eq!(trie.get(&large_key), Some(&999));
}

#[test]
fn test_scan_prefix() {
  let mut trie = HatTrie::new();
  trie.insert("app", 1);
  trie.insert("apple", 2);
  trie.insert("application", 3);
  trie.insert("banana", 4);

  // Scan "app" should find 3 items
  let results: Vec<_> = trie.scan_prefix("app").collect();
  assert_eq!(results.len(), 3);

  // Scan "appl" should find 2 items
  let results: Vec<_> = trie.scan_prefix("appl").collect();
  assert_eq!(results.len(), 2);

  // Scan "z" should find 0
  let results: Vec<_> = trie.scan_prefix("z").collect();
  assert_eq!(results.len(), 0);
}

#[test]
fn test_longest_common_prefix() {
  let mut trie = HatTrie::new();
  trie.insert("http", 1);
  trie.insert("http://google", 2);

  let (len, val) = trie.longest_common_prefix("http://google.com");
  assert_eq!(len, 13); // "http://google" is 13 chars
  assert_eq!(val, Some(&2));

  let (len, val) = trie.longest_common_prefix("http://yahoo.com");
  assert_eq!(len, 4); // "http"
  assert_eq!(val, Some(&1));

  let (len, val) = trie.longest_common_prefix("ftp://ftp.com");
  assert_eq!(len, 0);
  assert_eq!(val, None);
}

#[test]
fn test_traits() {
  let trie: HatTrie<i32> = vec![("a".to_string(), 1), ("b".to_string(), 2)].into_iter().collect();

  assert_eq!(trie["a"], 1);
  assert_eq!(trie["b"], 2);
}

// ============================================================================
// 2. Structural Integrity (Burst Logic)
// ============================================================================

#[test]
fn test_forced_burst_consistency() {
  // Use a tiny threshold to force the root bucket to burst immediately
  let mut trie = HatTrie::with_threshold(100);

  // Insert 50 strings (e.g. "val-00" to "val-49").
  // Each entry is approx 6 bytes key + 4 bytes value + 1 byte overhead = 11 bytes.
  // 50 * 11 = 550 bytes. This guarantees multiple bursts.
  for i in 0..50 {
    trie.insert(format!("val-{:02}", i), i);
  }

  // Verify all exist
  for i in 0..50 {
    let key = format!("val-{:02}", i);
    assert_eq!(trie.get(&key), Some(&i), "Failed to retrieve {}", key);
  }

  // Verify length
  assert_eq!(trie.len(), 50);
}

#[test]
fn test_deep_recursion_common_prefix() {
  // Tests that bursting works correctly when keys share long prefixes.
  // This forces the "consumption" of the prefix down the tree.
  let mut trie = HatTrie::with_threshold(50);

  let items = vec![
    "aaaaaaaaaa1",
    "aaaaaaaaaa2",
    "aaaaaaaaaa3",
    "aaaaaaaaaa4",
    "aaaaaaaaaa5",
  ];

  for (i, key) in items.iter().enumerate() {
    trie.insert(key, i);
  }

  for (i, key) in items.iter().enumerate() {
    assert_eq!(trie.get(key), Some(&i));
  }
}

#[test]
fn test_sparse_burst_distribution() {
  // Verify that when we burst, keys are distributed to the correct children
  // (a, m, z) and not just clumped together or lost.
  let mut trie = HatTrie::with_threshold(10); // Very small

  trie.insert("alpha", 1);
  trie.insert("mid", 2);
  trie.insert("zoo", 3);
  // Add dummy data to force the burst
  trie.insert("extra1", 9);
  trie.insert("extra2", 9);

  assert_eq!(trie.get("alpha"), Some(&1));
  assert_eq!(trie.get("mid"), Some(&2));
  assert_eq!(trie.get("zoo"), Some(&3));
}

// ============================================================================
// 3. Bucket Sorting & Memory Layout
// ============================================================================

#[test]
fn test_lexicographical_iteration_order() {
  let mut trie = HatTrie::new();

  // Insert in reverse order
  trie.insert("zebra", 1);
  trie.insert("delta", 2);
  trie.insert("alpha", 3);

  let keys: Vec<String> = trie.iter().map(|(k, _)| String::from_utf8(k).unwrap()).collect();

  assert_eq!(keys, vec!["alpha", "delta", "zebra"]);
}

#[test]
fn test_interleaved_insertion_sorting() {
  let mut trie = HatTrie::new();

  // Insert 'middle', then 'last', then 'first'
  trie.insert("banana", 1);
  trie.insert("carrot", 2);
  trie.insert("apple", 3);

  let keys: Vec<String> = trie.iter().map(|(k, _)| String::from_utf8(k).unwrap()).collect();

  assert_eq!(keys, vec!["apple", "banana", "carrot"]);
}

#[test]
fn test_binary_safety() {
  let mut trie = HatTrie::new();

  // Keys with null bytes and non-utf8
  let k1 = vec![65, 0, 66]; // "A\0B"
  let k2 = vec![65, 0, 67]; // "A\0C"
  let k3 = vec![255, 254]; // Invalid UTF-8

  trie.insert(&k1, "k1");
  trie.insert(&k2, "k2");
  trie.insert(&k3, "k3");

  assert_eq!(trie.get(&k1), Some(&"k1"));
  assert_eq!(trie.get(&k2), Some(&"k2"));
  assert_eq!(trie.get(&k3), Some(&"k3"));
}

// ============================================================================
// 4. Edge Cases
// ============================================================================

#[test]
fn test_empty_key() {
  let mut trie = HatTrie::new();
  trie.insert("", 1337);

  assert_eq!(trie.get(""), Some(&1337));
  assert_eq!(trie.get("a"), None);

  // Empty key should be first in iteration
  trie.insert("a", 1);
  let items: Vec<_> = trie.iter().collect();
  assert_eq!(items[0].1, &1337);
  assert_eq!(items[1].1, &1);
}

#[test]
fn test_prefix_conflicts() {
  // "test" is a prefix of "testing"
  let mut trie = HatTrie::new();

  trie.insert("testing", 1);
  trie.insert("test", 2);
  trie.insert("tester", 3);

  assert_eq!(trie.get("testing"), Some(&1));
  assert_eq!(trie.get("test"), Some(&2));
  assert_eq!(trie.get("tester"), Some(&3));

  // Order check: "test" (len 4) < "tester" (len 6) < "testing" (len 7)
  // based on bytes.
  let keys: Vec<String> = trie.iter().map(|(k, _)| String::from_utf8(k).unwrap()).collect();

  assert_eq!(keys, vec!["test", "tester", "testing"]);
}

// ============================================================================
// 5. Generic Type Support
// ============================================================================

#[test]
fn test_non_copy_values() {
  let mut trie = HatTrie::new();

  trie.insert("foo", String::from("bar"));
  trie.insert("baz", String::from("qux"));

  // Burst it to ensure values are moved correctly
  let mut trie = HatTrie::with_threshold(10);
  for i in 0..100 {
    trie.insert(format!("{}", i), vec![i; 10]);
  }

  assert_eq!(trie.get("50"), Some(&vec![50; 10]));
}

#[test]
fn test_zst_values() {
  // Zero-sized types are tricky for memory calculations
  let mut trie = HatTrie::with_threshold(50);

  for i in 0..100 {
    trie.insert(format!("key-{}", i), ());
  }

  assert_eq!(trie.get("key-99"), Some(&()));
}

// ============================================================================
// 6. Serialization (Requires feature flag)
// ============================================================================

#[test]
#[cfg(feature = "serde")]
fn test_serde_round_trip() {
  let mut trie = HatTrie::with_threshold(1024);
  trie.insert("apple", 1);
  trie.insert("banana", 2);

  let serialized = serde_json::to_string(&trie).unwrap();
  let deserialized: HatTrie<i32> = serde_json::from_str(&serialized).unwrap();

  assert_eq!(deserialized.get("apple"), Some(&1));
  assert_eq!(deserialized.get("banana"), Some(&2));
}

// ============================================================================
// 7. Property-Based Testing (Fuzzing)
// ============================================================================

proptest! {
  // Run 100 random scenarios
  #![proptest_config(ProptestConfig::with_cases(100))]

  #[test]
  fn prop_equivalence_check(
    // Generate a vector of (Key, Value) tuples
    // Keys: random bytes length 0-32
    // Values: random u32
    ops in proptest::collection::vec(
      (proptest::collection::vec(any::<u8>(), 0..32), any::<u32>()),
      0..500 // Up to 500 operations
    )
  ) {
    let mut trie = HatTrie::with_threshold(256); // Low threshold to force bursts
    let mut ref_map = BTreeMap::new();

    // 1. Comparison during Insertion
    for (k, v) in &ops {
      trie.insert(k.clone(), *v);
      ref_map.insert(k.clone(), *v);
    }

    assert_eq!(trie.len(), ref_map.len(), "Length mismatch");

    // 2. Comparison during Retrieval
    for (k, _) in &ops {
      assert_eq!(trie.get(k), ref_map.get(k), "Value mismatch for key {:?}", k);
    }

    // 3. Comparison during Iteration (Order)
    let trie_iter: Vec<_> = trie.iter().map(|(k, v)| (k, *v)).collect();
    let map_iter: Vec<_> = ref_map.iter().map(|(k, v)| (k.clone(), *v)).collect();

    assert_eq!(trie_iter, map_iter, "Iteration order mismatch");
  }
}
