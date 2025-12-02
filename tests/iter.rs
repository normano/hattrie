use hattrie::HatTrie;

#[test]
fn test_from_iter() {
  let data = vec![("key1".to_string(), 10), ("key2".to_string(), 20)];

  let trie: HatTrie<i32> = data.into_iter().collect();

  assert_eq!(trie.len(), 2);
  assert_eq!(trie.get("key1"), Some(&10));
}

#[test]
fn test_extend() {
  let mut trie = HatTrie::new();
  trie.insert("a", 1);

  let other_data = vec![("b".to_string(), 2), ("c".to_string(), 3)];

  trie.extend(other_data);

  assert_eq!(trie.len(), 3);
  assert_eq!(trie.get("a"), Some(&1));
  assert_eq!(trie.get("b"), Some(&2));
  assert_eq!(trie.get("c"), Some(&3));
}
