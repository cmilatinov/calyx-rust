use super::ResourceType;

#[test]
fn declared_types_parse_with_their_defaults() {
    let read = ResourceType::parse_declared("storage<array<vec4f>>").unwrap();
    assert_eq!(
        read,
        ResourceType::new("storage", Some("read"), "array<vec4f>")
    );
    let write = ResourceType::parse_declared("storage< array<vec4f, 4> , read_write >").unwrap();
    assert_eq!(
        write,
        ResourceType::new("storage", Some("read_write"), "array<vec4f,4>")
    );
    let uniform = ResourceType::parse_declared("uniform<Params>").unwrap();
    assert_eq!(uniform.display(), "uniform<Params>");
    assert!(ResourceType::parse_declared("uniform<T, read>").is_none());
    assert!(ResourceType::parse_declared("storage<>").is_none());
}
