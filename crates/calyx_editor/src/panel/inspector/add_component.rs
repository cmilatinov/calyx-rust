use std::collections::{BTreeMap, HashSet};

use calyx_engine::context::ReadOnlyRegistryContext;
use egui::{PopupCloseBehavior, Ui};
use re_ui::list_item::LabelContent;
use re_ui::UiExt;
use uuid::Uuid;

use crate::inspector::component_inspector::{component_category, display_name};
use crate::inspector::widgets::{SearchSelectState, Widgets};

/// The inspector's "Add Component" button and its searchable list of the
/// registered components an object does not have yet.
#[derive(Default)]
pub(super) struct AddComponentButton {
    select: SearchSelectState,
}

impl AddComponentButton {
    /// Draws the button and its popup. Returns the type UUID of the
    /// component picked this frame, if any, among those not in
    /// `entity_components`.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        assets: &ReadOnlyRegistryContext,
        entity_components: &HashSet<Uuid>,
    ) -> Option<Uuid> {
        let num_components = assets.components.read().components().count();
        let enabled = num_components > entity_components.len();
        let mut component_to_add = None;
        let res = ui
            .list_item()
            .draggable(false)
            .interactive(enabled)
            .show_flat(
                ui,
                LabelContent::new(" Add Component")
                    .always_show_buttons(true)
                    .truncate(true)
                    .with_icon(&re_ui::icons::ADD),
            )
            .on_hover_text("Add a new component to this game object");
        let id = ui.make_persistent_id("add_component_popup");
        if res.clicked() && enabled {
            self.select.open();
            ui.memory_mut(|mem| mem.open_popup(id));
        }
        egui::popup::popup_below_widget(
            ui,
            id,
            &res,
            PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                Widgets::search_select_contents(ui, id, &mut self.select, |ui, search| {
                    let candidates = {
                        let types = assets.types.read();
                        let components = assets.components.read();
                        components
                            .components()
                            .filter(|(type_uuid, _)| !entity_components.contains(type_uuid))
                            .map(|(type_uuid, component)| MenuEntry {
                                type_uuid: *type_uuid,
                                name: display_name(&types, component.as_reflect()),
                                category: component_category(&types, component.as_reflect()),
                            })
                            .collect::<Vec<_>>()
                    };
                    let groups = menu_groups(candidates, search);
                    if groups.is_empty() {
                        ui.weak("No matching components");
                    }
                    for (category, entries) in groups {
                        if let Some(category) = category {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(category).small().weak());
                        }
                        for entry in entries {
                            if ui.selectable_label(false, entry.name).clicked() {
                                component_to_add = Some(entry.type_uuid);
                                ui.memory_mut(|mem| mem.close_popup());
                            }
                        }
                    }
                });
            },
        );
        if component_to_add.is_some() {
            self.select.clear_search();
        }
        component_to_add
    }
}

/// A component the add-component menu can offer.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MenuEntry {
    type_uuid: Uuid,
    name: &'static str,
    category: Option<&'static str>,
}

/// The entries matching `search`, grouped for the menu: those without a
/// category first, then one group per category in name order, each sorted
/// by name. Groups left empty by the search are dropped.
fn menu_groups(
    entries: impl IntoIterator<Item = MenuEntry>,
    search: &str,
) -> Vec<(Option<&'static str>, Vec<MenuEntry>)> {
    let mut groups: BTreeMap<Option<&'static str>, Vec<MenuEntry>> = BTreeMap::new();
    for entry in entries
        .into_iter()
        .filter(|entry| component_matches_search(entry.name, search))
    {
        groups.entry(entry.category).or_default().push(entry);
    }
    groups
        .into_iter()
        .map(|(category, mut entries)| {
            entries.sort_by_key(|entry| entry.name);
            (category, entries)
        })
        .collect()
}

fn component_matches_search(name: &str, search: &str) -> bool {
    let search = search.trim();
    search.is_empty() || name.to_lowercase().contains(&search.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::{component_matches_search, menu_groups, MenuEntry};
    use uuid::Uuid;

    fn entry(name: &'static str, category: Option<&'static str>) -> MenuEntry {
        MenuEntry {
            type_uuid: Uuid::new_v4(),
            name,
            category,
        }
    }

    fn names(
        groups: &[(Option<&'static str>, Vec<MenuEntry>)],
    ) -> Vec<(Option<&'static str>, Vec<&'static str>)> {
        groups
            .iter()
            .map(|(category, entries)| {
                (*category, entries.iter().map(|entry| entry.name).collect())
            })
            .collect()
    }

    #[test]
    fn the_menu_lists_uncategorised_components_first_then_each_category() {
        let post = Some("Post Processing");
        let entries = [
            entry("Bloom", post),
            entry("Camera", None),
            entry("Global Post Process Volume", post),
            entry("Animator", None),
            entry("Tone Mapping", post),
        ];
        assert_eq!(
            names(&menu_groups(entries, "")),
            [
                (None, vec!["Animator", "Camera"]),
                (
                    post,
                    vec!["Bloom", "Global Post Process Volume", "Tone Mapping"]
                ),
            ]
        );
        assert_eq!(
            names(&menu_groups(entries, "bloom")),
            [(post, vec!["Bloom"])],
            "a group the search empties is dropped"
        );
        assert!(menu_groups(entries, "rigid").is_empty());
    }

    #[test]
    fn component_search_matches_case_insensitive_substrings() {
        assert!(component_matches_search("Tank Controller", "tank"));
        assert!(component_matches_search("Directional Light", "LIGHT"));
        assert!(!component_matches_search("Component Mesh", "camera"));
    }

    #[test]
    fn component_search_treats_blank_query_as_match() {
        assert!(component_matches_search("Camera", ""));
        assert!(component_matches_search("Camera", "   "));
    }
}
