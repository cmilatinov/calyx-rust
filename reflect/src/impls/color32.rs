use crate as reflect;
use crate::ReflectDefault;
use egui::Color32;
use reflect_derive::{impl_extern_type_uuid, impl_reflect_value};

impl_extern_type_uuid!(Color32, "52d47ce5-d2c4-4fd8-88b2-ee8c00932b6e");
impl_reflect_value!(Color32(Default));
