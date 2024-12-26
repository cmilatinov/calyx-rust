use egui::{emath, CursorIcon, Id, InnerResponse, LayerId, Order, Pos2, Sense, Ui, Vec2};
use std::any::Any;

pub trait EguiPos2Ext {
    fn orientation(a: Pos2, b: Pos2, c: Pos2) -> f32;
    fn line_intersect(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2) -> Option<Pos2>;
}

impl EguiPos2Ext for Pos2 {
    fn orientation(a: Pos2, b: Pos2, c: Pos2) -> f32 {
        (b - a).cross(c - a)
    }

    fn line_intersect(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2) -> Option<Pos2> {
        let o0 = Self::orientation(p2, p3, p0);
        let o1 = Self::orientation(p2, p3, p1);
        let o2 = Self::orientation(p0, p1, p2);
        let o3 = Self::orientation(p0, p1, p3);
        if o0 * o1 < 0.0 && o2 * o3 < 0.0 {
            let intersect = (p0 * o1 - p1 * o0) / (o1 - o0);
            Some(Pos2::new(intersect.x, intersect.y))
        } else {
            None
        }
    }
}

pub trait EguiVec2Ext {
    fn perp(&self) -> Vec2;
    fn cross(&self, other: Vec2) -> f32;
}

impl EguiVec2Ext for Vec2 {
    fn perp(&self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }

    fn cross(&self, other: Vec2) -> f32 {
        self.x * other.y - self.y * other.x
    }
}

pub trait EguiUiExt {
    fn dnd_drag_source_with_sense<Payload, R>(
        &mut self,
        id: Id,
        payload: Payload,
        sense: Sense,
        add_contents: impl FnOnce(&mut Self) -> R + Clone,
    ) -> InnerResponse<R>
    where
        Payload: Any + Send + Sync;

    fn pointer_in_poly(&self, points: &[Pos2]) -> bool;
}

impl EguiUiExt for Ui {
    fn dnd_drag_source_with_sense<Payload, R>(
        &mut self,
        id: Id,
        payload: Payload,
        sense: Sense,
        add_contents: impl FnOnce(&mut Self) -> R + Clone,
    ) -> InnerResponse<R>
    where
        Payload: Any + Send + Sync,
    {
        let InnerResponse { inner, response } = self.scope(add_contents.clone());

        // Check for drags:
        let dnd_response = self
            .interact(response.rect, id, sense | Sense::drag())
            .on_hover_cursor(CursorIcon::Grab);

        if self.ctx().is_being_dragged(id) {
            egui::DragAndDrop::set_payload(self.ctx(), payload);

            // Paint the body to a new layer:
            let layer_id = LayerId::new(Order::Tooltip, id);
            let InnerResponse { response, .. } = self.with_layer_id(layer_id, add_contents);

            // Now we move the visuals of the body to where the mouse is.
            // Normally you need to decide a location for a widget first,
            // because otherwise that widget cannot interact with the mouse.
            // However, a dragged component cannot be interacted with anyway
            // (anything with `Order::Tooltip` always gets an empty [`Response`])
            // So this is fine!

            if let Some(pointer_pos) = self.ctx().pointer_interact_pos() {
                let delta = pointer_pos - response.rect.center();
                self.ctx()
                    .transform_layer_shapes(layer_id, emath::TSTransform::from_translation(delta));
            }
        }

        InnerResponse::new(inner, dnd_response | response)
    }

    fn pointer_in_poly(&self, points: &[Pos2]) -> bool {
        let Some(pointer_pos) = self.input(|i| i.pointer.latest_pos()) else {
            return false;
        };

        let ui_rect = self.max_rect();
        let start = Pos2::new(ui_rect.min.x, pointer_pos.y);
        let mut num_intersects = 0;
        for (i, point) in points.iter().enumerate() {
            let j = (i + 1) % points.len();
            let next = points[j];
            if Pos2::line_intersect(*point, next, start, pointer_pos).is_some() {
                num_intersects += 1;
            }
        }

        num_intersects % 2 != 0
    }
}
