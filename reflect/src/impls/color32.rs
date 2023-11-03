use crate as reflect;
use crate::ReflectDefault;
use egui::Color32;
use reflect_derive::impl_reflect_value;

impl_reflect_value!(Color32(Default));
