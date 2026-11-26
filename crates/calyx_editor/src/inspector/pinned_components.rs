//! The order the game object inspector lists components in.
//!
//! A project pins components to the top through `[inspector] pinned` in its
//! `project.toml`. Pinned components come first, in list order, and every
//! other component follows alphabetically by display name. Without a list,
//! Game Object then Transform are pinned. The list is read once, when the
//! project loads, and each pin edit writes it back.

use std::collections::HashSet;
use std::path::PathBuf;

use calyx_engine::component::{ComponentID, ComponentTransform};
use calyx_engine::reflect::type_registry::TypeRegistry;
use calyx_engine::utils::TypeUuid;
use calyx_project::{PinnedComponent, Project};
use uuid::Uuid;

use super::component_inspector::type_display_name;

/// A change to the pinned list asked for from a component header's menu.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PinEdit {
    /// Pins the component after the components already pinned.
    Pin,
    Unpin,
    /// Moves the component above the pinned component shown before it.
    MoveUp,
    /// Moves the component below the pinned component shown after it.
    MoveDown,
}

/// What a component header's menu offers about pinning.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PinState {
    pub pinned: bool,
    pub can_move_up: bool,
    pub can_move_down: bool,
}

/// The project's pinned components, as loaded from its `project.toml`.
pub struct PinnedComponents {
    project_directory: PathBuf,
    /// The list from the file, or `None` while the defaults apply. Entries
    /// for types that are not registered are kept.
    entries: Option<Vec<PinnedComponent>>,
}

impl PinnedComponents {
    /// Reads the pinned list of the project at `project_directory`. A
    /// project that fails to load gets the defaults.
    pub fn load(project_directory: impl Into<PathBuf>) -> Self {
        let project_directory = project_directory.into();
        let entries = match Project::load(project_directory.clone()) {
            Ok(project) => project.inspector().pinned.clone(),
            Err(error) => {
                log::error!("Failed to read the inspector's pinned components: {error}");
                None
            }
        };
        Self {
            project_directory,
            entries,
        }
    }

    /// The pinned component types, in order.
    pub fn pinned(&self) -> Vec<Uuid> {
        match &self.entries {
            Some(entries) => entries.iter().map(|entry| entry.uuid).collect(),
            None => vec![ComponentID::type_uuid(), ComponentTransform::type_uuid()],
        }
    }

    /// Orders `components`, given as type UUIDs with their display names:
    /// the pinned ones in list order, then the rest by name.
    pub fn order(&self, components: impl IntoIterator<Item = (Uuid, String)>) -> Vec<Uuid> {
        let mut rest: Vec<(Uuid, String)> = components.into_iter().collect();
        let mut ordered = Vec::with_capacity(rest.len());
        for pinned in self.pinned() {
            if let Some(index) = rest.iter().position(|(uuid, _)| *uuid == pinned) {
                ordered.push(rest.remove(index).0);
            }
        }
        rest.sort_by(|(a_uuid, a_name), (b_uuid, b_name)| {
            a_name.cmp(b_name).then(a_uuid.cmp(b_uuid))
        });
        ordered.extend(rest.into_iter().map(|(uuid, _)| uuid));
        ordered
    }

    /// The header menu state of each of `shown`, the components drawn in
    /// the order [`Self::order`] gave.
    pub fn states(&self, shown: &[Uuid]) -> Vec<PinState> {
        let pinned = self.pinned();
        let shown_pinned = shown.iter().filter(|uuid| pinned.contains(uuid)).count();
        let mut index = 0;
        shown
            .iter()
            .map(|uuid| {
                if !pinned.contains(uuid) {
                    return PinState::default();
                }
                let state = PinState {
                    pinned: true,
                    can_move_up: index > 0,
                    can_move_down: index + 1 < shown_pinned,
                };
                index += 1;
                state
            })
            .collect()
    }

    /// Applies `edit` to `component`, moving it among the pinned components
    /// in `shown`, then saves the list with each registered type's current
    /// display name. A failed save is logged; the new order still holds for
    /// the session.
    pub fn edit(
        &mut self,
        type_registry: &TypeRegistry,
        component: Uuid,
        edit: PinEdit,
        shown: &[Uuid],
    ) {
        let pinned = self.pinned();
        let shown_pinned: Vec<Uuid> = shown
            .iter()
            .copied()
            .filter(|uuid| pinned.contains(uuid))
            .collect();
        let mut entries = self.entries.take().unwrap_or_else(|| {
            pinned
                .iter()
                .map(|uuid| PinnedComponent {
                    uuid: *uuid,
                    name: String::new(),
                })
                .collect()
        });
        // A type listed twice shows once, at its first entry; drop the rest
        // so a move reorders the entry that decides where it shows.
        let mut seen = HashSet::new();
        entries.retain(|entry| seen.insert(entry.uuid));
        let position = |entries: &[PinnedComponent], uuid: Uuid| {
            entries.iter().position(|entry| entry.uuid == uuid)
        };
        let neighbour = |offset: isize| {
            let index = shown_pinned.iter().position(|uuid| *uuid == component)?;
            shown_pinned.get(index.checked_add_signed(offset)?).copied()
        };
        match edit {
            PinEdit::Pin => {
                if position(&entries, component).is_none() {
                    entries.push(PinnedComponent {
                        uuid: component,
                        name: String::new(),
                    });
                }
            }
            PinEdit::Unpin => entries.retain(|entry| entry.uuid != component),
            PinEdit::MoveUp | PinEdit::MoveDown => {
                let up = edit == PinEdit::MoveUp;
                if let (Some(other), Some(from)) = (
                    neighbour(if up { -1 } else { 1 }),
                    position(&entries, component),
                ) {
                    let entry = entries.remove(from);
                    if let Some(to) = position(&entries, other) {
                        entries.insert(if up { to } else { to + 1 }, entry);
                    }
                }
            }
        }
        for entry in entries.iter_mut() {
            if let Some(name) = type_display_name(type_registry, entry.uuid) {
                entry.name = name;
            }
        }
        self.entries = Some(entries);
        if let Err(error) = self.save() {
            log::error!("Failed to save the inspector's pinned components: {error}");
        }
    }

    /// Writes the pinned list into the project's `project.toml`, keeping
    /// every other setting the file holds.
    fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut project = Project::load(self.project_directory.clone())?;
        project.inspector_mut().pinned = self.entries.clone();
        project.save()
    }
}

#[cfg(test)]
mod tests;
