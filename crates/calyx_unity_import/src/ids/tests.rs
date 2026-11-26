use super::*;

#[test]
fn matches_the_rfc_example_and_pythons_uuid5() {
    // Python: uuid.uuid5(uuid.NAMESPACE_DNS, "python.org")
    let dns = Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap();
    assert_eq!(
        uuid_v5(&dns, b"python.org").to_string(),
        "886313e1-3b8a-5372-9b90-0c9aee199e5d"
    );
}
