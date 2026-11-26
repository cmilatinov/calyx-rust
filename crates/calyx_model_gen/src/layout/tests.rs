use super::*;

#[test]
fn the_prefix_is_the_path_under_assets() {
    let layout = Layout::new(Path::new("sandbox/assets/generated")).unwrap();
    assert_eq!(layout.prefix, "generated");
    let nested = Layout::new(Path::new("/x/assets/a/assets/props/clay")).unwrap();
    assert_eq!(nested.prefix, "props/clay");
    assert!(Layout::new(Path::new("sandbox/generated")).is_err());
    assert!(Layout::new(Path::new("sandbox/assets")).is_err());
}

#[test]
fn parent_components_resolve_before_the_prefix_is_taken() {
    for spelling in [
        "sandbox/assets/generated/../generated",
        "sandbox/assets/./generated",
        "sandbox/assets/x/../generated/",
    ] {
        let layout = Layout::new(Path::new(spelling)).unwrap();
        assert_eq!(layout.prefix, "generated", "{spelling}");
    }
    assert!(Layout::new(Path::new("sandbox/assets/generated/..")).is_err());
    assert!(Layout::new(Path::new("sandbox/assets/../generated")).is_err());
}

#[test]
fn a_meta_is_written_as_the_editor_writes_it() {
    let layout = Layout::new(Path::new("assets/generated")).unwrap();
    let crate_box = layout.asset(&["models", "crate"], "obj", TYPE_MESH);
    assert_eq!(
        crate_box.meta(),
        r#"{
  "main": {
    "id": "ab200de6-0ee5-55ab-885c-66964e1f6681",
    "name": "generated/models/crate",
    "display_name": "crate",
    "type_uuid": "792d264b-de6f-4431-b59f-76f18fdb3bfe"
  },
  "inner": []
}"#
    );
}
