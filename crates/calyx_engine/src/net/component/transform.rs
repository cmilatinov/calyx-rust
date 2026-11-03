use crate as calyx_engine;
use crate::component::{
    Component, ComponentEventContext, ComponentRef, ComponentTransform, ComponentUpdate,
    ReflectComponent, ReflectComponentUpdate,
};
use crate::input::Input;
use crate::math::Transform;
use crate::net::sync::{SynchronizationOptions, Synchronized};

use crate::reflect::{Reflect, ReflectDefault};
use crate::resource::ResourceMap;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "9eb02caf-dcf2-4ea4-98bf-5c170230b9a2"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Network Transform")]
#[serde(default)]
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
    fn update(
        this: ComponentRef<Self>,
        mut ctx: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        // The synchronizer needs the whole context while it changes its own
        // state, so it runs on a copy of this component that is written back
        // after. Nothing it calls writes this component through the context,
        // so the write-back overwrites no other write.
        let Some(mut net_transform) = this.read(ctx.scene, |c| c.clone()) else {
            return;
        };
        if let Some(value) = net_transform.transform.update(
            &mut ctx,
            resources,
            |ComponentEventContext {
                 scene, game_object, ..
             }| {
                let Some(entry) = scene.entry(*game_object) else {
                    return Default::default();
                };
                let Some(c_transform) = entry.get_component::<ComponentTransform>().ok() else {
                    return Default::default();
                };
                c_transform.transform
            },
        ) {
            ctx.scene
                .write_component::<ComponentTransform, _>(ctx.game_object, |c_transform| {
                    c_transform.transform = value;
                });
        }
        this.write(ctx.scene, |c| {
            *c = net_transform;
        });
    }
}
