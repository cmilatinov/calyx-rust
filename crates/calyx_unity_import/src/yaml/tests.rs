use super::*;

const PREFAB: &str = "%YAML 1.1
%TAG !u! tag:unity3d.com,2011:
--- !u!1 &100
GameObject:
  m_Component:
  - component: {fileID: 400}
  - component: {fileID: 2300}
  m_Name: crate
  m_IsActive: 1
--- !u!4 &400
Transform:
  m_GameObject: {fileID: 100}
  m_LocalRotation: {x: 0, y: -0.5, z: 0, w: 0.8660254}
  m_LocalPosition: {x: 1.5, y: 0, z: -2}
  m_Children: []
  m_Father: {fileID: 0}
--- !u!4 &500 stripped
Transform:
  m_PrefabInstance: {fileID: 600}
";

#[test]
fn documents_keep_their_class_file_id_and_stripped_flag() {
    let documents = parse_documents(PREFAB);
    let headers: Vec<_> = documents
        .iter()
        .map(|d| (d.class_id, d.file_id, d.stripped, d.class_name.as_str()))
        .collect();
    assert_eq!(
        headers,
        vec![
            (1, 100, false, "GameObject"),
            (4, 400, false, "Transform"),
            (4, 500, true, "Transform"),
        ]
    );
}

#[test]
fn block_and_flow_values_parse_into_maps_lists_and_text() {
    let documents = parse_documents(PREFAB);
    let game_object = &documents[0].body;
    assert_eq!(game_object.str_at("m_Name"), Some("crate"));
    let components = game_object.get("m_Component").unwrap().items();
    assert_eq!(components.len(), 2);
    assert_eq!(
        components[1].get("component").unwrap().str_at("fileID"),
        Some("2300")
    );
    let transform = &documents[1].body;
    let rotation = transform.get("m_LocalRotation").unwrap();
    assert_eq!(rotation.f64_at("y"), Some(-0.5));
    assert_eq!(transform.get("m_Children"), Some(&Value::List(vec![])));
    assert_eq!(transform.get("m_Father").unwrap().i64_at("fileID"), Some(0));
}

#[test]
fn sequences_of_mappings_and_quoted_scalars_parse() {
    let text = "--- !u!1001 &7
PrefabInstance:
  m_Modification:
    m_Modifications:
    - target: {fileID: 1, guid: abc, type: 3}
      propertyPath: 'm_Materials.Array.data[0]'
      value:
      objectReference: {fileID: 2100000, guid: def, type: 2}
    - target: {fileID: 1, guid: abc, type: 3}
      propertyPath: m_Name
      value: barrel (2)
      objectReference: {fileID: 0}
  m_SourcePrefab: {fileID: 100100000, guid: abc, type: 3}
";
    let documents = parse_documents(text);
    let modifications = documents[0]
        .body
        .get("m_Modification")
        .unwrap()
        .get("m_Modifications")
        .unwrap()
        .items();
    assert_eq!(modifications.len(), 2);
    assert_eq!(
        modifications[0].str_at("propertyPath"),
        Some("m_Materials.Array.data[0]")
    );
    assert_eq!(
        modifications[0]
            .get("objectReference")
            .unwrap()
            .str_at("guid"),
        Some("def")
    );
    assert_eq!(modifications[1].str_at("value"), Some("barrel (2)"));
}

#[test]
fn meta_files_parse_from_column_zero() {
    let meta = parse_meta(
        "fileFormatVersion: 2\nguid: abc\nModelImporter:\n  tangentSpace:\n    normalSmoothAngle: 0\n    normalImportMode: 1\n",
    );
    let tangent_space = meta
        .get("ModelImporter")
        .unwrap()
        .get("tangentSpace")
        .unwrap();
    assert_eq!(tangent_space.i64_at("normalImportMode"), Some(1));
    assert_eq!(tangent_space.f64_at("normalSmoothAngle"), Some(0.0));
}
