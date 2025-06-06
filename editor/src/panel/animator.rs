use crate::inspector::assets::animation_graph_inspector::AnimationGraphInspector;
use crate::panel::Panel;
use crate::selection::{Selection, SelectionType};
use crate::{icons, EditorAppState};
use egui::panel::Side;
use egui::text::LayoutJob;
use egui::{
    Color32, FontId, LayerId, Order, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui,
    UiBuilder, Vec2,
};
use engine::assets::animation_graph::{
    AnimationGraph, AnimationMotion, AnimationNode, AnimationTransition,
};
use engine::component::ComponentAnimator;
use engine::core::Ref;
use engine::ext::egui::{EguiUiExt, EguiVec2Ext};
use petgraph::prelude::{EdgeIndex, NodeIndex};
use re_ui::{DesignTokens, Icon};
use std::any::Any;
use std::collections::HashMap;
use std::ops::DerefMut;
use uuid::Uuid;

pub struct PanelAnimator {
    rect: Rect,
    selected_graph: Option<Ref<AnimationGraph>>,
    latest_pos: Pos2,
    source_node: Option<NodeIndex>,
}

impl Default for PanelAnimator {
    fn default() -> Self {
        Self {
            rect: Rect::from_center_size(Pos2::new(0.0, 0.0), Vec2::splat(1000.0)),
            selected_graph: None,
            latest_pos: Default::default(),
            source_node: None,
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

    fn ui(&mut self, ui: &mut Ui, state: &mut EditorAppState) {
        let ty = state.selection.ty();
        let mut selected_id = state.selection.first(ty);
        let mut selected_asset_id = None;
        let mut game_object = None;
        if let SelectionType::AnimationNode(id) = ty {
            selected_asset_id = Some(id);
        } else if let SelectionType::AnimationTransition(id) = ty {
            selected_asset_id = Some(id);
        } else if let SelectionType::Asset = ty {
            selected_asset_id = selected_id;
            selected_id = None;
        } else if let SelectionType::GameObject = ty {
            game_object = selected_id.and_then(|id| {
                state
                    .game
                    .scenes
                    .simulation_scene()
                    .get_game_object_by_uuid(id)
            });
        }
        if let Some(id) = selected_asset_id {
            if let Ok(graph) = state
                .game
                .assets
                .asset_registry
                .read()
                .load_by_id::<AnimationGraph>(id)
            {
                self.selected_graph = Some(graph);
            }
        }
        let Some(graph) = self.selected_graph.clone() else {
            return;
        };
        let asset_id = graph.id();
        let mut graph = graph.write();
        egui::SidePanel::new(Side::Left, "animator_side_panel")
            .resizable(true)
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                egui::ScrollArea::both().show(ui, |ui| {
                    egui::Frame {
                        fill: ui.style().visuals.panel_fill,
                        inner_margin: DesignTokens::panel_margin(),
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        re_ui::list_item::list_item_scope(ui, "animator_tree_scope", |ui| {
                            let scene = state.game.scenes.simulation_scene_mut();
                            let mut entry = game_object.and_then(|go| scene.entry_mut(go));
                            let parameter_values = entry
                                .as_mut()
                                .and_then(|entry| {
                                    entry.get_component_mut::<ComponentAnimator>().ok()
                                })
                                .map(|animator| animator.parameters_mut());
                            AnimationGraphInspector::parameters(
                                ui,
                                &mut graph.parameters,
                                parameter_values,
                            );
                        });
                        ui.allocate_space(ui.available_size());
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                let mut rect = self.rect;
                let resp = egui::Scene::new()
                    .show(ui, &mut rect, |ui| {
                        self.draw_graph(
                            ui,
                            &mut state.selection,
                            &mut graph,
                            asset_id,
                            selected_id,
                        );
                    })
                    .response;
                if resp.secondary_clicked() {
                    if let Some(pos) = ui.input(|input| input.pointer.interact_pos()) {
                        let transform = ui
                            .ctx()
                            .layer_transform_from_global(LayerId::new(
                                ui.layer_id().order,
                                ui.id().with("scene_area"),
                            ))
                            .unwrap_or_default();
                        self.latest_pos = transform * pos;
                    }
                }
                resp.context_menu(|ui| {
                    ui.set_max_width(200.0);
                    ui.menu_button("Create state", |ui| {
                        if ui.button("Animation").clicked() {
                            graph.add_node(AnimationNode {
                                id: Uuid::new_v4(),
                                name: "Animation".to_string(),
                                motion: AnimationMotion::AnimationClip(Default::default()),
                                position: self.latest_pos,
                            });
                            ui.close_menu();
                        }
                        if ui.button("Blend Tree").clicked() {
                            graph.add_node(AnimationNode {
                                id: Uuid::new_v4(),
                                name: "Blend Tree".to_string(),
                                motion: AnimationMotion::BlendTree1D(Default::default()),
                                position: self.latest_pos,
                            });
                            ui.close_menu();
                        }
                    });
                });
                self.rect = rect;
            });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl PanelAnimator {
    fn stroke() -> Stroke {
        Stroke::new(1.5, Color32::GRAY)
    }

    fn hovered_stroke() -> Stroke {
        Stroke::new(3.0, Color32::WHITE)
    }

    fn selected_stroke(ui: &mut Ui) -> Stroke {
        Stroke::new(3.0, ui.visuals().selection.bg_fill)
    }

    fn draw_graph(
        &mut self,
        ui: &mut Ui,
        selection: &mut Selection,
        graph: &mut AnimationGraph,
        asset_id: Uuid,
        selected_id: Option<Uuid>,
    ) {
        let nodes = graph.node_indices().collect::<Vec<_>>();
        let node_layer_id = LayerId::new(Order::Middle, ui.layer_id().id.with("nodes"));
        let mut node_ui = ui.new_child(
            UiBuilder::new()
                .layer_id(node_layer_id)
                .sense(Sense::hover())
                .max_rect(ui.max_rect()),
        );
        ui.ctx().set_transform_layer(
            node_layer_id,
            ui.ctx()
                .layer_transform_to_global(ui.layer_id())
                .unwrap_or_default(),
        );
        let mut any_node_hovered = false;
        for node in nodes {
            any_node_hovered |= self
                .draw_node(&mut node_ui, selection, graph, node, asset_id, selected_id)
                .hovered();
        }
        let mut edge_map: HashMap<[NodeIndex; 2], Vec<EdgeIndex>> = Default::default();
        for edge in graph.edge_indices() {
            if let Some((source, target)) = graph.edge_endpoints(edge) {
                let mut key = [source, target];
                key.sort();
                edge_map.entry(key).or_default().push(edge);
            }
        }
        let edge_layer_id = LayerId::new(Order::Background, ui.layer_id().id.with("edges"));
        let mut edge_ui = ui.new_child(
            UiBuilder::new()
                .layer_id(edge_layer_id)
                .sense(Sense::hover())
                .max_rect(ui.max_rect()),
        );
        ui.ctx().set_transform_layer(
            edge_layer_id,
            ui.ctx()
                .layer_transform_to_global(ui.layer_id())
                .unwrap_or_default(),
        );
        for ([source, target], edges) in edge_map {
            self.draw_edges(
                &mut edge_ui,
                selection,
                graph,
                source,
                target,
                edges.as_slice(),
                asset_id,
                selected_id,
                any_node_hovered,
            );
        }
        self.draw_new_transition(&mut edge_ui, graph);
    }

    fn draw_node(
        &mut self,
        ui: &mut Ui,
        selection: &mut Selection,
        graph: &mut AnimationGraph,
        node: NodeIndex,
        asset_id: Uuid,
        selected_id: Option<Uuid>,
    ) -> Response {
        let visuals = &ui.style().visuals;
        let selected = if let Some(id) = selected_id {
            graph[node].id == id
        } else {
            false
        };
        let job = LayoutJob::simple_singleline(
            graph[node].name.clone(),
            FontId::proportional(24.0),
            visuals.strong_text_color(),
        );
        let galley = ui.fonts(|fonts| fonts.layout_job(job));

        let graph = graph.deref_mut();

        let padding = 15.0;
        let corner_radius = 5.0;
        let stroke = if selected {
            Stroke::new(1.0, visuals.strong_text_color())
        } else {
            Stroke::new(1.0, visuals.weak_text_color())
        };
        let bg = if selected {
            visuals.selection.bg_fill
        } else {
            visuals.widgets.noninteractive.bg_fill
        };

        let size = galley.rect.expand(padding + stroke.width).size();
        let max_rect = Rect::from_center_size(graph[node].position, size);
        let builder = UiBuilder::new()
            .max_rect(max_rect)
            .sense(Sense::click_and_drag());
        let mut node_ui = ui.new_child(builder);

        egui::Frame::default()
            .inner_margin(padding)
            .stroke(stroke)
            .fill(bg)
            .corner_radius(corner_radius)
            .show(&mut node_ui, |ui| {
                ui.add(egui::Label::new(galley).selectable(false));
            });

        let res = node_ui.response();
        res.context_menu(|ui| {
            if ui.button("Make Transition").clicked() {
                self.source_node = Some(node);
                ui.close_menu();
            }
            if ui.button("Remove").clicked() {
                graph.remove_node(node);
                ui.close_menu();
            }
        });
        if res.dragged() {
            if let Some(node) = graph.node_weight_mut(node) {
                node.position += res.drag_delta();
            }
        }
        if res.clicked() {
            if let Some(source) = self.source_node {
                if source != node {
                    graph.add_edge(
                        source,
                        node,
                        AnimationTransition {
                            id: Uuid::new_v4(),
                            name: "Transition".to_string(),
                            duration: 0.5,
                            has_exit_time: false,
                            exit_time: 0.0,
                            conditions: vec![],
                        },
                    );
                    self.source_node = None;
                }
            } else {
                *selection = if selected {
                    Selection::from_id(SelectionType::Asset, asset_id)
                } else {
                    Selection::from_id(SelectionType::AnimationNode(asset_id), graph[node].id)
                };
            }
        }
        res
    }

    fn draw_edges(
        &mut self,
        ui: &mut Ui,
        editor_selection: &mut Selection,
        graph: &mut AnimationGraph,
        source: NodeIndex,
        target: NodeIndex,
        edges: &[EdgeIndex],
        asset_id: Uuid,
        selected_id: Option<Uuid>,
        any_node_hovered: bool,
    ) {
        let edge_directions = edges
            .iter()
            .map(|ei| {
                if let Some((start, _)) = graph.edge_endpoints(*ei) {
                    start == source
                } else {
                    true
                }
            })
            .collect::<Vec<_>>();
        let endpoints = [graph[source].position, graph[target].position];
        let response = ui.allocate_rect(
            Rect::from_two_pos(endpoints[0], endpoints[1]).expand(30.0),
            Sense::click(),
        );
        let spacing = 16.0;
        let hitbox_size = spacing / 2.0;
        let dir = (endpoints[1] - endpoints[0]).normalized();
        let perp_dir = dir.perp().normalized();
        let mut start_offset = -(edges.len() as f32 - 1.0) / 2.0;
        let end_offset = start_offset.abs().ceil();
        let mut idx = 0;
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
            let selected = if let Some(id) = selected_id {
                graph[edges[idx]].id == id
            } else {
                false
            };
            let hovered = !any_node_hovered
                && ui.rect_contains_pointer2(response.interact_rect)
                && ui.poly_contains_pointer(poly);
            let stroke = if selected {
                Self::selected_stroke(ui)
            } else if hovered {
                Self::hovered_stroke()
            } else {
                Self::stroke()
            };
            if hovered && ui.input(|input| input.pointer.primary_released()) {
                let selection = if selected {
                    Selection::from_id(SelectionType::Asset, asset_id)
                } else {
                    Selection::from_id(
                        SelectionType::AnimationTransition(asset_id),
                        graph[edges[idx]].id,
                    )
                };
                *editor_selection = selection;
            }
            ui.painter().line_segment([p0, p1], stroke);
            Self::draw_arrow(
                ui.painter(),
                Pos2::new((p0.x + p1.x) / 2.0, (p0.y + p1.y) / 2.0),
                if edge_directions[idx] { dir } else { -dir },
                if selected || hovered { 14.0 } else { 8.0 },
                stroke.color,
            );
            start_offset += 1.0;
            idx += 1;
        }
    }

    fn draw_new_transition(&mut self, ui: &mut Ui, graph: &mut AnimationGraph) {
        let Some(mut cursor_pos) = ui.input(|input| input.pointer.latest_pos()) else {
            return;
        };
        if let Some(transform) = ui.ctx().layer_transform_from_global(ui.layer_id()) {
            cursor_pos = transform * cursor_pos;
        }
        let Some(source_node) = self.source_node else {
            return;
        };
        let source_pos = graph[source_node].position;
        let stroke = Self::hovered_stroke();
        ui.painter().line_segment([source_pos, cursor_pos], stroke);
        Self::draw_arrow(
            ui.painter(),
            (source_pos + Vec2::new(cursor_pos.x, cursor_pos.y)) / 2.0,
            (cursor_pos - source_pos).normalized(),
            14.0,
            stroke.color,
        );
    }

    fn draw_arrow(painter: &Painter, tip: Pos2, direction: Vec2, size: f32, color: Color32) {
        let perpendicular = Vec2::new(-direction.y, direction.x) * 0.5 * size;

        let p1 = tip - direction * size + perpendicular;
        let p2 = tip - direction * size - perpendicular;

        // Draw a filled triangle for the arrow
        painter.add(Shape::convex_polygon(
            vec![tip, p1, p2],
            color,
            Stroke::NONE,
        ));
    }
}
