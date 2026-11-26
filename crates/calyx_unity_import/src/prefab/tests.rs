use super::*;
use crate::package::tests::package_bytes;

const PREFAB: &str = "--- !u!1 &10
GameObject:
  m_Component:
  - component: {fileID: 40}
  m_Name: crate
  m_IsActive: 1
--- !u!4 &40
Transform:
  m_LocalRotation: {x: 0, y: 0, z: 0, w: 1}
  m_LocalPosition: {x: 0, y: 0.5, z: 0}
  m_LocalScale: {x: 1, y: 1, z: 1}
  m_Father: {fileID: 0}
--- !u!33 &330
MeshFilter:
  m_Mesh: {fileID: 4300000, guid: fbx1, type: 3}
--- !u!23 &230
MeshRenderer:
  m_CastShadows: 1
  m_Materials:
  - {fileID: 2100000, guid: mat1, type: 2}
";

/// A prefab variant of the model file itself, as Unity writes one: it
/// zeroes the root transform and sets the material.
const VARIANT: &str = "--- !u!1001 &7
PrefabInstance:
  m_Modification:
    m_TransformParent: {fileID: 0}
    m_Modifications:
    - target: {fileID: -8679921383154817045, guid: fbx2, type: 3}
      propertyPath: m_LocalPosition.x
      value: 0
      objectReference: {fileID: 0}
    - target: {fileID: -7511558181221131132, guid: fbx2, type: 3}
      propertyPath: 'm_Materials.Array.data[0]'
      value:
      objectReference: {fileID: 2100000, guid: mat1, type: 2}
    - target: {fileID: 919132149155446097, guid: fbx2, type: 3}
      propertyPath: m_Name
      value: barrel
      objectReference: {fileID: 0}
  m_SourcePrefab: {fileID: 100100000, guid: fbx2, type: 3}
";

fn package() -> UnityPackage {
    let bytes = package_bytes(&[
        ("fbx1", "Assets/P/Models/crate.fbx", Some("fbx"), None),
        ("fbx2", "Assets/P/Models/barrel_raw.fbx", Some("fbx"), None),
        ("pre1", "Assets/P/Prefabs/crate.prefab", Some(PREFAB), None),
        (
            "var1",
            "Assets/P/Prefabs/barrel.prefab",
            Some(VARIANT),
            None,
        ),
    ]);
    UnityPackage::from_reader(bytes.as_slice()).unwrap()
}

fn modification(path: &str, value: &str) -> Value {
    crate::yaml::parse_documents(&format!(
        "--- !u!1 &1\nX:\n  list:\n  - propertyPath: {path}\n    value: {value}\n"
    ))[0]
        .body
        .get("list")
        .unwrap()
        .items()[0]
        .clone()
}

#[test]
fn a_prefab_places_its_mesh_material_and_transform() {
    let placed = resolve(&package(), "pre1", &mut HashMap::new()).unwrap();
    assert_eq!(placed.name, "crate");
    assert_eq!(placed.mesh.as_deref(), Some("fbx1"));
    assert_eq!(placed.material.as_deref(), Some("mat1"));
    assert_eq!(placed.position[1], Json::Float(0.5));
    assert!(placed.active && placed.shadows);
}

#[test]
fn a_variant_applies_its_modifications_over_the_model_it_derives_from() {
    let placed = resolve(&package(), "var1", &mut HashMap::new()).unwrap();
    assert_eq!(placed.name, "barrel");
    assert_eq!(placed.mesh.as_deref(), Some("fbx2"));
    assert_eq!(placed.material.as_deref(), Some("mat1"));
    // Set by the variant: a float; left as the model's default: integers.
    assert_eq!(placed.position[0], Json::Float(0.0));
    assert_eq!(placed.position[1], Json::Int(0));
    assert_eq!(placed.scale, [Json::Int(1), Json::Int(1), Json::Int(1)]);
}

#[test]
fn a_scene_instance_moves_renames_hides_and_unshadows_its_copy() {
    let mut cache = HashMap::new();
    let mut placed = resolve(&package(), "pre1", &mut cache).unwrap();
    placed.apply(&[
        modification("m_LocalPosition.z", "12.5"),
        modification("m_LocalRotation.w", "0.5"),
        modification("m_LocalScale.y", "2"),
        modification("m_Name", "crate (3)"),
        modification("m_IsActive", "0"),
        modification("m_CastShadows", "0"),
        modification("m_LocalEulerAnglesHint.y", "90"),
    ]);
    assert_eq!(placed.position[2], Json::Float(12.5));
    assert_eq!(placed.rotation[3], Json::Float(0.5));
    assert_eq!(placed.scale[1], Json::Float(2.0));
    assert_eq!(placed.name, "crate (3)");
    assert!(!placed.active && !placed.shadows);
    // The cached prefab is untouched.
    assert_eq!(cache["pre1"].name, "crate");
}
