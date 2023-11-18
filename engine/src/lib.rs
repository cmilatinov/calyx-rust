#![allow(clippy::module_inception)]

pub extern crate eframe;
pub extern crate egui;
pub extern crate egui_dock;
pub extern crate egui_extras;
pub extern crate egui_wgpu;
pub extern crate image;
pub extern crate indextree;
pub extern crate inventory;
pub extern crate legion;
pub extern crate log;
pub extern crate nalgebra_glm as glm;
pub extern crate relative_path;
pub extern crate russimp;
pub extern crate rusty_pool;
pub extern crate serde;
pub extern crate utils;
pub extern crate uuid;

pub mod assets;
pub mod background;
pub mod class_registry;
pub mod component;
pub mod core;
pub mod math;
pub mod render;
pub mod scene;
