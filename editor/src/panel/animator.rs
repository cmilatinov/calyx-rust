use crate::icons;
use crate::panel::Panel;
use engine::egui;
use engine::egui::emath::TSTransform;
use engine::egui::{
    Color32, CursorIcon, LayerId, Margin, Order, Rect, Sense, Stroke, TextWrapMode, Ui,
};
use engine::ext::egui::{EguiUiExt, EguiVec2Ext};
use engine::petgraph::prelude::{EdgeIndex, NodeIndex};
use engine::petgraph::Graph;
use re_ui::Icon;
use std::any::Any;
use std::collections::HashMap;

pub struct PanelAnimator {
    zoom_factor: f32,
    transform: TSTransform,
    graph: Graph<String, ()>,
}

impl Default for PanelAnimator {
    fn default() -> Self {
        let mut graph: Graph<String, ()> = Default::default();
        let a = graph.add_node(String::from("First"));
        let b = graph.add_node(String::from("Second"));
        let c = graph.add_node(String::from("Third"));
        graph.add_edge(a, b, ());
        graph.add_edge(b, a, ());
        graph.add_edge(b, c, ());
        graph.add_edge(c, b, ());
        graph.add_edge(c, b, ());
        graph.add_edge(c, a, ());
        Self {
            zoom_factor: 1.0,
            transform: Default::default(),
            graph,
        }
    }
}

impl Panel for PanelAnimator {
    fn name() -> &'static str
    where
        Self: Sized,
    {
        "Animator"
    }

    fn icon(&self) -> Option<&'static Icon> {
        Some(&icons::WALKING)
    }

    fn ui(&mut self, ui: &mut Ui) {
        let (id, rect) = ui.allocate_space(ui.available_size());
        let response = ui.interact(rect, id, Sense::click_and_drag());
        if response.dragged() {
            self.transform.translation += response.drag_delta();
        }

        if response.double_clicked() {
            self.transform = Default::default();
            self.zoom_factor = 1.0;
        }

        let transform =
            TSTransform::from_translation(ui.min_rect().left_top().to_vec2()) * self.transform;
        if let Some(pointer) = ui.ctx().input(|i| i.pointer.hover_pos()) {
            // Note: doesn't catch zooming / panning if a button in this PanZoom container is hovered.
            if ui.rect_contains_pointer(response.rect) {
                let pointer_in_layer = transform.inverse() * pointer;
                let zoom_delta = ui.ctx().input(|i| 1.0 + i.smooth_scroll_delta.y * 0.001);
                let zoom_factor = self.zoom_factor;
                self.zoom_factor *= zoom_delta;
                self.zoom_factor = self.zoom_factor.clamp(0.25, 2.0);
                let real_zoom_delta = self.zoom_factor / zoom_factor;

                // Zoom in on pointer:
                self.transform = self.transform
                    * TSTransform::from_translation(pointer_in_layer.to_vec2())
                    * TSTransform::from_scaling(real_zoom_delta)
                    * TSTransform::from_translation(-pointer_in_layer.to_vec2());
            }
        }

        for (i, node) in self.graph.raw_nodes().iter().enumerate() {
            let id = id.with(("node", i));
            let window_layer = ui.layer_id();
            let res = egui::Area::new(id)
                .default_pos(egui::Pos2 {
                    x: 0.0,
                    y: i as f32 * 50.0,
                })
                .sense(Sense::click_and_drag())
                .order(Order::Middle)
                .show(ui.ctx(), |ui| {
                    ui.set_clip_rect(transform.inverse() * rect);
                    egui::Frame::default()
                        .inner_margin(Margin::same(10.0))
                        .stroke(Stroke::new(1.0, Color32::DARK_GRAY))
                        .fill(ui.style().visuals.window_fill)
                        .show(ui, |ui| {
                            ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                            ui.style_mut().visuals.override_text_color = Some(Color32::WHITE);
                            ui.add(
                                egui::Label::new(node.weight.as_str())
                                    .selectable(false)
                                    .sense(Sense::hover()),
                            );
                        });
                })
                .response
                .on_hover_cursor(CursorIcon::Move);
            if res.clicked() || res.dragged() {
                res.request_focus();
            }
            if res.has_focus() {
                ui.ctx()
                    .layer_painter(LayerId::new(Order::Foreground, id.with("top")))
                    .rect_stroke(transform * res.rect, 0.0, Stroke::new(1.0, Color32::WHITE));
            }
            ui.ctx().set_transform_layer(res.layer_id, transform);
            ui.ctx().set_sublayer(window_layer, res.layer_id);
        }

        let mut edge_map: HashMap<[NodeIndex; 2], Vec<EdgeIndex>> = Default::default();

        for (index, edge) in self.graph.raw_edges().iter().enumerate() {
            let mut key = [edge.source(), edge.target()];
            key.sort();
            edge_map.entry(key).or_default().push(EdgeIndex::new(index));
        }

        for ([source, target], edges) in edge_map {
            let Some(src_state) =
                egui::AreaState::load(ui.ctx(), id.with(("node", source.index())))
            else {
                continue;
            };
            let Some(dst_state) =
                egui::AreaState::load(ui.ctx(), id.with(("node", target.index())))
            else {
                continue;
            };
            let Some(src_pos) = src_state.pivot_pos else {
                continue;
            };
            let Some(dst_pos) = dst_state.pivot_pos else {
                continue;
            };
            let Some(src_size) = src_state.size else {
                continue;
            };
            let Some(dst_size) = dst_state.size else {
                continue;
            };

            let endpoints = [
                transform * src_pos + (transform.scaling * src_size / 2.0),
                transform * dst_pos + (transform.scaling * dst_size / 2.0),
            ];
            let response = ui.allocate_rect(
                Rect::from_two_pos(endpoints[0], endpoints[1]),
                Sense::hover(),
            );
            let stroke = Stroke::new(2.0, Color32::LIGHT_GRAY);
            let hovered_stroke = Stroke::new(5.0, Color32::LIGHT_GRAY);
            let spacing = 15.0 * self.zoom_factor;
            let hitbox_size = spacing / 2.0;
            let dir = (endpoints[1] - endpoints[0]).normalized();
            let perp_dir = dir.perp().normalized();
            let mut start_offset = -(edges.len() as f32 - 1.0) / 2.0;
            let end_offset = start_offset.abs().ceil();
            let interact_rect = response.rect.expand(30.0);
            while start_offset <= end_offset {
                let offset = perp_dir * spacing * start_offset;
                let p0 = endpoints[0] + offset;
                let p1 = endpoints[1] + offset;
                let poly = &[
                    p0 - perp_dir * hitbox_size,
                    p0 + perp_dir * hitbox_size,
                    p1 + perp_dir * hitbox_size,
                    p1 - perp_dir * hitbox_size,
                ];
                let stroke = if ui.rect_contains_pointer(interact_rect) && ui.pointer_in_poly(poly)
                {
                    hovered_stroke
                } else {
                    stroke
                };
                ui.painter().line_segment([p0, p1], stroke);
                start_offset += 1.0;
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
