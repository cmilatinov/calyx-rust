use crate::core::TimeType;
use crate::net::NetworkObjectId;
use renet::ClientId;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub enum ServerEvent {
    ClientConnected { client_id: ClientId },
    ClientDisconnected { client_id: ClientId },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum GameMessage {
    // SERVER EVENTS
    ServerEvent(ServerEvent),
    // CLIENT EVENTS
    SelfConnected {
        server_client_id: ClientId,
        client_ids: Vec<ClientId>,
    },
    ClientConnected {
        client_id: ClientId,
    },
    ClientDisconnected {
        client_id: ClientId,
    },
    // SYNCHRONIZATION EVENTS
    SyncTime {
        current_time: TimeType,
    },
    SyncComponent {
        time: TimeType,
        network_object_id: NetworkObjectId,
        from_client_id: ClientId,
        component_uuid: Uuid,
        data: Vec<u8>,
    },
    TransferOwnership {
        network_object_id: NetworkObjectId,
        from_client_id: ClientId,
        to_client_id: ClientId,
    },
}

impl GameMessage {
    pub const PROTOCOL_ID: u64 = 42069;
}

#[derive(Debug)]
pub struct MessageQueue<T> {
    messages: VecDeque<T>,
}

impl Default for MessageQueue<GameMessage> {
    fn default() -> Self {
        Self {
            messages: Default::default(),
        }
    }
}

pub trait MessageHandler<C, M> {
    fn handle_message(&mut self, context: &mut C, message: &M) -> MessageHandlerResult;
}

pub enum MessageHandlerResult {
    Consume,
    Ignore,
}

impl<T> MessageQueue<T> {
    pub fn queue_message(&mut self, message: T) {
        self.messages.push_back(message);
    }

    pub fn receive_messages<C, H: MessageHandler<C, T>>(
        &mut self,
        context: &mut C,
        handler: &mut H,
    ) {
        self.messages.retain(|msg| {
            matches!(
                handler.handle_message(context, msg),
                MessageHandlerResult::Ignore
            )
        });
    }
}
