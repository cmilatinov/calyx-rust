//! Edit reporting of every registered type inspector, driven through egui
//! with simulated input.

use calyx_engine::assets::texture::Texture;
use calyx_engine::assets::{AssetAccess, AssetRef};
use calyx_engine::component::{
    ComponentCollider, ComponentID, ComponentRigidBody, ComponentTransform, ParticleBlendMode,
    ParticleSpawnShape,
};
use calyx_engine::math::{Curve, CurveInterpolation, Gradient};
use calyx_engine::reflect::Reflect;
use calyx_engine::scene::GameObjectRef;
use calyx_engine::utils::TypeUuid;
use egui::accesskit::Role;
use egui::{Color32, Pos2, Ui, Vec2};
use nalgebra_glm::Vec3;
use rapier3d::dynamics::RigidBodyType;
use uuid::Uuid;

use super::inspector_registry::InspectorRegistry;
use super::test_harness::{any_changed, combined, InspectorHarness};
use super::type_inspector::{InspectorContext, InspectorResponse, TypeInspector};

const AWAY: Pos2 = Pos2::new(780.0, 580.0);

fn lookup(registry: &InspectorRegistry, type_uuid: Uuid) -> &dyn TypeInspector {
    registry
        .type_inspector_lookup(type_uuid)
        .unwrap_or_else(|| panic!("no inspector registered for {type_uuid}"))
}

fn assert_idle(responses: &[InspectorResponse]) {
    assert!(
        responses.iter().all(InspectorResponse::is_none),
        "idle frames reported {responses:?}"
    );
}

/// A drag reports its start before its stop, and a change between them.
fn assert_drag(responses: &[InspectorResponse]) {
    let all = combined(responses);
    assert!(
        all.changed && all.drag_started && all.drag_stopped,
        "drag reported {responses:?}"
    );
    let start = responses.iter().position(|r| r.drag_started).unwrap();
    let stop = responses.iter().rposition(|r| r.drag_stopped).unwrap();
    let change = responses.iter().position(|r| r.changed).unwrap();
    assert!(
        start <= change && change <= stop,
        "drag reported {responses:?}"
    );
}

/// A discrete edit reports one change and no drag.
fn assert_discrete(responses: &[InspectorResponse]) {
    assert_eq!(
        responses.iter().filter(|r| r.changed).count(),
        1,
        "discrete edit reported {responses:?}"
    );
}

/// Shows `value` with the inspector registered for its type.
macro_rules! draw {
    ($inspector:expr, $value:expr) => {
        &mut |ui: &mut Ui, ctx: &InspectorContext| $inspector.show_inspector(ui, ctx, $value)
    };
}

#[test]
fn bool_inspector_reports_a_click() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, bool::type_uuid());
    let mut value = false;

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let checkbox = h.widget(Role::CheckBox, 0);
    let responses = h.click(checkbox, draw!(inspector, &mut value));
    assert_discrete(&responses);
    assert_idle(&h.idle(2, draw!(inspector, &mut value)));
    assert!(value);
}

#[test]
fn float_inspector_reports_a_drag() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, f32::type_uuid());
    let mut value = 1.0f32;

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let field = h.widget(Role::SpinButton, 0);
    let responses = h.drag(field, Vec2::new(40.0, 0.0), draw!(inspector, &mut value));
    assert_drag(&responses);
    assert_ne!(value, 1.0);
}

#[test]
fn integer_inspector_reports_a_drag() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, i32::type_uuid());
    let mut value = 0i32;

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let field = h.widget(Role::SpinButton, 0);
    let responses = h.drag(field, Vec2::new(40.0, 0.0), draw!(inspector, &mut value));
    assert_drag(&responses);
    assert_ne!(value, 0);
}

#[test]
fn string_inspector_reports_typing_between_focus_and_blur() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, String::type_uuid());
    let mut value = String::from("name");

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let field = h.widget(Role::TextInput, 0);
    let mut responses = h.click(field, draw!(inspector, &mut value));
    assert!(
        combined(&responses).drag_started,
        "focus reported {responses:?}"
    );
    assert!(!any_changed(&responses), "focus reported {responses:?}");
    responses.extend(h.type_text("x", draw!(inspector, &mut value)));
    responses.extend(h.type_text("y", draw!(inspector, &mut value)));
    responses.extend(h.click(AWAY, draw!(inspector, &mut value)));
    assert_drag(&responses);
    assert_eq!(responses.iter().filter(|r| r.changed).count(), 2);
    assert!(value.contains("xy"), "value {value}");
}

#[test]
fn uuid_inspector_reports_nothing_because_it_is_read_only() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, Uuid::type_uuid());
    let original = Uuid::new_v4();
    let mut value = original;

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let field = h.widget(Role::TextInput, 0);
    let mut responses = h.click(field, draw!(inspector, &mut value));
    responses.extend(h.type_text("x", draw!(inspector, &mut value)));
    responses.extend(h.click(AWAY, draw!(inspector, &mut value)));
    assert_idle(&responses);
    assert_eq!(value, original);
}

