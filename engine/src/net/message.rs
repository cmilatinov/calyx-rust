use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum GameMessage {
    SetOwner { network_id: u64 },
}

impl GameMessage {
    pub const PROTOCOL_ID: u64 = 42069;
}
