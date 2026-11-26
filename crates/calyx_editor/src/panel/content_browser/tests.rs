use super::{breadcrumb_names, file_name_str};
use std::path::Path;

#[test]
fn file_name_str_returns_last_component() {
    assert_eq!(
        file_name_str(Path::new("assets/props/crate.cxprefab")),
        Some("crate.cxprefab")
    );
    assert_eq!(file_name_str(Path::new("assets/props")), Some("props"));
}

#[test]
fn file_name_str_is_none_without_a_file_name() {
    assert_eq!(file_name_str(Path::new("")), None);
    assert_eq!(file_name_str(Path::new("assets/..")), None);
}

#[test]
fn file_name_str_is_none_for_non_utf8_names() {
    #[cfg(unix)]
    let name = {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        OsStr::from_bytes(b"\xff.png").to_owned()
    };
    #[cfg(windows)]
    let name = {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0xD800, 0x0061])
    };
    assert_eq!(file_name_str(Path::new(&name)), None);
}

#[test]
fn breadcrumb_names_lists_folders_below_root() {
    assert_eq!(
        breadcrumb_names(Path::new("assets"), Path::new("assets/models/props")),
        Some(vec!["models".to_string(), "props".to_string()])
    );
}

#[test]
fn breadcrumb_names_is_empty_at_root() {
    assert_eq!(
        breadcrumb_names(Path::new("assets"), Path::new("assets")),
        Some(Vec::new())
    );
}

#[test]
fn breadcrumb_names_rejects_folders_outside_root() {
    assert_eq!(
        breadcrumb_names(Path::new("assets"), Path::new("sandbox/scenes")),
        None
    );
}
