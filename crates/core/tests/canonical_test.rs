use duon_core::canonical::{canonicalize_json, compute_result_hash};
use serde_json::json;

#[test]
fn test_canonical_json_key_sorting() {
    let uncanonical1 = json!({
        "z": 1,
        "a": "hello",
        "m": {
            "b": 2,
            "a": 1
        }
    });

    let uncanonical2 = json!({
        "a": "hello",
        "m": {
            "a": 1,
            "b": 2
        },
        "z": 1
    });

    let s1 = canonicalize_json(&uncanonical1);
    let s2 = canonicalize_json(&uncanonical2);

    assert_eq!(s1, s2);
    assert_eq!(s1, r#"{"a":"hello","m":{"a":1,"b":2},"z":1}"#);
}

#[test]
fn test_result_hash_stability() {
    let data1 = json!({
        "party_a": "吾立方公司",
        "amount": 1600
    });

    let data2 = json!({
        "amount": 1600,
        "party_a": "吾立方公司"
    });

    let evidence = json!({
        "/party_a": [{ "page": 1, "quote": "吾立方公司" }]
    });

    let hash1 = compute_result_hash(&data1, &evidence, "1.0.0", "Qwen3-8B");
    let hash2 = compute_result_hash(&data2, &evidence, "1.0.0", "Qwen3-8B");

    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 64); // SHA-256 hex string
}
