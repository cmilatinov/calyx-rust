use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use calyx_engine::component::ParticleBlendMode;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::utils::TypeUuid;
use convert_case::{Case, Casing};
use egui::Id;
use egui::Ui;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[uuid = "4f16380d-093c-412c-b73a-f33ea8f4dc4f"]
#[reflect(Default, TypeInspector)]
pub struct ParticleBlendModeInspector;

impl TypeInspector for ParticleBlendModeInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        vec![ParticleBlendMode::type_uuid()]
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        let Some(blend_mode) = instance.downcast_mut::<ParticleBlendMode>() else {
            return InspectorResponse::NONE;
        };
        let label = ctx
            .field_name
            .map(|name| name.from_case(Case::Snake).to_case(Case::Title))
            .unwrap_or_else(|| String::from("Blend Mode"));
        let id = Id::new(ctx.game_object.node).with(ctx.field_name);
        let mut response = InspectorResponse::NONE;
        crate::inspector::widgets::Widgets::inspector_prop_value(ui, label, |ui, _| {
            egui::ComboBox::from_id_salt(id)
                .selected_text(match blend_mode {
                    ParticleBlendMode::Alpha => "Alpha",
                    ParticleBlendMode::Additive => "Additive",
                })
                .show_ui(ui, |ui| {
                    response |= ui.selectable_value(blend_mode, ParticleBlendMode::Alpha, "Alpha");
                    response |=
                        ui.selectable_value(blend_mode, ParticleBlendMode::Additive, "Additive");
                });
        });
        response
    }
}
