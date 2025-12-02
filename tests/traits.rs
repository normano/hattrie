use hattrie::HatTrie;

struct NotCloneable;

#[test]
fn test_non_cloneable_values_work() {
  let mut trie = HatTrie::new();

  trie.insert("key", NotCloneable);

  assert!(trie.get("key").is_some());
}

#[test]
fn test_clone_independence() {
  let mut original = HatTrie::new();
  original.insert("key", 1);

  let mut clone = original.clone();
  clone.insert("key", 2); // Modify clone

  assert_eq!(original.get("key"), Some(&1));
  assert_eq!(clone.get("key"), Some(&2));
}

#[test]
fn test_partial_eq_structural_independence() {
  // Create two tries with different thresholds.
  // They will have totally different memory layouts (one flat, one deep).
  let mut small_buckets = HatTrie::with_threshold(10);
  let mut large_buckets = HatTrie::with_threshold(1024);

  let data = vec![
    ("a", 1),
    ("b", 2),
    ("c", 3),
    ("long_string_to_force_split_in_small_bucket", 4),
  ];

  for (k, v) in &data {
    small_buckets.insert(k, *v);
    large_buckets.insert(k, *v);
  }

  // They should compare equal despite internal structure
  assert_eq!(small_buckets, large_buckets);

  // Modify one
  small_buckets.insert("d", 5);
  assert_ne!(small_buckets, large_buckets);
}

#[test]
fn test_into_iter_move() {
  let mut trie = HatTrie::new();
  // String is not Copy, ensuring we are really moving ownership
  trie.insert("a", "value_a".to_string());
  trie.insert("b", "value_b".to_string());

  let collected: Vec<(Vec<u8>, String)> = trie.into_iter().collect();

  assert_eq!(collected.len(), 2);
  assert_eq!(collected[0].1, "value_a");
}

#[test]
fn test_debug_format() {
  let mut trie = HatTrie::new();
  trie.insert("a", 1);
  let debug_str = format!("{:?}", trie);
  // Just verify it doesn't crash and contains struct name
  assert!(debug_str.contains("HatTrie"));
}
