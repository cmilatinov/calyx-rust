use super::{
    element_view, ComponentInspector, ElementView, FieldTrail, ListEdit, ReflectListContainer,
};
use crate::inspector::edit_tracker::{ComponentKey, InspectorEditTracker};
use crate::inspector::pinned_components::PinState;
use crate::inspector::test_harness::{any_changed, combined, Draw, InspectorHarness};
use crate::inspector::type_inspector::{InspectorContext, InspectorResponse};
use crate::scene_history::{InspectorValueEdit, SceneHistory, SceneHistoryRestore};
use crate::widgets::ListContainer;
use calyx_engine::component::{
    Component, ComponentBone, ComponentCamera, ComponentOutline, ComponentParticleSystem,
    ComponentPointLight, ComponentTonemap, OutlineStyle, TonemappingPreset,
};
use calyx_engine::reflect::type_registry::TypeRegistry;
use calyx_engine::reflect::{ListInfo, Reflect, ReflectDefault, TypeInfo};
use calyx_engine::utils::TypeUuid;
use egui::accesskit::Role;
use egui::{Id, Rect, Ui, Vec2};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Default, Debug, PartialEq, TypeUuid, Reflect, Serialize, Deserialize)]
#[uuid = "0d6f3b2a-9c4e-4a71-b58d-3e2f7a1c9d64"]
struct ListHolder {
    values: Vec<f32>,
}

#[derive(Default, TypeUuid, Reflect)]
#[uuid = "4b0c8e1d-2f7a-4c39-9e65-a1d3b7f02c58"]
struct GroupedHolder {
    first: bool,
    #[reflect_attr(group = "Physics")]
    mass: bool,
    middle: bool,
    #[reflect_attr(group = "Render")]
    color: bool,
    #[reflect_attr(group = "Physics")]
    drag: bool,
    last: bool,
}

#[derive(Default, TypeUuid, Reflect)]
#[uuid = "9e2d5c47-0a6b-4f18-b3c2-6d84e1f7a093"]
struct NestedGroups {
    inner: GroupedHolder,
}

fn values_info(registry: &TypeRegistry) -> &ListInfo {
    let Some(TypeInfo::List(info)) = registry.type_info::<Vec<f32>>() else {
        panic!("expected list info for Vec<f32>");
    };
    info
}

/// Runs `edit` on `holder`'s `values` list the way the inspector does:
/// through the list widget's container, reported as one discrete change
/// to the edit tracker, whose commit becomes the undo step. Returns how
/// many history entries the edit added.
fn inspector_edit(
    registry: &TypeRegistry,
    history: &mut SceneHistory,
    holder: &mut ListHolder,
    edit: impl FnOnce(&mut ReflectListContainer),
) -> usize {
    let key = ComponentKey {
        game_object: Uuid::nil(),
        component: ListHolder::type_uuid(),
    };
    let mut tracker = InspectorEditTracker::default();
    tracker.set_baseline(key, serde_json::to_value(&*holder).ok());
    let TypeInfo::Struct(info) = registry.type_info::<ListHolder>().unwrap() else {
        panic!("expected struct info");
    };
    let field = info.field("values").unwrap();
    let values = field.get_reflect_mut(holder.as_reflect_mut()).unwrap();
    let mut container = ReflectListContainer::new(
        values.as_list_mut().unwrap(),
        registry,
        Some(values_info(registry)),
    );
    edit(&mut container);
    // Element edits report as changes too; structural ones always do.
    assert!(tracker.report(key, InspectorResponse::CHANGED, false));
    let Some(commit) = tracker.commit(key, serde_json::to_value(&*holder).ok()) else {
        return 0;
    };
    assert!(history.push_inspector_value(InspectorValueEdit {
        label: "Edit List Holder".into(),
        game_object: commit.key.game_object,
        component: commit.key.component,
        changes: commit.changes,
    }));
    1
}

fn restore(holder: &mut ListHolder, restore: Option<SceneHistoryRestore>) {
    let Some(SceneHistoryRestore::InspectorValue { values, .. }) = restore else {
        panic!("expected an inspector value restore");
    };
    let mut serialized = serde_json::to_value(&*holder).unwrap();
    for (path, value) in values {
        assert!(crate::set_json_value(&mut serialized, &path, value));
    }
    *holder = serde_json::from_value(serialized).unwrap();
}

