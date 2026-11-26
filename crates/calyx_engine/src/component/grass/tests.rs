use super::*;
use crate::test_support::test_registries;

#[test]
fn grass_field_is_registered_and_saves_no_benders() {
    let registries = test_registries();
    assert!(registries
        .components
        .read()
        .component(ComponentGrassField::type_uuid())
        .is_some());
    let field = ComponentGrassField {
        benders: vec![GrassBender::default()],
        ..Default::default()
    };
    let saved = serde_json::to_value(&field).unwrap();
    assert!(saved.get("benders").is_none(), "{saved}");
    let loaded: ComponentGrassField = serde_json::from_value(saved).unwrap();
    assert!(loaded.benders.is_empty());
    assert_eq!(loaded.sway, field.sway);
}
