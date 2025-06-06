use egui::Ui;
use engine::context::ReadOnlyAssetContext;
use engine::reflect::{Reflect, StructInfo};
use engine::reflect_trait;
use engine::scene::{GameObject, Scene};
use uuid::Uuid;

#[reflect_trait]
pub trait TypeInspector: Send + Sync {
    fn target_type_uuids(&self) -> Vec<Uuid>;
    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect);
    fn show_inspector_context(
        &self,
        _ui: &mut Ui,
        _ctx: &InspectorContext,
        _instance: &mut dyn Reflect,
    ) {
    }
}

#[derive(Copy, Clone)]
pub struct InspectorContext<'a> {
    pub assets: &'a ReadOnlyAssetContext,
    pub scene: &'a Scene,
    pub game_object: GameObject,
    pub parent: Option<GameObject>,
    pub type_info: &'a StructInfo,
    pub field_name: Option<&'static str>,
}
