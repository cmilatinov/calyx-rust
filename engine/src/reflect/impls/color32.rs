use crate as engine;
use crate::reflect::ReflectDefault;
use egui::Color32;
use engine_derive::impl_reflect_value;

impl_reflect_value!(Color32(Default));
