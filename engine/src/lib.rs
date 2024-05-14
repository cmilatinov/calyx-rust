#![allow(clippy::module_inception)]

pub extern crate eframe;
pub extern crate egui;
pub extern crate egui_dock;
pub extern crate egui_extras;
pub extern crate egui_wgpu;
pub extern crate image;
pub extern crate inventory;
pub extern crate legion;
pub extern crate log;
pub extern crate mint;
pub extern crate nalgebra;
pub extern crate nalgebra_glm as glm;
pub extern crate petgraph;
pub extern crate rapier3d;
pub extern crate relative_path;
pub extern crate russimp;
pub extern crate rusty_pool;
pub extern crate serde;
pub extern crate serde_json;
pub extern crate transform_gizmo_egui;
pub extern crate uuid;

pub mod assets;
pub mod background;
pub mod class_registry;
pub mod component;
pub mod core;
pub mod math;
pub mod physics;
pub mod reflect;
pub mod render;
pub mod scene;
pub mod utils;

pub use engine_derive::*;
use inventory::collect;
use reflect::type_registry::TypeRegistry;

pub struct ReflectRegistrationFn(pub fn(&mut TypeRegistry));
collect!(ReflectRegistrationFn);
