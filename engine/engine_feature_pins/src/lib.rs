//! Feature pins for engine's dependency tree; see this crate's `Cargo.toml`.
//! The crate holds no code beyond naming its pins.

// Each pin is used only for the features it enables. Naming it here keeps
// Cargo's `unused_dependencies` lint from asking for its removal.

use ecolor as _;
use egui as _;
use epaint as _;
use half as _;
use lock_api as _;
use num_rational as _;
use parking_lot as _;
use proc_macro2 as _;
use quote as _;
use rgb as _;
use syn as _;
use url as _;
use uuid as _;
use zeroize as _;

#[cfg(windows)]
use bytemuck as _;
#[cfg(windows)]
use futures_core as _;
#[cfg(windows)]
use futures_sink as _;
#[cfg(windows)]
use getrandom as _;
#[cfg(windows)]
use percent_encoding as _;
#[cfg(windows)]
use rand_core as _;
#[cfg(windows)]
use smallvec as _;
#[cfg(windows)]
use tracing as _;
#[cfg(windows)]
use winapi as _;
#[cfg(windows)]
use windows_sys_0_52 as _;
#[cfg(windows)]
use windows_sys_0_59 as _;
#[cfg(windows)]
use windows_sys_0_61 as _;

#[cfg(target_os = "linux")]
use mio as _;
#[cfg(target_os = "linux")]
use rand as _;
#[cfg(target_os = "linux")]
use rustix as _;
#[cfg(target_os = "linux")]
use strict_num as _;
#[cfg(target_os = "linux")]
use tiny_skia as _;
#[cfg(target_os = "linux")]
use tiny_skia_path as _;
#[cfg(target_os = "linux")]
use winnow as _;