#[test]
fn list_edits_round_trip_through_inspector_history() {
    let registry = TypeRegistry::new();
    let mut history = SceneHistory::default();
    let mut holder = ListHolder {
        values: vec![1.0, 2.0],
    };

    let added = inspector_edit(&registry, &mut history, &mut holder, |list| {
        assert!(list.can_add());
        list.push_default();
    });
    assert_eq!(added, 1);
    assert_eq!(holder.values, vec![1.0, 2.0, 0.0]);

    inspector_edit(&registry, &mut history, &mut holder, |list| {
        *list.nth_mut(2).downcast_mut::<f32>().unwrap() = 7.0;
    });
    assert_eq!(holder.values, vec![1.0, 2.0, 7.0]);

    let removed = inspector_edit(&registry, &mut history, &mut holder, |list| {
        list.remove(0);
    });
    assert_eq!(removed, 1);
    assert_eq!(holder.values, vec![2.0, 7.0]);

    restore(&mut holder, history.undo());
    assert_eq!(holder.values, vec![1.0, 2.0, 7.0]);
    restore(&mut holder, history.undo());
    assert_eq!(holder.values, vec![1.0, 2.0, 0.0]);
    restore(&mut holder, history.undo());
    assert_eq!(holder.values, vec![1.0, 2.0]);
    assert!(!history.can_undo());

    restore(&mut holder, history.redo());
    restore(&mut holder, history.redo());
    restore(&mut holder, history.redo());
    assert_eq!(holder.values, vec![2.0, 7.0]);
}

#[test]
fn list_moves_are_one_undo_step() {
    let registry = TypeRegistry::new();
    let mut history = SceneHistory::default();
    let mut holder = ListHolder {
        values: vec![1.0, 2.0, 3.0],
    };

    // A move shifts every element between its ends, all in one step.
    let moved = inspector_edit(&registry, &mut history, &mut holder, |list| {
        list.move_item(0, 2);
    });
    assert_eq!(moved, 1);
    assert_eq!(holder.values, vec![2.0, 3.0, 1.0]);

    restore(&mut holder, history.undo());
    assert_eq!(holder.values, vec![1.0, 2.0, 3.0]);
    assert!(!history.can_undo());
    restore(&mut holder, history.redo());
    assert_eq!(holder.values, vec![2.0, 3.0, 1.0]);
}

#[test]
fn list_inspector_reports_element_edits_and_structural_changes() {
    let mut h = InspectorHarness::new();
    h.type_info = ListHolder::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut holder = ListHolder {
        values: vec![1.0, 2.0],
    };
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        inspector.show_fields(
            ui,
            ctx,
            holder.as_reflect_mut(),
            &FieldTrail::root(Id::new("list_holder")),
        )
    };

    let idle = h.idle(3, &mut draw);
    assert!(idle.iter().all(InspectorResponse::is_none), "{idle:?}");
    let element = h.widget(Role::SpinButton, 0);
    let drag = combined(&h.drag(element, Vec2::new(40.0, 0.0), &mut draw));
    assert!(
        drag.changed && drag.drag_started && drag.drag_stopped,
        "{drag:?}"
    );
    assert!(!any_changed(&h.idle(2, &mut draw)));
    assert_ne!(holder.values[0], 1.0);
    assert_eq!(holder.values.len(), 2);
}

#[test]
fn reflect_list_container_adds_removes_and_moves() {
    let registry = TypeRegistry::new();
    let info = values_info(&registry);
    let mut values = vec![1.0f32, 2.0];
    let list = values.as_reflect_mut().as_list_mut().unwrap();
    let mut container = ReflectListContainer::new(list, &registry, Some(info));

    container.push_default();
    container.move_item(2, 0);
    container.remove(1);
    // Out of range: nothing changes and nothing is logged.
    container.remove(9);
    container.move_item(0, 9);

    assert_eq!(container.len(), 2);
    assert_eq!(
        container.edits,
        vec![
            ListEdit::Add,
            ListEdit::Move { from: 2, to: 0 },
            ListEdit::Remove(1)
        ]
    );
    assert_eq!(values, vec![0.0, 2.0]);
}

