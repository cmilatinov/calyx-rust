//! The components a scene or prefab load skipped, reported once per load.
//!
//! Loading a scene, rebuilding it after a project assembly reload, or
//! instantiating a prefab skips each component whose type is not registered
//! or whose data does not deserialize. Saving afterwards writes the scene
//! without it, so every load lists what it skipped in one warning.

use uuid::Uuid;

use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::TypeInfo;

/// Why a load skipped a component.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DropReason {
    /// No component with the type UUID is registered.
    #[error("its type is not a registered component")]
    Unregistered,
    /// The component is registered but its data does not deserialize.
    #[error("its data does not deserialize: {0}")]
    Invalid(String),
}

/// A component a load skipped.
///
/// The message reads `<type_uuid> on object '<object_name>' (<object_id>): [<type_name>, ]<reason>`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{type_uuid} on object '{}' ({object_id}): {}{reason}",
    object_label(.object_name),
    type_name_prefix(.type_name)
)]
pub struct DroppedComponent {
    /// The UUID of the object the component belonged to, as stored.
    pub object_id: Uuid,
    /// The object's name, empty when it has none.
    pub object_name: String,
    /// The component's type UUID.
    pub type_uuid: Uuid,
    /// The component's type name, when the type registry knows it.
    pub type_name: Option<String>,
    pub reason: DropReason,
}

/// The object's name, or `(unnamed)` when it has none.
fn object_label(object_name: &str) -> &str {
    if object_name.is_empty() {
        "(unnamed)"
    } else {
        object_name
    }
}

/// The type name followed by `, `, or nothing when the name is unknown.
fn type_name_prefix(type_name: &Option<String>) -> String {
    type_name
        .as_ref()
        .map(|type_name| format!("{type_name}, "))
        .unwrap_or_default()
}

/// The type name `types` holds for `type_uuid`, if any.
pub(crate) fn registered_type_name(types: &TypeRegistry, type_uuid: Uuid) -> Option<String> {
    match types.type_info_by_id(type_uuid)? {
        TypeInfo::Struct(info) => Some(info.type_name.to_owned()),
        _ => None,
    }
}

/// The warning that reports `dropped` for one load of `what`, or `None` when
/// the load dropped nothing.
pub fn dropped_components_report(what: &str, dropped: &[DroppedComponent]) -> Option<String> {
    if dropped.is_empty() {
        return None;
    }
    let mut report = format!(
        "Loading {what} skipped {} component(s); saving will not write them:",
        dropped.len()
    );
    for component in dropped {
        report.push_str("\n  - ");
        report.push_str(&component.to_string());
    }
    Some(report)
}

/// Logs [`dropped_components_report`] as one warning.
pub(crate) fn log_dropped_components(what: &str, dropped: &[DroppedComponent]) {
    if let Some(report) = dropped_components_report(what, dropped) {
        log::warn!("{report}");
    }
}
