use std::collections::{BTreeMap, HashSet};

use hattrie::HatTrie;
use proptest::prelude::*;

#[test]
fn test_range_query_basic() {
  let mut trie = HatTrie::new();
  trie.insert("a", 1);
  trie.insert("b", 2);
  trie.insert("c", 3);
  trie.insert("d", 4);

  // Inclusive
  let res: Vec<_> = trie.range("b".."d").map(|(k, _)| k).collect();
  assert_eq!(res, vec![b"b", b"c"]); // "d" is excluded in .. range

  // Inclusive End
  let res: Vec<_> = trie.range("b"..="d").map(|(k, _)| k).collect();
  assert_eq!(res, vec![b"b", b"c", b"d"]);

  // Unbounded
  let res: Vec<_> = trie.range("c"..).map(|(k, _)| k).collect();
  assert_eq!(res, vec![b"c", b"d"]);
}

#[test]
fn test_range_query_seek_performance() {
  // Insert "z" then "a".
  let mut trie = HatTrie::new();
  trie.insert("z", 26);
  trie.insert("a", 1);

  // Range starting at "m". Should skip "a".
  let mut iter = trie.range("m"..);
  let (first_key, _) = iter.next().unwrap();
  assert_eq!(first_key, b"z");
}

#[test]
fn test_lower_bound() {
  let mut trie = HatTrie::new();
  trie.insert("apple", 1);
  trie.insert("apricot", 2);

  // lower_bound "app" -> "apple"
  assert_eq!(trie.lower_bound("app").unwrap().0, b"apple");

  // lower_bound "apple" -> "apple"
  assert_eq!(trie.lower_bound("apple").unwrap().0, b"apple");

  // lower_bound "applf" -> "apricot"
  assert_eq!(trie.lower_bound("applf").unwrap().0, b"apricot");
}

#[test]
fn test_fuzzy_search() {
  let mut trie = HatTrie::new();
  trie.insert("apple", 1);
  trie.insert("apply", 2);
  trie.insert("apricot", 3);
  trie.insert("banana", 4);

  // Distance 1 from "apple"
  // Matches: "apple" (0), "apply" (1 - subst 'e'->'y')
  let res = trie.fuzzy_search(b"apple", 1);

  let keys: Vec<String> = res.into_iter().map(|(k, _)| String::from_utf8(k).unwrap()).collect();
  assert!(keys.contains(&"apple".to_string()));
  assert!(keys.contains(&"apply".to_string()));
  assert!(!keys.contains(&"apricot".to_string()));
}

#[test]
fn test_range_bucket_crossing() {
  // Insert enough to force multiple buckets (bursting).
  // With generic threshold, we force it small to ensure structure.
  let mut trie = HatTrie::with_threshold(100);

  // "key-000" to "key-255" roughly.
  for i in 0..200 {
    trie.insert(format!("key-{:03}", i), i);
  }

  // Range from 050 to 150. This likely spans bucket boundaries.
  let range_start = "key-050";
  let range_end = "key-150"; // Exclusive

  let count = trie.range(range_start..range_end).count();
  assert_eq!(count, 100); // 150 - 50 = 100 items

  // Verify exact bounds
  let first = trie.range(range_start..range_end).next().unwrap();
  assert_eq!(first.0, b"key-050");
}

#[test]
fn test_range_prefix_trap() {
  // Test the tricky case where keys are prefixes of each other.
  // Standard lexicographical order: "app" < "apple" < "application"
  let mut trie = HatTrie::new();
  trie.insert("app", 1);
  trie.insert("apple", 2);
  trie.insert("application", 3);

  // Range: "apple" .. "application"
  // "app" should be skipped. "apple" included. "application" excluded.
  let res: Vec<_> = trie
    .range("apple".."application")
    .map(|(k, _)| String::from_utf8(k).unwrap())
    .collect();

  assert_eq!(res, vec!["apple"]);

  // Range: "app" .. "apple"
  // "app" included. "apple" excluded.
  let res: Vec<_> = trie
    .range("app".."apple")
    .map(|(k, _)| String::from_utf8(k).unwrap())
    .collect();
  assert_eq!(res, vec!["app"]);
}

#[test]
fn test_lower_upper_bound() {
  let mut trie = HatTrie::new();
  trie.insert("apple", 1);
  trie.insert("apricot", 2);
  trie.insert("banana", 3);

  // Exact Match
  assert_eq!(trie.lower_bound("apple").unwrap().0, b"apple");
  // Upper bound skips exact
  assert_eq!(trie.upper_bound("apple").unwrap().0, b"apricot");

  // Gap (probe "applf", between "apple" and "apricot")
  assert_eq!(trie.lower_bound("applf").unwrap().0, b"apricot");
  assert_eq!(trie.upper_bound("applf").unwrap().0, b"apricot");

  // Off the end
  assert!(trie.lower_bound("carrot").is_none());
}

#[test]
fn test_cursor_divergence() {
  // Test seeking logic when the key exists in a different subtree.
  let mut trie = HatTrie::new();
  trie.insert("apple", 1);
  trie.insert("banana", 2);

  // "azzz" shares 'a' with "apple", but is greater.
  // The iterator should go down 'a', find it ends at "apple", backtrack, and go to 'b'.
  let res = trie.lower_bound("azzz");
  assert_eq!(res.unwrap().0, b"banana");
}