#[test]
fn color_inspector_reports_a_change_in_its_picker() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, Color32::type_uuid());
    let mut value = Color32::from_rgb(10, 20, 30);

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let button = h.widget(Role::ColorWell, 0);
    let responses = h.click(button, draw!(inspector, &mut value));
    assert!(!any_changed(&responses), "opening reported {responses:?}");
    let channel = h.widget(Role::SpinButton, 0);
    let responses = h.drag(channel, Vec2::new(40.0, 0.0), draw!(inspector, &mut value));
    assert!(any_changed(&responses), "picker reported {responses:?}");
    assert_ne!(value, Color32::from_rgb(10, 20, 30));
}

#[test]
fn vec_inspector_reports_a_drag() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, Vec3::type_uuid());
    let mut value = Vec3::zeros();

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let y = h.widget(Role::SpinButton, 1);
    let responses = h.drag(y, Vec2::new(40.0, 0.0), draw!(inspector, &mut value));
    assert_drag(&responses);
    assert_eq!(value.x, 0.0);
    assert_ne!(value.y, 0.0);
}

#[test]
fn transform_inspector_reports_position_and_rotation_drags() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, ComponentTransform::type_uuid());
    let mut value = ComponentTransform::default();

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let position_x = h.widget(Role::SpinButton, 0);
    let responses = h.drag(
        position_x,
        Vec2::new(40.0, 0.0),
        draw!(inspector, &mut value),
    );
    assert_drag(&responses);
    assert_ne!(value.transform.position.x, 0.0);

    let rotation = value.transform.rotation;
    let rotation_y = h.widget(Role::SpinButton, 4);
    let responses = h.drag(
        rotation_y,
        Vec2::new(40.0, 0.0),
        draw!(inspector, &mut value),
    );
    assert_drag(&responses);
    assert_ne!(value.transform.rotation, rotation);
    assert_idle(&h.idle(2, draw!(inspector, &mut value)));
}

#[test]
fn transform_context_menu_reports_a_reset() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, ComponentTransform::type_uuid());
    let mut value = ComponentTransform::default();
    value.transform.position = Vec3::new(1.0, 2.0, 3.0);
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        inspector.show_inspector_context(ui, ctx, value.as_reflect_mut())
    };

    assert_idle(&h.idle(3, &mut draw));
    let reset = h.labelled(Role::Button, "Reset");
    assert_discrete(&h.click(reset, &mut draw));
    assert_eq!(value.transform.position, Vec3::zeros());
}

#[test]
fn collider_inspector_reports_a_toggle_and_a_drag() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, ComponentCollider::type_uuid());
    let mut value = ComponentCollider::default();
    let enabled = value.enabled;

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let checkbox = h.widget(Role::CheckBox, 0);
    assert_discrete(&h.click(checkbox, draw!(inspector, &mut value)));
    assert_eq!(value.enabled, !enabled);

    let friction = value.friction;
    let field = h.widget(Role::SpinButton, 0);
    assert_drag(&h.drag(field, Vec2::new(40.0, 0.0), draw!(inspector, &mut value)));
    assert!(value.friction != friction || value.shape != ComponentCollider::default().shape);
}

#[test]
fn rigid_body_inspector_reports_a_toggle_and_a_type_choice() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, ComponentRigidBody::type_uuid());
    let mut value = ComponentRigidBody::default();

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let checkbox = h.widget(Role::CheckBox, 0);
    assert_discrete(&h.click(checkbox, draw!(inspector, &mut value)));
    assert!(!value.enabled);

    let combo = h.widget(Role::ComboBox, 0);
    assert!(!any_changed(&h.click(combo, draw!(inspector, &mut value))));
    let fixed = h.labelled(Role::Button, "Fixed");
    assert_discrete(&h.click(fixed, draw!(inspector, &mut value)));
    assert_eq!(value.ty, RigidBodyType::Fixed);
}

#[test]
fn blend_mode_inspector_reports_a_choice() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, ParticleBlendMode::type_uuid());
    let mut value = ParticleBlendMode::Additive;

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let combo = h.widget(Role::ComboBox, 0);
    assert!(!any_changed(&h.click(combo, draw!(inspector, &mut value))));
    let alpha = h.labelled(Role::Button, "Alpha");
    assert_discrete(&h.click(alpha, draw!(inspector, &mut value)));
    assert_eq!(value, ParticleBlendMode::Alpha);
}

#[test]
fn particle_spawn_shape_inspector_reports_a_choice_and_a_drag() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, ParticleSpawnShape::type_uuid());
    let mut value = ParticleSpawnShape::default();

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let combo = h.widget(Role::ComboBox, 0);
    assert!(!any_changed(&h.click(combo, draw!(inspector, &mut value))));
    let shape = h.labelled(Role::Button, "Box");
    assert_discrete(&h.click(shape, draw!(inspector, &mut value)));
    assert!(matches!(value, ParticleSpawnShape::Box { .. }));

    let extent = h.widget(Role::SpinButton, 0);
    assert_drag(&h.drag(extent, Vec2::new(40.0, 0.0), draw!(inspector, &mut value)));
    assert_ne!(
        value,
        ParticleSpawnShape::Box {
            extents: Vec3::from_element(0.5)
        }
    );
}

