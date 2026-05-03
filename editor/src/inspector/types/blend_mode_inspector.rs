use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use convert_case::{Case, Casing};
use egui::Id;
use egui::Ui;
use engine::component::ParticleBlendMode;
use engine::reflect::{Reflect, ReflectDefault};
use engine::utils::TypeUuid;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[uuid = "4f16380d-093c-412c-b73a-f33ea8f4dc4f"]
#[reflect(Default, TypeInspector)]
#[repr(C)]
pub struct ParticleBlendModeInspector;

impl TypeInspector for ParticleBlendModeInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        vec![ParticleBlendMode::type_uuid()]
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        let Some(blend_mode) = instance.downcast_mut::<ParticleBlendMode>() else {
            return;
        };
        let label = ctx
            .field_name
            .map(|name| name.from_case(Case::Snake).to_case(Case::Title))
            .unwrap_or_else(|| String::from("Blend Mode"));
        let id = Id::new(ctx.game_object.node).with(ctx.field_name);
        crate::inspector::widgets::Widgets::inspector_prop_value(ui, label, |ui, _| {
            egui::ComboBox::from_id_salt(id)
                .selected_text(match blend_mode {
                    ParticleBlendMode::Alpha => "Alpha",
                    ParticleBlendMode::Additive => "Additive",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(blend_mode, ParticleBlendMode::Alpha, "Alpha");
                    ui.selectable_value(blend_mode, ParticleBlendMode::Additive, "Additive");
                });
        });
    }
}