#[test]
fn lists_without_list_info_cannot_grow() {
    let registry = TypeRegistry::new();
    let mut values = vec![1.0f32];
    let list = values.as_reflect_mut().as_list_mut().unwrap();
    let mut container = ReflectListContainer::new(list, &registry, None);

    assert!(!container.can_add());
    container.push_default();
    container.remove(0);

    assert_eq!(container.edits, vec![ListEdit::Remove(0)]);
    assert!(values.is_empty());
}

#[test]
fn field_ids_differ_per_list_element() {
    let component = FieldTrail::root(Id::new("component"));
    let rows = component.child("rows");
    let first = rows.child(0).child("slots");
    let second = rows.child(1).child("slots");

    assert_ne!(first.id(), second.id());
    assert_eq!(first.id(), rows.child(0).child("slots").id());
    let other = FieldTrail::root(Id::new("other"));
    assert_ne!(first.id(), other.child("rows").child(0).child("slots").id());
}

#[test]
fn nested_lists_render_as_lists() {
    let registry = TypeRegistry::new();
    let inner = registry.type_info::<Vec<f32>>();
    assert!(matches!(inner, Some(TypeInfo::List(_))));

    assert_eq!(element_view(false, false, inner, true), ElementView::List);
    // An inner list type that no field registered still shows its elements.
    assert_eq!(element_view(false, false, None, true), ElementView::List);
}

#[test]
fn list_elements_prefer_their_type_inspector() {
    let registry = TypeRegistry::new();
    let holder = registry.type_info::<ListHolder>();

    assert_eq!(element_view(true, false, None, false), ElementView::Inline);
    assert_eq!(element_view(true, true, None, false), ElementView::Enum);
    assert_eq!(
        element_view(false, false, holder, false),
        ElementView::Struct
    );
    assert_eq!(element_view(false, false, None, false), ElementView::Label);
}

#[test]
fn grouped_fields_render_in_collapsible_sections() {
    let mut h = InspectorHarness::new();
    h.type_info = GroupedHolder::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut holder = GroupedHolder::default();
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        inspector.show_fields(
            ui,
            ctx,
            holder.as_reflect_mut(),
            &FieldTrail::root(Id::new("grouped_holder")),
        )
    };

    h.idle(3, &mut draw);
    // Each group sits at its first field, ungrouped fields where declared.
    assert_eq!(
        h.labels(),
        ["First", "Physics", "Mass", "Drag", "Middle", "Render", "Color", "Last"]
    );

    let physics = h.labelled_any("Physics");
    h.click(physics, &mut draw);
    h.idle(10, &mut draw);
    assert_eq!(
        h.labels(),
        ["First", "Physics", "Middle", "Render", "Color", "Last"]
    );
}

#[test]
fn nested_struct_fields_render_their_groups() {
    let mut h = InspectorHarness::new();
    h.type_info = NestedGroups::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut nested = NestedGroups::default();
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        inspector.show_fields(
            ui,
            ctx,
            nested.as_reflect_mut(),
            &FieldTrail::root(Id::new("nested_groups")),
        )
    };

    h.idle(3, &mut draw);
    assert_eq!(
        h.labels(),
        ["Inner", "First", "Physics", "Mass", "Drag", "Middle", "Render", "Color", "Last"]
    );
}

#[test]
fn default_inspector_reports_its_fields_edits() {
    let mut h = InspectorHarness::new();
    h.type_info = ComponentCamera::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut camera = ComponentCamera::default();
    let before = serde_json::to_value(&camera).unwrap();
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        let response = inspector.show(ui, ctx, camera.as_reflect_mut(), PinState::default(), None);
        assert!(!response.remove);
        response.edit
    };

    let idle = h.idle(3, &mut draw);
    assert!(idle.iter().all(|r| r.is_none()), "{idle:?}");
    let field = h.widget(Role::SpinButton, 0);
    let drag = combined(&h.drag(field, Vec2::new(40.0, 0.0), &mut draw));
    assert!(
        drag.changed && drag.drag_started && drag.drag_stopped,
        "{drag:?}"
    );
    assert!(!any_changed(&h.idle(2, &mut draw)));

    let after = serde_json::to_value(&camera).unwrap();
    let numbers_changed = ["fov", "near_plane", "far_plane"]
        .iter()
        .filter(|field| before[**field] != after[**field])
        .count();
    assert_eq!(numbers_changed, 1, "{before} -> {after}");
}

