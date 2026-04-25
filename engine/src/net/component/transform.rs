use crate as engine;
use crate::component::{
    Component, ComponentEventContext, ComponentTransform, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use crate::input::Input;
use crate::math::Transform;
use crate::net::sync::{SynchronizationOptions, Synchronized};

use crate::reflect::{Reflect, ReflectDefault};
use crate::resource::ResourceMap;
use crate::try_all;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "9eb02caf-dcf2-4ea4-98bf-5c170230b9a2"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Network Transform")]
#[serde(default)]
#[repr(C)]
pub struct ComponentNetworkTransform {
    #[reflect_skip]
    #[serde(skip)]
    transform: Synchronized<Transform>,
}

impl Default for ComponentNetworkTransform {
    fn default() -> Self {
        Self {
            transform: Synchronized::new(
                SynchronizationOptions::builder()
                    .interpolate(true)
                    .component_uuid(ComponentTransform::type_uuid())
                    .build(),
            ),
        }
    }
}

impl Component for ComponentNetworkTransform {}

impl ComponentUpdate for ComponentNetworkTransform {
    fn update(&self, mut ctx: ComponentEventContext, resources: &mut ResourceMap, _input: &Input) {
        let Some(mut net_transform) = ctx
            .scene
            .read_component::<ComponentNetworkTransform, _, _>(ctx.game_object, |c| c.clone())
        else {
            return;
        };
        if let Some(value) = net_transform.transform.update(
            &mut ctx,
            resources,
            |ComponentEventContext {
                 scene, game_object, ..
             }| {
                try_all!(
                    None => return Default::default();
                    let entry = scene.entry(*game_object);
                    let c_transform = entry.get_component::<ComponentTransform>().ok();
                );
                c_transform.transform
            },
        ) {
            try_all!(
                None => return Default::default();
                let mut entry = ctx.scene.entry_mut(ctx.game_object);
                let c_transform = entry.get_component_mut::<ComponentTransform>().ok();
            );
            c_transform.transform = value;
        }
        ctx.scene
            .write_component::<ComponentNetworkTransform, _>(ctx.game_object, |c| {
                *c = net_transform;
            });
    }
}
