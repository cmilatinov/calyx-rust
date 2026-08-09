//! Remote control layer that lets external tools (notably `calyx_ctl` and AI
//! coding agents) drive the editor over localhost TCP.
//!
//! Enabled only when the `CALYX_REMOTE_PORT` environment variable is set; the
//! editor behaves identically otherwise. See `.steering/agent-loop.md` for the
//! full protocol and usage guide.

mod handlers;
mod runtime;
mod server;

pub use runtime::RemoteRuntime;
pub use server::{remote_config_from_env, RemoteServer};
