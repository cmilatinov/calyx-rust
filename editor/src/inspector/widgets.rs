use engine::egui::{DragValue, Ui};
use engine::glm::Vec3;

pub struct Widgets;

const DRAG_SIZE: f32 = 56.0;

impl Widgets {
    pub fn drag_float3(ui: &mut Ui, speed: f32, value: &mut Vec3) -> bool {
        let mut changed = false;
        changed |= ui.add_sized([DRAG_SIZE, ui.available_height()], DragValue::new(&mut value.x).speed(speed)).changed();
        changed |= ui.add_sized([DRAG_SIZE, ui.available_height()], DragValue::new(&mut value.y).speed(speed)).changed();
        changed |= ui.add_sized([DRAG_SIZE, ui.available_height()], DragValue::new(&mut value.z).speed(speed)).changed();
        changed
    }

    pub fn drag_angle(ui: &mut Ui, value: &mut f32) -> bool {
        let mut degrees = value.to_degrees();
        let res = ui.add_sized(
            [DRAG_SIZE, ui.available_height()],
            DragValue::new(&mut degrees)
                .speed(1.0)
                .suffix("°")
        );
        let changed = res.changed();
        if changed {
            *value = degrees.to_radians();
        }
        changed
    }

    pub fn drag_angle3(ui: &mut Ui, value: &mut Vec3) -> bool {
        let mut changed = false;
        changed |= Self::drag_angle(ui, &mut value.x);
        changed |= Self::drag_angle(ui, &mut value.y);
        changed |= Self::drag_angle(ui, &mut value.z);
        changed
    }
}