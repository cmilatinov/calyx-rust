#[test]
fn type_uuid_derive() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/type_uuid_malformed.rs");
    t.compile_fail("tests/ui/type_uuid_non_string.rs");
    t.pass("tests/ui/type_uuid_valid.rs");
}
