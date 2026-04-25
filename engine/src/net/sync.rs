use crate::component::ComponentEventContext;
use crate::core::Time;
use crate::net::{
    ComponentNetworkObject, GameMessage, MessageHandler, MessageHandlerResult, Network,
};
use crate::resource::ResourceMap;
use lerp::Lerp;
use renet::DefaultChannel;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use typed_builder::TypedBuilder;
use uuid::Uuid;

#[derive(Clone, TypedBuilder)]
pub struct SynchronizationOptions {
    #[builder(default = true)]
    pub interpolate: bool,
    #[builder(default = false)]
    pub extrapolate: bool,
    pub component_uuid: Uuid,
    #[builder(default = 100)]
    pub max_ticks: u32,
}

#[derive(Clone)]
pub struct Synchronized<T> {
    options: SynchronizationOptions,
    history: BTreeMap<u32, T>,
}

impl<T: Serialize + DeserializeOwned + Clone + Lerp<f32>> Synchronized<T> {
    pub fn new(options: SynchronizationOptions) -> Self {
        Self {
            options,
            history: Default::default(),
        }
    }

    fn prune(&mut self, target_tick: u32) {
        let keep_from = target_tick.saturating_sub(self.options.max_ticks);
        self.history.retain(|&tick, _| tick >= keep_from);
    }

    pub fn value(&self, time: &Time, network_tick_period: f32) -> Option<T> {
        let target_time = (time.time - 3.0 * network_tick_period).max(0.0);
        let render_tick = time.time_to_tick(target_time);
        let before = self.history.range(..=render_tick).next_back();
        let after = self.history.range(render_tick..).next();
        match (before, after) {
            (Some((&tick_a, a)), Some((&tick_b, b))) if tick_a != tick_b => {
                let alpha =
                    ((render_tick - tick_a) as f32 / (tick_b - tick_a) as f32).clamp(0.0, 1.0);
                Some(a.clone().lerp(b.clone(), alpha))
            }
            (Some((_, a)), _) => Some(a.clone()),
            _ => None,
        }
    }

    pub fn update<F: FnOnce(&mut ComponentEventContext) -> T>(
        &mut self,
        ctx: &mut ComponentEventContext,
        resources: &mut ResourceMap,
        getter: F,
    ) -> Option<T> {
        let value = getter(ctx);
        self.update_owner(ctx, resources, &value);
        let new_value = self.update_remote(ctx, resources);
        let Some((network, time)) = resources.resource_pair_mut::<Network, Time>() else {
            return new_value;
        };
        network.queue.receive_messages(&mut (ctx, &*time), self);
        self.prune(time.current_tick());
        new_value
    }

    fn update_owner(
        &mut self,
        ctx: &mut ComponentEventContext,
        resources: &mut ResourceMap,
        value: &T,
    ) {
        let Some((network, time)) = resources.resource_pair_mut::<Network, Time>() else {
            return;
        };
        if !ctx.scene.is_owner(ctx.game_object, network) {
            return;
        }
        let Some(entry) = ctx.scene.entry(ctx.game_object) else {
            return;
        };
        let Some(c_netobj_id) = entry
            .get_component::<ComponentNetworkObject>()
            .ok()
            .map(|c_netobj| c_netobj.id)
        else {
            return;
        };
        let Some(self_client_id) = network.local_id else {
            return;
        };
        let Ok(data) = bincode::serde::encode_to_vec(&value, bincode::config::standard()) else {
            return;
        };
        let message = GameMessage::SyncComponent {
            time: time.time,
            from_client_id: self_client_id,
            network_object_id: c_netobj_id,
            component_uuid: self.options.component_uuid,
            data,
        };
        if let Some(server) = &mut network.server {
            if let Err(e) = server.broadcast_message_except(
                self_client_id,
                DefaultChannel::ReliableOrdered,
                &message,
            ) {
                log::warn!("Failed to broadcast SyncComponent: {e}");
            }
        } else {
            if let Err(e) = network.client.send_message(&message) {
                log::warn!("Failed to send SyncComponent: {e}");
            }
        }
    }

    fn update_remote(
        &mut self,
        ctx: &mut ComponentEventContext,
        resources: &mut ResourceMap,
    ) -> Option<T> {
        let Some((network, time)) = resources.resource_pair_mut::<Network, Time>() else {
            return None;
        };
        if ctx.scene.is_owner(ctx.game_object, network) {
            return None;
        }
        self.value(time, network.tick_period())
    }
}

impl<T: Serialize + DeserializeOwned + Clone + Lerp<f32>>
    MessageHandler<(&mut ComponentEventContext<'_>, &Time), GameMessage> for Synchronized<T>
{
    fn handle_message(
        &mut self,
        (
            ComponentEventContext {
                scene, game_object, ..
            },
            time,
        ): &mut (&mut ComponentEventContext<'_>, &Time),
        message: &GameMessage,
    ) -> MessageHandlerResult {
        let Some(entry) = scene.entry(*game_object) else {
            return MessageHandlerResult::Ignore;
        };
        let Some((c_netobj_id, c_netobj_owner_id)) = entry
            .get_component::<ComponentNetworkObject>()
            .ok()
            .map(|c_netobj| (c_netobj.id, c_netobj.owner_id))
        else {
            return MessageHandlerResult::Ignore;
        };
        match message {
            GameMessage::SyncComponent {
                time: sync_time,
                from_client_id,
                network_object_id,
                component_uuid,
                data,
            } if c_netobj_id == *network_object_id
                && c_netobj_owner_id == *from_client_id
                && *component_uuid == self.options.component_uuid =>
            {
                if let Ok((value, _)) =
                    bincode::serde::decode_from_slice(&data, bincode::config::standard())
                {
                    self.history.insert(time.time_to_tick(*sync_time), value);
                }
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}
