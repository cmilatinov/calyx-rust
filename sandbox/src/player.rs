use egui::{Key, Modifiers};
use engine::assets::animation_graph::AnimationParameterValue;
use engine::component::{Component, ComponentAnimator, ComponentEventContext, ReflectComponent};
use engine::input::Input;
use engine::net::{ComponentNetworkObject, GameMessage};
use engine::reflect::{Reflect, ReflectDefault};
use engine::resource::ResourceMap;
use engine::scene::GameObjectRef;
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "8c4d2976-47c1-403c-a248-6db84a124816"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Player Controller", update)]
#[repr(C)]
pub struct ComponentPlayerController {
    pub camera: GameObjectRef,
}

impl Component for ComponentPlayerController {
    fn update(
        &mut self,
        mut ctx: ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        self.update_transfer(&mut ctx, resources, input);
        self.update_animator(&mut ctx, resources, input);
    }
}

impl ComponentPlayerController {
    fn update_transfer(
        &mut self,
        ComponentEventContext {
            scene, game_object, ..
        }: &mut ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        let network = resources.network_mut();
        let transfer = input
            .input_mut(|input| input.consume_key(Modifiers::NONE, Key::T))
            .unwrap_or(false);
        if !transfer
            || network.client.is_disconnected()
            || !scene.is_game_object_owner(*game_object, network)
        {
            return;
        }
        let Some(entry) = scene.entry(*game_object) else {
            return;
        };
        let Ok(player_network_object_id) = entry
            .get_component::<ComponentNetworkObject>()
            .map(|c_netobj| c_netobj.id)
        else {
            return;
        };
        let Some(entry) = self.camera.entry(scene) else {
            return;
        };
        let Ok(camera_network_object_id) = entry
            .get_component::<ComponentNetworkObject>()
            .map(|c_netobj| c_netobj.id)
        else {
            return;
        };
        let Some(from_client_id) = network.client.client_id() else {
            return;
        };
        let Some(to_client_id) = network
            .client
            .client_ids()
            .into_iter()
            .find(|cid| from_client_id != *cid)
        else {
            return;
        };
        println!("CLIENT - Transferring ownership to {}", to_client_id);
        let _ = network
            .client
            .send_message(&GameMessage::TransferOwnership {
                from_client_id,
                to_client_id,
                network_object_id: player_network_object_id,
            });
        let _ = network
            .client
            .send_message(&GameMessage::TransferOwnership {
                from_client_id,
                to_client_id,
                network_object_id: camera_network_object_id,
            });
    }

    fn update_animator(
        &mut self,
        ComponentEventContext {
            scene,
            game_object,
            assets,
            ..
        }: &mut ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        let animate_plus = input
            .input_mut(|input| input.key_down(Key::Equals))
            .unwrap_or(false);
        let animate_minus = input
            .input_mut(|input| input.key_down(Key::Minus))
            .unwrap_or(false);
        if !animate_plus && !animate_minus {
            return;
        }
        if !scene.is_game_object_owner(*game_object, resources.network()) {
            return;
        }
        let Some(mut entry) = scene.entry_mut(*game_object) else {
            return;
        };
        let Ok(c_animator) = entry.get_component_mut::<ComponentAnimator>() else {
            return;
        };
        let delta = 0.01 * ((animate_plus as u32 as f32) - (animate_minus as u32 as f32));
        c_animator.set_parameter_with(assets, "Speed", move |value| {
            let value = match value {
                Some(AnimationParameterValue::Float(value)) => value,
                _ => 0.0,
            };
            AnimationParameterValue::Float((value + delta).clamp(0.0, 1.0))
        });
    }
}