#[test]
fn game_object_ref_inspector_reports_a_choice() {
    let mut h = InspectorHarness::new();
    let target = h.scene.create(None, None);
    h.scene
        .write_component::<ComponentID, _>(target, |id| id.name = "Target".into());
    let target_id = h.scene.uuid(target);
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, GameObjectRef::type_uuid());
    let mut value = GameObjectRef::default();

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let combo = h.widget(Role::ComboBox, 0);
    assert!(!any_changed(&h.click(combo, draw!(inspector, &mut value))));
    let option = h.labelled(Role::Button, "Target");
    assert_discrete(&h.click(option, draw!(inspector, &mut value)));
    assert_eq!(value.id(), target_id);
}

#[test]
fn asset_ref_inspector_reports_a_choice() {
    let mut h = InspectorHarness::new();
    let render_context = h.asset_context.render_context.clone();
    let texture = h
        .asset_context
        .registries
        .assets
        .read()
        .create(
            "inspector_texture".into(),
            Texture::solid_color_2d(render_context, "inspector_texture", [255, 0, 0, 255]),
        )
        .unwrap();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, AssetRef::<Texture>::type_uuid());
    let mut value = AssetRef::<Texture>::default();
    let mut draw = |ui: &mut Ui, ctx: &InspectorContext| {
        inspector.show_inspector(ui, ctx, value.as_reflect_mut())
    };

    assert_idle(&h.idle(3, &mut draw));
    let combo = h.widget(Role::ComboBox, 0);
    assert!(!any_changed(&h.click(combo, &mut draw)));
    let option = h
        .widgets(Role::Button)
        .into_iter()
        .find(|node| {
            node.label
                .as_deref()
                .is_some_and(|label| label.contains("inspector_texture"))
        })
        .expect("the texture should be listed")
        .rect
        .center();
    assert_discrete(&h.click(option, &mut draw));
    assert_eq!(value.id(), texture.id());
}

#[test]
fn curve_inspector_reports_a_key_drag() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, Curve::type_uuid());
    let mut value = Curve::linear(0.0, 1.0);

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let key = h.labelled_any("Curve key 1");
    let responses = h.drag(key, Vec2::new(-200.0, 20.0), draw!(inspector, &mut value));
    assert_drag(&responses);
    assert_eq!(value.keys.len(), 2);
    assert!(value.keys[1].time < 1.0, "{value:?}");
    assert!(value.keys[1].value < 1.0, "{value:?}");
    assert!(value.keys[0].time <= value.keys[1].time);
}

#[test]
fn curve_inspector_adds_a_key_on_double_click() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, Curve::type_uuid());
    let mut value = Curve::constant(0.0);

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let graph = h.labelled_any("Curve graph");
    assert_discrete(&h.double_click(graph, draw!(inspector, &mut value)));
    assert_eq!(value.keys.len(), 3);
    let middle = value.keys[1];
    assert!(middle.time > 0.0 && middle.time < 1.0, "{value:?}");
    assert!(middle.value > 0.0, "{value:?}");
    assert_idle(&h.idle(2, draw!(inspector, &mut value)));
}

#[test]
fn curve_inspector_menu_sets_interpolation_and_deletes_a_key() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, Curve::type_uuid());
    let mut value = Curve::linear(0.0, 1.0);

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let key = h.labelled_any("Curve key 0");
    assert!(!any_changed(
        &h.secondary_click(key, draw!(inspector, &mut value))
    ));
    let smooth = h.labelled(Role::RadioButton, "Smooth");
    assert_discrete(&h.click(smooth, draw!(inspector, &mut value)));
    assert_eq!(value.keys[0].interpolation, CurveInterpolation::Smooth);

    let delete = h.labelled(Role::Button, "Delete Key");
    assert_discrete(&h.click(delete, draw!(inspector, &mut value)));
    assert_eq!(value.keys.len(), 1);
    assert_eq!(value.keys[0].value, 1.0);
}

#[test]
fn gradient_inspector_reports_a_key_drag() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, Gradient::type_uuid());
    let mut value = Gradient::default();

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let key = h.labelled_any("Gradient key 1");
    let responses = h.drag(key, Vec2::new(-200.0, 0.0), draw!(inspector, &mut value));
    assert_drag(&responses);
    assert_eq!(value.keys.len(), 2);
    assert!(value.keys[1].time < 1.0, "{value:?}");
}

#[test]
fn gradient_inspector_adds_a_key_on_a_strip_click() {
    let mut h = InspectorHarness::new();
    let registry = h.inspector_registry();
    let inspector = lookup(&registry, Gradient::type_uuid());
    let mut value = Gradient::default();

    assert_idle(&h.idle(3, draw!(inspector, &mut value)));
    let strip = h.labelled_any("Gradient strip");
    // The strip's middle is above its markers.
    let point = strip - Vec2::new(0.0, 6.0);
    assert_discrete(&h.click(point, draw!(inspector, &mut value)));
    assert_eq!(value.keys.len(), 3);
    assert!((value.keys[1].color.w - 0.5).abs() < 0.05, "{value:?}");
}
