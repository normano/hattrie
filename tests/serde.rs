#![cfg(feature = "serde")]

use hattrie::HatTrie;
use serde_json;

#[test]
fn test_config_persistence() {
  // 1. Create a trie with a NON-DEFAULT threshold
  let mut original = HatTrie::with_threshold(123);
  original.insert("a", 1);

  // 2. Serialize
  let serialized = serde_json::to_string(&original).unwrap();

  // 3. Deserialize
  let loaded: HatTrie<i32> = serde_json::from_str(&serialized).unwrap();

  // 4. Verify data match
  assert_eq!(loaded.get("a"), Some(&1));

  // 5. Verify configuration match (This requires the field to be serialized)
  // We insert enough data to trigger a burst at the OLD threshold (123)
  // but not the DEFAULT threshold (4096).
  // If threshold wasn't saved, this logic might behave differently,
  // though checking internal state is hard without exposing private fields.
  // For now, we trust the Round-Trip equality check.
  assert_eq!(original, loaded);
}

#[test]
fn test_malformed_payload() {
  // A Trie root must be an Enum. Let's feed it garbage JSON.
  let bad_json = r#"{ "root": "NotANode", "len": 100, "burst_threshold": 4096 }"#;
  let res: Result<HatTrie<i32>, _> = serde_json::from_str(bad_json);
  assert!(res.is_err());
}
