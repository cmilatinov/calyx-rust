use crate as calyx_engine;
use crate::reflect::ReflectDefault;
use calyx_engine_derive::{impl_extern_type_uuid, impl_reflect_value};
use egui::Color32;

impl_extern_type_uuid!(Color32, "38318a5b-5df9-410e-ae5e-e07ade73f823");

impl_reflect_value!(Color32(Default));
