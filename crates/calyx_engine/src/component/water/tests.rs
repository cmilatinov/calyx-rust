use super::*;
use crate::reflect::TypeInfo;
use crate::scene::SceneData;
use crate::test_support::test_registries;

#[test]
fn water_is_registered_and_round_trips_through_a_scene() {
    let registries = test_registries();
    assert!(registries
        .components
        .read()
        .component(ComponentWater::type_uuid())
        .is_some());
    let mut scene = registries.scene();
    let owner = scene.create(None, None);
    let water = ComponentWater {
        points: vec![
            WaterPoint {
                position: vec2(1.0, 2.0),
                width: 3.0,
            },
            WaterPoint {
                position: vec2(4.0, 9.0),
                width: 5.5,
            },
        ],
        cell_size: 0.5,
        shading: WaterShading::Faceted,
        obstacles: vec![WaterObstacle {
            position: vec2(2.0, 4.0),
            radius: 0.75,
        }],
        openings: vec![WaterObstacle {
            position: vec2(4.0, 9.0),
            radius: 2.5,
        }],
        wave_amplitude: 0.2,
        wave_length: 4.5,
        flow_speed: 0.4,
        deep_color: Color32::from_rgb(1, 2, 3),
        shallow_color: Color32::from_rgb(4, 5, 6),
        foam_color: Color32::from_rgb(7, 8, 9),
        shore_distance: 2.5,
        foam_width: 0.75,
        foam_patches: 0.6,
        obstacle_foam: 0.3,
        obstacle_shallows: 0.8,
        facet_variation: 0.3,
        roughness: 0.45,
        glint_strength: 4.0,
        outline_style: 2,
        bank_material: AssetRef::from_id(uuid::Uuid::new_v4()),
        bank_height: 0.7,
        bank_depth: 0.5,
        bank_run: 0.2,
        bank_lip: 0.9,
    };
    scene.add_component(owner, water.clone());
    let data: SceneData = (&scene).into();
    let json = serde_json::to_value(&data).unwrap();
    let restored: SceneData = serde_json::from_value(json).unwrap();
    let value = &restored.components[&scene.uuid(owner)][&ComponentWater::type_uuid()];
    let restored: ComponentWater = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored, water);
    // A field a saved scene lacks takes its default.
    let partial: ComponentWater =
        serde_json::from_value(serde_json::json!({ "cell_size": 3.0 })).unwrap();
    assert_eq!(
        partial,
        ComponentWater {
            cell_size: 3.0,
            ..Default::default()
        }
    );
    // The default river bends and meshes.
    assert!(ComponentWater::default().path().geometry().is_some());
}

#[test]
fn every_field_is_reflected_for_the_inspector() {
    let registries = test_registries();
    let types = registries.types.read();
    let Some(TypeInfo::Struct(info)) = types.type_info::<ComponentWater>() else {
        panic!("the water is a reflected struct");
    };
    let names: Vec<&str> = info.fields.keys().copied().collect();
    assert_eq!(
        names,
        [
            "points",
            "cell_size",
            "shading",
            "obstacles",
            "openings",
            "wave_amplitude",
            "wave_length",
            "flow_speed",
            "deep_color",
            "shallow_color",
            "foam_color",
            "shore_distance",
            "foam_width",
            "foam_patches",
            "obstacle_foam",
            "obstacle_shallows",
            "facet_variation",
            "roughness",
            "glint_strength",
            "outline_style",
            "bank_material",
            "bank_height",
            "bank_depth",
            "bank_run",
            "bank_lip",
        ]
    );
    let Some(TypeInfo::Struct(point)) = types.type_info::<WaterPoint>() else {
        panic!("a point is a reflected struct");
    };
    assert!(point.field("position").is_some() && point.field("width").is_some());
    // The inspector edits a field through its reflection.
    let mut water = ComponentWater::default();
    info.field("wave_amplitude")
        .unwrap()
        .set(water.as_reflect_mut(), 0.5f32)
        .unwrap();
    info.field("deep_color")
        .unwrap()
        .set(water.as_reflect_mut(), Color32::RED)
        .unwrap();
    let points = info
        .field("points")
        .unwrap()
        .get_reflect_mut(water.as_reflect_mut())
        .unwrap();
    assert_eq!(points.as_list().unwrap().len(), 4);
    assert_eq!(water.wave_amplitude, 0.5);
    assert_eq!(water.deep_color, Color32::RED);
}