#[test]
fn test_fuzzy_search_basics() {
  let mut trie = HatTrie::new();
  trie.insert("apple", 1);
  trie.insert("apply", 2);
  trie.insert("apricot", 3);
  trie.insert("banana", 4);

  // Distance 1 from "apple"
  // Matches: "apple" (0), "apply" (1 - subst 'e'->'y')
  let res = trie.fuzzy_search(b"apple", 1);

  let keys: Vec<String> = res.into_iter().map(|(k, _)| String::from_utf8(k).unwrap()).collect();
  assert!(keys.contains(&"apple".to_string()));
  assert!(keys.contains(&"apply".to_string()));
  assert!(!keys.contains(&"apricot".to_string()));
}

// ============================================================================
// 2. Property-Based Fuzzing
// ============================================================================

// --- Helper: Reference Levenshtein Implementation ---
fn reference_levenshtein(a: &[u8], b: &[u8]) -> usize {
  if a.is_empty() {
    return b.len();
  }
  if b.is_empty() {
    return a.len();
  }

  let mut cache = (0..=b.len()).collect::<Vec<_>>();

  for (i, &ca) in a.iter().enumerate() {
    let mut prev_diag = cache[0];
    cache[0] = i + 1;
    for (j, &cb) in b.iter().enumerate() {
      let old_diag = prev_diag;
      prev_diag = cache[j + 1];
      let sub_cost = if ca == cb { 0 } else { 1 };
      cache[j + 1] = std::cmp::min(
        cache[j] + 1, // insert
        std::cmp::min(
          cache[j + 1] + 1, // delete
          old_diag + sub_cost,
        ), // sub
      );
    }
  }
  cache[b.len()]
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(200))]

  // 1. Range Query Equivalence vs BTreeMap
  #[test]
  fn prop_range_equivalence(
    keys in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..20), 1..100),
    range_start in proptest::collection::vec(any::<u8>(), 0..20),
    range_end in proptest::collection::vec(any::<u8>(), 0..20),
  ) {
    let mut trie = HatTrie::with_threshold(128);
    let mut map = BTreeMap::new();

    for (i, k) in keys.iter().enumerate() {
      trie.insert(k.clone(), i);
      map.insert(k.clone(), i);
    }

    // Construct a standard Rust range
    // We use a simplified range logic for testing: start..end
    // Ensure start <= end for valid range, otherwise swap
    let (start, end) = if range_start <= range_end {
      (range_start.clone(), range_end.clone())
    } else {
      (range_end.clone(), range_start.clone())
    };

    // HatTrie infers K=Vec<u8>, BTreeMap infers T=Vec<u8>.
    let trie_range: Vec<_> = trie.range(start.clone()..end.clone())
      .map(|(k, v)| (k, *v))
      .collect();

    let map_range: Vec<_> = map.range(start.clone()..end.clone())
      .map(|(k, v)| (k.clone(), *v))
      .collect();

    assert_eq!(trie_range, map_range, "Range mismatch for {:?}..{:?}", start, end);
  }

  // 2. Cursor Equivalence (Lower Bound)
  #[test]
  fn prop_cursor_lower_bound(
    keys in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..20), 1..100),
    probe in proptest::collection::vec(any::<u8>(), 0..20)
  ) {
    let mut trie = HatTrie::with_threshold(50); // Small threshold to force structure
    let mut map = BTreeMap::new();

    for (i, k) in keys.iter().enumerate() {
      trie.insert(k.clone(), i);
      map.insert(k.clone(), i);
    }

    let trie_res = trie.lower_bound(&probe).map(|(k, v)| (k, *v));

    // BTreeMap equivalent: range(probe..).next()
    let map_res = map.range(probe.clone()..).next().map(|(k, v)| (k.clone(), *v));

    assert_eq!(trie_res, map_res, "Lower bound mismatch for probe {:?}", probe);
  }

  // 3. Fuzzy Search Oracle
  #[test]
  fn prop_fuzzy_search_oracle(
    keys in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..10), 1..100),
    target in proptest::collection::vec(any::<u8>(), 0..10),
    max_dist in 0usize..4usize
  ) {
    let mut trie = HatTrie::new();
    // Use a HashSet to track expected keys (deduplication)
    let mut unique_keys = HashSet::new();

    for (i, k) in keys.iter().enumerate() {
      trie.insert(k.clone(), i);
      unique_keys.insert(k.clone());
    }

    // 1. Calculate Expected Results via Brute Force
    let mut expected = Vec::new();
    for k in &unique_keys {
      if reference_levenshtein(&target, k) <= max_dist {
        expected.push(k.clone());
      }
    }
    expected.sort(); // Sort for comparison

    // 2. Calculate Actual Results
    let mut actual: Vec<_> = trie.fuzzy_search(&target, max_dist)
      .into_iter()
      .map(|(k, _)| k)
      .collect();
    actual.sort();

    assert_eq!(actual, expected, "Fuzzy search mismatch for target {:?} dist {}", target, max_dist);
  }
}
