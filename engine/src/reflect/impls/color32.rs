use crate as engine;
use crate::reflect::ReflectDefault;
use egui::Color32;
use engine_derive::{impl_extern_type_uuid, impl_reflect_value};

impl_extern_type_uuid!(Color32, "38318a5b-5df9-410e-ae5e-e07ade73f823");

impl_reflect_value!(Color32(Default));
