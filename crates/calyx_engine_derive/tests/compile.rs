#[test]
fn type_uuid_derive() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/type_uuid_malformed.rs");
    t.compile_fail("tests/ui/type_uuid_non_string.rs");
    t.compile_fail("tests/ui/extern_type_uuid_malformed.rs");
    t.pass("tests/ui/type_uuid_valid.rs");
}

#[test]
fn resource_derive() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/resource_not_struct.rs");
    t.pass("tests/ui/resource_plain_struct.rs");
}

#[test]
fn reflect_derive() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/reflect_union.rs");
    t.compile_fail("tests/ui/reflect_bad_attrs.rs");
    t.compile_fail("tests/ui/reflect_bad_group.rs");
}

#[test]
fn deserialize_with_context_derive() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/deserialize_context_shapes.rs");
}
