use egui::{emath, CursorIcon, Id, InnerResponse, LayerId, Order, Sense, Ui};
use std::any::Any;

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
}
