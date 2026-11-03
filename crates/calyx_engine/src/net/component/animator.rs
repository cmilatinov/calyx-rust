use crate as calyx_engine;
use crate::component::{
    AnimationParameters, Component, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
};
use crate::component::{ComponentAnimator, ComponentEventContext, ComponentRef};
use crate::input::Input;
use crate::net::sync::{SynchronizationOptions, Synchronized};
use crate::reflect::{Reflect, ReflectDefault};
use crate::resource::ResourceMap;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "997d899f-b7ec-48cd-854f-0e3ec07440b6"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Network Animator")]
#[serde(default)]
pub struct ComponentNetworkAnimator {
    #[serde(skip)]
    #[reflect_skip]
    parameters: Synchronized<AnimationParameters>,
}

impl Default for ComponentNetworkAnimator {
    fn default() -> Self {
        Self {
            parameters: Synchronized::new(
                SynchronizationOptions::builder()
                    .interpolate(true)
                    .component_uuid(ComponentAnimator::type_uuid())
                    .build(),
            ),
        }
    }
}

impl Component for ComponentNetworkAnimator {}

impl ComponentUpdate for ComponentNetworkAnimator {
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
        let Some(mut net_animator) = this.read(ctx.scene, |c| c.clone()) else {
            return;
        };
        if let Some(AnimationParameters(value)) = net_animator.parameters.update(
            &mut ctx,
            resources,
            |ComponentEventContext {
                 scene, game_object, ..
             }| {
                let Some(entry) = scene.entry(*game_object) else {
                    return Default::default();
                };
                let Ok(c_animator) = entry.get_component::<ComponentAnimator>() else {
                    return Default::default();
                };
                AnimationParameters(c_animator.parameters.clone())
            },
        ) {
            ctx.scene
                .write_component::<ComponentAnimator, _>(ctx.game_object, |c_animator| {
                    c_animator.parameters = value;
                });
        }
        this.write(ctx.scene, |c| {
            *c = net_animator;
        });
    }
}
