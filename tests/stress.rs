use hattrie::HatTrie;

#[test]
fn test_deep_recursion_stack() {
  // We want to force a very deep trie.
  // We use a small threshold and construct keys that share long prefixes.
  let mut trie = HatTrie::with_threshold(10);

  // Depth 500
  let mut key = Vec::new();
  for i in 0..500 {
    key.push(0); // Add a byte
    // Insert a value at every level of depth
    trie.insert(&key, i);
  }

  assert_eq!(trie.len(), 500);
  assert_eq!(trie.get(&vec![0; 500]), Some(&499));

  // Cleanup - verify drop doesn't stack overflow
  drop(trie);
}

#[test]
fn test_zst_behavior() {
  // HatTrie used as a Set (Value = ())
  // This is tricky because size_in_bytes calculations might return 0 for values.
  let mut trie: HatTrie<()> = HatTrie::with_threshold(50);

  for i in 0..1000 {
    trie.insert(format!("key-{}", i), ());
  }

  assert_eq!(trie.len(), 1000);
  assert_eq!(trie.get("key-999"), Some(&()));

  // Verify bursting happened (should handle ZST correctly)
  // We can't check internal structure easily, but correctness implies it worked.
}