/// Draws a camera with the header checkbox in `enabled`'s state, applying
/// each toggle the header reports to it, as the inspector panel does.
fn show_toggleable_camera(
    h: &mut InspectorHarness,
    enabled: &std::cell::Cell<bool>,
    toggles: &std::cell::Cell<usize>,
    frames: impl FnOnce(&mut InspectorHarness, &mut Draw),
) -> ComponentCamera {
    h.type_info = ComponentCamera::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut camera = ComponentCamera::default();
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        let response = inspector.show(
            ui,
            ctx,
            camera.as_reflect_mut(),
            PinState::default(),
            Some(enabled.get()),
        );
        if let Some(state) = response.enabled {
            enabled.set(state);
            toggles.set(toggles.get() + 1);
        }
        response.edit
    };
    frames(h, &mut draw);
    drop(draw);
    camera
}

#[test]
fn header_checkbox_toggles_without_collapsing_the_header() {
    let mut h = InspectorHarness::new();
    let enabled = std::cell::Cell::new(true);
    let toggles = std::cell::Cell::new(0);
    show_toggleable_camera(&mut h, &enabled, &toggles, |h, draw| {
        h.idle(3, draw);
        let labels = h.labels().len();
        assert!(h.labels().contains(&"Fov"), "{:?}", h.labels());

        let checkbox = h.widget(Role::CheckBox, 0);
        h.click(checkbox, draw);
        h.idle(10, draw);
        assert!(!enabled.get(), "the click switched the component off");
        assert_eq!(toggles.get(), 1);
        assert_eq!(h.labels().len(), labels, "the header stayed expanded");

        let checkbox = h.widget(Role::CheckBox, 0);
        h.click(checkbox, draw);
        h.idle(10, draw);
        assert!(enabled.get(), "a second click switched it back on");
        assert_eq!(toggles.get(), 2);
        assert_eq!(h.labels().len(), labels, "the header stayed expanded");
    });
}

#[test]
fn header_checkbox_fits_the_header_row() {
    let mut h = InspectorHarness::new();
    let enabled = std::cell::Cell::new(true);
    let toggles = std::cell::Cell::new(0);
    show_toggleable_camera(&mut h, &enabled, &toggles, |h, draw| {
        h.idle(3, draw);
        let checkbox = h.widgets(Role::CheckBox)[0].rect;
        let header = h
            .nodes()
            .iter()
            .find(|node| node.label.as_deref() == Some("Camera"))
            .unwrap_or_else(|| panic!("no Camera header in {:#?}", h.nodes()))
            .rect;
        let text_size = h.ctx.style().text_styles[&egui::TextStyle::Body].size;
        assert_eq!(checkbox.size(), Vec2::splat(text_size), "{checkbox:?}");
        assert!(
            (checkbox.center().y - header.center().y).abs() < 0.5,
            "{checkbox:?} is centred in {header:?}"
        );
        assert!(header.contains_rect(checkbox), "{checkbox:?} in {header:?}");
    });
}

#[test]
fn a_disabled_component_cannot_be_edited() {
    let mut h = InspectorHarness::new();
    let enabled = std::cell::Cell::new(false);
    let toggles = std::cell::Cell::new(0);
    let camera = show_toggleable_camera(&mut h, &enabled, &toggles, |h, draw| {
        h.idle(3, draw);
        assert!(h.labels().contains(&"Fov"), "the fields still show");
        let field = h.widget(Role::SpinButton, 0);
        let drag = h.drag(field, Vec2::new(40.0, 0.0), draw);
        assert!(!any_changed(&drag), "{drag:?}");
    });
    let defaults = ComponentCamera::default();
    assert_eq!(camera.fov, defaults.fov);
    assert_eq!(camera.near_plane, defaults.near_plane);
    assert_eq!(camera.far_plane, defaults.far_plane);
    assert_eq!(toggles.get(), 0);
}

