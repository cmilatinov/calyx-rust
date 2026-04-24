use crate as engine;
use crate::component::{AnimationParameters, Component, ReflectComponent};
use crate::component::{ComponentAnimator, ComponentEventContext};
use crate::input::Input;
use crate::net::sync::{SynchronizationOptions, Synchronized};
use crate::reflect::{Reflect, ReflectDefault};
use crate::resource::ResourceMap;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "997d899f-b7ec-48cd-854f-0e3ec07440b6"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Network Animator", update)]
#[serde(default)]
#[repr(C)]
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
                    .extrapolate(false)
                    .component_uuid(ComponentAnimator::type_uuid())
                    .build(),
            ),
        }
    }
}

impl Component for ComponentNetworkAnimator {
    fn update(
        &mut self,
        mut ctx: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        if let Some(AnimationParameters(value)) = self.parameters.update(
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
            let Some(mut entry) = ctx.scene.entry_mut(ctx.game_object) else {
                return Default::default();
            };
            let Ok(c_animator) = entry.get_component_mut::<ComponentAnimator>() else {
                return Default::default();
            };
            c_animator.parameters = value;
        }
    }
}
