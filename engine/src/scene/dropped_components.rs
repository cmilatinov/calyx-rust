//! The components a scene or prefab load skipped, reported once per load.
//!
//! Loading a scene, rebuilding it after a project assembly reload, or
//! instantiating a prefab skips each component whose type is not registered
//! or whose data does not deserialize. Saving afterwards writes the scene
//! without it, so every load lists what it skipped in one warning.

use std::fmt;

use uuid::Uuid;

use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::TypeInfo;

/// Why a load skipped a component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropReason {
    /// No component with the type UUID is registered.
    Unregistered,
    /// The component is registered but its data does not deserialize.
    Invalid(String),
}

impl fmt::Display for DropReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unregistered => f.write_str("its type is not a registered component"),
            Self::Invalid(error) => write!(f, "its data does not deserialize: {error}"),
        }
    }
}

/// A component a load skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl fmt::Display for DroppedComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = if self.object_name.is_empty() {
            "(unnamed)"
        } else {
            self.object_name.as_str()
        };
        write!(
            f,
            "{} on object '{name}' ({}): ",
            self.type_uuid, self.object_id
        )?;
        if let Some(type_name) = &self.type_name {
            write!(f, "{type_name}, ")?;
        }
        write!(f, "{}", self.reason)
    }
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