/// The inspector panel passes an enabled state only for a component type
/// that opts in to disabling, so a bone's header, pinned or not, has no
/// checkbox, while a point light's, which opts in, has one.
#[test]
fn only_components_that_can_be_disabled_have_a_header_checkbox() {
    assert!(!ComponentBone::default().can_disable());
    assert!(ComponentPointLight::default().can_disable());
    for pinned in [false, true] {
        let (_, checkboxes) = header_checkboxes::<ComponentBone>("Bone", pinned);
        assert!(checkboxes.is_empty(), "{checkboxes:#?}");

        let (header, checkboxes) = header_checkboxes::<ComponentPointLight>("Point Light", pinned);
        assert_eq!(checkboxes.len(), 1, "{checkboxes:#?}");
        assert!(
            header.intersects(checkboxes[0]),
            "the checkbox {:?} sits in the header {header:?}",
            checkboxes[0]
        );
    }
}

/// Draws a default `T`'s inspector the way the inspector panel does, with
/// an enabled state only when `T` opts in to disabling. Returns the rect of
/// the header labelled `label` and the rect of every checkbox drawn.
fn header_checkboxes<T: Component + Default + TypeUuid>(
    label: &str,
    pinned: bool,
) -> (Rect, Vec<Rect>) {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut component = T::default();
    h.type_info = T::type_uuid();
    let enabled = component.can_disable().then_some(true);
    let pin = PinState {
        pinned,
        ..PinState::default()
    };
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        inspector
            .show(ui, ctx, component.as_reflect_mut(), pin, enabled)
            .edit
    };
    h.idle(3, &mut draw);
    let header = h
        .nodes()
        .iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no {label} header in {:#?}", h.nodes()))
        .rect;
    let checkboxes = h
        .widgets(Role::CheckBox)
        .iter()
        .map(|checkbox| checkbox.rect)
        .collect();
    (header, checkboxes)
}

/// The particle system's size module shows its curve in the curve editor,
/// and dragging a key edits only the size curve.
#[test]
fn particle_size_curve_edits_in_the_curve_editor() {
    let mut h = InspectorHarness::new();
    h.type_info = ComponentParticleSystem::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut system = ComponentParticleSystem::default();
    let before = system.size.curve.clone();
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        inspector.show_fields(
            ui,
            ctx,
            system.as_reflect_mut(),
            &FieldTrail::root(Id::new("particle_system")),
        )
    };

    h.idle(3, &mut draw);
    let graphs = h
        .labels()
        .iter()
        .filter(|label| **label == "Curve graph")
        .count();
    assert_eq!(graphs, 1, "{:?}", h.labels());
    let key = h.labelled_any("Curve key 1");
    let drag = combined(&h.drag(key, Vec2::new(-200.0, -20.0), &mut draw));
    assert!(
        drag.changed && drag.drag_started && drag.drag_stopped,
        "{drag:?}"
    );
    assert_ne!(system.size.curve, before);
    assert_eq!(system.color, ComponentParticleSystem::default().color);
}

