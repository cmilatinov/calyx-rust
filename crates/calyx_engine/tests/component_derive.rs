//! The component derive must expand without any engine trait in scope.
//! This file deliberately imports no `calyx_engine` traits; it names the derive
//! macros and the one required trait impl by path.

#[derive(
    Default,
    serde::Serialize,
    serde::Deserialize,
    calyx_engine::TypeUuid,
    calyx_engine::Component,
    calyx_engine::Reflect,
)]
#[uuid = "5d0f3a51-2b8e-4c0e-9a57-1f6a0f9e2c41"]
struct ComponentProbe {
    value: f32,
}

impl calyx_engine::component::Component for ComponentProbe {}

#[test]
fn component_derive_expands_without_reflect_in_scope() {
    let probe = ComponentProbe::default();
    let instance: &dyn calyx_engine::component::ComponentInstance = &probe;
    let restored = instance
        .deserialize(&serde_json::json!({ "value": 2.5 }))
        .expect("well-formed probe data deserializes");
    let restored = restored
        .downcast::<ComponentProbe>()
        .unwrap_or_else(|_| panic!("deserialize returns the derived component type"));
    assert_eq!(restored.value, 2.5);

    let Err(err) = instance.deserialize(&serde_json::json!({ "value": "high" })) else {
        panic!("malformed probe data must not deserialize");
    };
    assert!(
        err.to_string()
            .contains("invalid type: string \"high\", expected f32"),
        "unexpected error message: {err}"
    );
}
