use super::{AssetError, AssetErrorKind};
use std::error::Error;

const KINDS: [(AssetErrorKind, &str); 5] = [
    (AssetErrorKind::NotFound, "asset not found"),
    (AssetErrorKind::LoadError, "asset load failed"),
    (AssetErrorKind::AlreadyExists, "asset already exists"),
    (AssetErrorKind::TypeMismatch, "asset type mismatch"),
    (AssetErrorKind::Incomplete, "asset source incomplete"),
];

#[test]
fn formats_every_kind_with_every_combination_of_context() {
    for (kind, label) in KINDS {
        for with_type in [false, true] {
            for with_path in [false, true] {
                for with_source in [false, true] {
                    let mut error = AssetError::new(kind);
                    let mut expected = label.to_string();
                    if with_type {
                        error = error.with_type("Texture");
                        expected.push_str(" for Texture");
                    }
                    if with_path {
                        error = error.with_path("textures/brick.png");
                        expected.push_str(" at textures/brick.png");
                    }
                    if with_source {
                        error = error.with_source("unexpected end of file");
                        expected.push_str(": unexpected end of file");
                    }
                    assert_eq!(error.to_string(), expected);
                }
            }
        }
    }
}

#[test]
fn prebuilt_constants_format_as_their_kind() {
    assert_eq!(AssetError::NotFound.to_string(), "asset not found");
    assert_eq!(AssetError::LoadError.to_string(), "asset load failed");
    assert_eq!(
        AssetError::AlreadyExists.to_string(),
        "asset already exists"
    );
    assert_eq!(AssetError::TypeMismatch.to_string(), "asset type mismatch");
    assert_eq!(
        AssetError::Incomplete.to_string(),
        "asset source incomplete"
    );
}

#[test]
fn string_source_is_part_of_the_message_not_an_error_chain() {
    let error = AssetError::LoadError.with_source("bad header");
    assert!(error.source().is_none());
}
