use std::collections::HashSet;

use egui::{PopupCloseBehavior, Ui};
use engine::context::ReadOnlyRegistryContext;
use re_ui::list_item::LabelContent;
use re_ui::UiExt;
use uuid::Uuid;

use crate::inspector::component_inspector::display_name;
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
                    let search = search.trim().to_owned();
                    let mut shown = 0usize;
                    for (type_uuid, component) in assets.components.read().components() {
                        if entity_components.contains(type_uuid) {
                            continue;
                        }
                        let name = display_name(&assets.types.read(), component.as_reflect());
                        if !component_matches_search(name, &search) {
                            continue;
                        }
                        shown += 1;
                        if ui.selectable_label(false, name).clicked() {
                            component_to_add = Some(*type_uuid);
                            ui.memory_mut(|mem| mem.close_popup());
                        }
                    }

                    if shown == 0 {
                        ui.weak("No matching components");
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

fn component_matches_search(name: &str, search: &str) -> bool {
    let search = search.trim();
    search.is_empty() || name.to_lowercase().contains(&search.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::component_matches_search;

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