/// The particle system's color module shows one gradient strip, and
/// dragging one of its keys or clicking the strip to add one edits only
/// the color.
#[test]
fn particle_color_edits_in_the_gradient_editor() {
    let mut h = InspectorHarness::new();
    h.type_info = ComponentParticleSystem::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut system = ComponentParticleSystem::default();
    let defaults = ComponentParticleSystem::default();
    // Scrolled so the color module, below the screen's height, shows.
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        egui::ScrollArea::vertical()
            .vertical_scroll_offset(500.0)
            .show(ui, |ui| {
                re_ui::list_item::list_item_scope(ui, "particle_system", |ui| {
                    inspector.show_fields(
                        ui,
                        ctx,
                        system.as_reflect_mut(),
                        &FieldTrail::root(Id::new("particle_system")),
                    )
                })
            })
            .inner
    };

    h.idle(3, &mut draw);
    let strips: Vec<egui::Rect> = h
        .nodes()
        .iter()
        .filter(|node| node.label.as_deref() == Some("Gradient strip"))
        .map(|node| node.rect)
        .collect();
    assert_eq!(strips.len(), 1, "{:?}", h.labels());
    assert!(
        strips[0].max.y < 600.0,
        "the strip is on screen: {strips:?}"
    );

    let key = h.labelled_any("Gradient key 1");
    let drag = combined(&h.drag(key, Vec2::new(-80.0, 0.0), &mut draw));
    assert!(
        drag.changed && drag.drag_started && drag.drag_stopped,
        "{drag:?}"
    );
    let strip = h.labelled_any("Gradient strip") - Vec2::new(0.0, 6.0);
    let add = h.click(strip, &mut draw);
    assert_eq!(add.iter().filter(|r| r.changed).count(), 1, "{add:?}");
    drop(draw);

    let keys = &system.color.gradient.keys;
    assert_eq!(keys.len(), 3, "{keys:?}");
    assert!(keys[2].time < 1.0, "the dragged key moved: {keys:?}");
    assert_eq!(system.size, defaults.size);
    assert_eq!(system.emission, defaults.emission);
    assert_eq!(system.rendering.blend_mode, defaults.rendering.blend_mode);
}

/// In a narrow inspector laid out as `PanelInspector` lays it out, the
/// particle system's fields fit the panel: its scroll area has nothing to
/// scroll sideways, the curve editor and the gradient strip end before the
/// panel's right edge and every row starts after its left edge.
#[test]
fn particle_system_fields_fit_a_narrow_inspector() {
    let mut h = InspectorHarness::new();
    h.type_info = ComponentParticleSystem::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut system = ComponentParticleSystem::default();
    let panel = egui::Rect::from_min_size(egui::pos2(40.0, 0.0), egui::vec2(320.0, 4000.0));
    let content = std::cell::Cell::new((0.0f32, egui::Rect::NOTHING));
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        let mut response = InspectorResponse::NONE;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(panel), |ui| {
            ui.set_clip_rect(panel);
            let output = egui::ScrollArea::both()
                .auto_shrink([true, true])
                .show(ui, |ui| {
                    // As `PanelInspector` lays out its content.
                    egui::Frame {
                        fill: ui.style().visuals.panel_fill,
                        inner_margin: re_ui::DesignTokens::panel_margin(),
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        re_ui::list_item::list_item_scope(ui, "narrow_inspector", |ui| {
                            response = inspector.show_fields(
                                ui,
                                ctx,
                                system.as_reflect_mut(),
                                &FieldTrail::root(Id::new("narrow_particle_system")),
                            );
                        });
                        ui.allocate_space(ui.available_size());
                    });
                });
            content.set((output.content_size.x, output.inner_rect));
        });
        response
    };

    h.idle(4, &mut draw);
    let (content_width, viewport) = content.get();
    assert!(
        content_width <= viewport.width() + 0.5,
        "content is {content_width} wide in a {} wide inspector",
        viewport.width()
    );
    for editor in ["Curve graph", "Gradient strip"] {
        let rects: Vec<egui::Rect> = h
            .nodes()
            .iter()
            .filter(|node| node.label.as_deref() == Some(editor))
            .map(|node| node.rect)
            .collect();
        assert!(!rects.is_empty(), "the {editor} is drawn");
        for rect in rects {
            assert!(
                rect.max.x <= viewport.max.x + 0.5,
                "the {editor} ends at {} past the inspector's right edge {}",
                rect.max.x,
                viewport.max.x
            );
        }
    }
    let left = h
        .nodes()
        .iter()
        .filter(|node| node.label.is_some())
        .map(|node| node.rect.min.x)
        .fold(f32::INFINITY, f32::min);
    assert!(
        left >= viewport.min.x - 0.5,
        "a row starts at {left}, left of the inspector at {}",
        viewport.min.x
    );
}

#[derive(Clone, Copy, Debug, Default, PartialEq, TypeUuid, Reflect, Serialize, Deserialize)]
#[uuid = "73077ef6-6460-4a4e-8051-26d76df35176"]
#[reflect(Default)]
enum TestQuality {
    #[default]
    Low,
    Medium,
    #[reflect_attr(name = "Very High")]
    VeryHigh,
}

