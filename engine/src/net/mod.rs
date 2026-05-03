//! Multiplayer and replication APIs.
//!
//! The net module exposes the high-level client, server, message, and network
//! ownership types used to drive the engine's Renet-based multiplayer layer.

mod client;
mod component;
mod message;
mod network;
mod server;
mod sync;

pub use client::*;
pub use component::*;
pub use message::*;
pub use network::*;
pub use server::*;

#[cfg(test)]
mod tests;