#[derive(Default, Debug, PartialEq, TypeUuid, Reflect, Serialize, Deserialize)]
#[uuid = "cec6d20c-45f6-4b51-936a-40c181610eb9"]
struct QualityHolder {
    quality: TestQuality,
    levels: Vec<TestQuality>,
}

#[test]
fn unit_enum_fields_pick_their_variant_from_a_dropdown() {
    let mut h = InspectorHarness::new();
    h.type_info = QualityHolder::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let holder = std::cell::RefCell::new(QualityHolder::default());
    let key = ComponentKey {
        game_object: Uuid::nil(),
        component: QualityHolder::type_uuid(),
    };
    let tracker = std::cell::RefCell::new(InspectorEditTracker::default());
    tracker
        .borrow_mut()
        .set_baseline(key, serde_json::to_value(&*holder.borrow()).ok());
    let commits = std::cell::RefCell::new(Vec::new());
    // Each frame's response goes through the edit tracker, as the
    // inspector panel does, and every committed edit is kept.
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        let response = inspector.show_fields(
            ui,
            ctx,
            holder.borrow_mut().as_reflect_mut(),
            &FieldTrail::root(Id::new("quality_holder")),
        );
        let pointer_down = ui.input(|input| input.pointer.any_down());
        let mut tracker = tracker.borrow_mut();
        if tracker.report(key, response, pointer_down) {
            let current = serde_json::to_value(&*holder.borrow()).ok();
            commits.borrow_mut().extend(tracker.commit(key, current));
        }
        response
    };

    h.idle(3, &mut draw);
    let dropdown = &h.widgets(Role::ComboBox)[0];
    assert_eq!(dropdown.label.as_deref(), Some("Quality"));
    assert_eq!(dropdown.value.as_deref(), Some("Low"));

    h.click(h.widget(Role::ComboBox, 0), &mut draw);
    let options = h
        .labels()
        .into_iter()
        .filter(|label| ["Low", "Medium", "Very High"].contains(label))
        .collect::<Vec<_>>();
    assert_eq!(options, ["Low", "Medium", "Very High"]);

    let picked = h.click(h.labelled(Role::Button, "Very High"), &mut draw);
    assert!(any_changed(&picked), "{picked:?}");
    assert!(picked.iter().all(|response| !response.drag_started));
    assert_eq!(holder.borrow().quality, TestQuality::VeryHigh);
    h.idle(3, &mut draw);
    let dropdown = &h.widgets(Role::ComboBox)[0];
    assert_eq!(dropdown.value.as_deref(), Some("Very High"));

    // The pick is one edit of the field, and undoing it restores the
    // variant it replaced.
    let commits = commits.take();
    assert_eq!(commits.len(), 1);
    let mut history = SceneHistory::default();
    assert!(history.push_inspector_value(InspectorValueEdit {
        label: "Edit Quality Holder".into(),
        game_object: commits[0].key.game_object,
        component: commits[0].key.component,
        changes: commits[0].changes.clone(),
    }));
    let Some(SceneHistoryRestore::InspectorValue { values, .. }) = history.undo() else {
        panic!("expected an inspector value restore");
    };
    let mut serialized = serde_json::to_value(&*holder.borrow()).unwrap();
    for (path, value) in values {
        assert!(crate::set_json_value(&mut serialized, &path, value));
    }
    let restored: QualityHolder = serde_json::from_value(serialized).unwrap();
    assert_eq!(restored.quality, TestQuality::Low);
}

#[test]
fn unit_enum_list_elements_pick_their_variant_from_a_dropdown() {
    let mut h = InspectorHarness::new();
    h.type_info = QualityHolder::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut holder = QualityHolder {
        quality: TestQuality::Low,
        levels: vec![TestQuality::Medium],
    };
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        inspector.show_fields(
            ui,
            ctx,
            holder.as_reflect_mut(),
            &FieldTrail::root(Id::new("quality_list")),
        )
    };

    h.idle(3, &mut draw);
    let element = h.labelled(Role::ComboBox, "Element 0");
    h.click(element, &mut draw);
    let picked = h.click(h.labelled(Role::Button, "Low"), &mut draw);
    assert!(any_changed(&picked), "{picked:?}");
    assert_eq!(holder.levels, vec![TestQuality::Low]);
}

/// The Outline effect labels its style list "Style 1", "Style 2", ...,
/// the numbers materials pick them by, while a list without the label
/// attributes keeps "Element 0", "Element 1", ...
#[test]
fn the_outline_style_list_counts_from_style_1_and_other_lists_from_element_0() {
    let mut h = InspectorHarness::new();
    h.type_info = ComponentOutline::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut outline = ComponentOutline {
        styles: vec![OutlineStyle::default(), OutlineStyle::default()],
        ..Default::default()
    };
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        inspector.show_fields(
            ui,
            ctx,
            outline.as_reflect_mut(),
            &FieldTrail::root(Id::new("outline_styles")),
        )
    };

    h.idle(3, &mut draw);
    let labels = h.labels();
    assert!(labels.contains(&"Style 1"), "{labels:?}");
    assert!(labels.contains(&"Style 2"), "{labels:?}");
    assert!(
        !labels.iter().any(|label| label.starts_with("Element")),
        "{labels:?}"
    );

    let mut h = InspectorHarness::new();
    h.type_info = QualityHolder::type_uuid();
    let registry = h.inspector_registry();
    let inspector = ComponentInspector::new(&registry);
    let mut holder = QualityHolder {
        quality: TestQuality::Low,
        levels: vec![TestQuality::Medium, TestQuality::Low],
    };
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        inspector.show_fields(
            ui,
            ctx,
            holder.as_reflect_mut(),
            &FieldTrail::root(Id::new("quality_list")),
        )
    };

    h.idle(3, &mut draw);
    h.labelled(Role::ComboBox, "Element 0");
    h.labelled(Role::ComboBox, "Element 1");
}

/// The Tone Mapping component's `preset` shows its variant, both as an
/// object's component and as a profile's effect entry, which the
/// profile inspector draws unpinned, and the neutral curve is picked
/// from it.
#[test]
fn tone_mapping_picks_its_preset_from_a_dropdown() {
    for pinned in [true, false] {
        let mut h = InspectorHarness::new();
        h.type_info = ComponentTonemap::type_uuid();
        let registry = h.inspector_registry();
        let inspector = ComponentInspector::new(&registry);
        let tonemap = std::cell::RefCell::new(ComponentTonemap::default());
        let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
            let mut tonemap = tonemap.borrow_mut();
            let instance = tonemap.as_reflect_mut();
            let response = if pinned {
                inspector.show(ui, ctx, instance, PinState::default(), Some(true))
            } else {
                inspector.show_unpinned(ui, ctx, instance, Some(true))
            };
            response.edit
        };

        h.idle(3, &mut draw);
        let dropdowns = h.widgets(Role::ComboBox);
        assert_eq!(dropdowns.len(), 1, "{:#?}", h.nodes());
        assert_eq!(dropdowns[0].label.as_deref(), Some("Preset"));
        assert_eq!(dropdowns[0].value.as_deref(), Some("ACES"));
        assert!(!h.labels().contains(&"TonemappingPreset"));

        h.click(h.widget(Role::ComboBox, 0), &mut draw);
        let options = h
            .labels()
            .into_iter()
            .filter(|label| ["ACES", "Neutral"].contains(label))
            .collect::<Vec<_>>();
        assert_eq!(options, ["ACES", "Neutral"]);
        let picked = h.click(h.labelled(Role::Button, "Neutral"), &mut draw);
        assert!(any_changed(&picked), "{picked:?}");
        assert_eq!(tonemap.borrow().preset, TonemappingPreset::Neutral);
    }
}

#[test]
fn unit_enum_elements_draw_inline() {
    let registry = TypeRegistry::new();
    let quality = registry.type_info::<TestQuality>();
    assert_eq!(
        element_view(false, false, quality, false),
        ElementView::UnitEnum
    );
    // A type inspector still wins.
    assert_eq!(element_view(true, false, quality, false), ElementView::Enum);
}
