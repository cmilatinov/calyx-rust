use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};
use crate::inspector::widgets::Widgets;
use crate::widgets::{List, ListButtons, ListItemContext};
use egui::{ComboBox, DragValue, Id, Label, TextEdit, Ui, UiBuilder};
use engine::assets::animation::Animation;
use engine::assets::animation_graph::{
    AnimationClip, AnimationCondition, AnimationGraph, AnimationMotion, AnimationNode,
    AnimationParameter, AnimationParameterValue, AnimationTransition, BlendTree, BoolCondition,
    FloatCondition, IntCondition,
};
use engine::assets::AssetRegistry;
use engine::context::GameContext;
use engine::reflect::ReflectDefault;
use engine::utils::TypeUuid;
use engine::Reflect;
use petgraph::prelude::StableGraph;
use re_ui::list_item::{
    CustomContent, LabelContent, ListItem, PropertyContent, ShowCollapsingResponse,
};
use re_ui::UiExt;
use std::collections::HashMap;
// use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
pub struct AnimationGraphInspector;

impl AssetInspector for AnimationGraphInspector {
    fn target_type_uuid(&self) -> Uuid {
        AnimationGraph::type_uuid()
    }

    fn show_inspector(&self, ui: &mut Ui, game: &mut GameContext, asset_id: Uuid) {
        let Ok(asset) = game.assets.asset_registry.read().load_dyn_by_id(asset_id) else {
            return;
        };
        let Some(graph_ref) = asset.try_downcast::<AnimationGraph>() else {
            return;
        };
        let mut graph = graph_ref.write();
        let AnimationGraph {
            graph,
            parameters,
            start_node,
        } = &mut *graph;
        Self::start_node_select(ui, graph, start_node);
        Self::parameters(ui, parameters, None);
    }
}

impl AnimationGraphInspector {
    pub fn parameters(
        ui: &mut Ui,
        parameters: &mut Vec<AnimationParameter>,
        mut values: Option<&mut HashMap<Uuid, AnimationParameterValue>>,
    ) {
        let list_id = Id::new("animation_graph").with("parameters");
        List::new("Parameters", parameters, |p| p.id).show(
            ui,
            move |ui,
                  ListItemContext {
                      value,
                      index,
                      selected,
                  }| {
                let item_id = list_id.with(index);
                ListItem::new()
                    .selected(selected)
                    .interactive(true)
                    .draggable(true)
                    .show_hierarchical_with_children(
                        ui,
                        item_id,
                        true,
                        LabelContent::new(format!("Parameter {}", index + 1)),
                        |ui| {
                            ui.list_item_flat_noninteractive(
                                PropertyContent::new("Name").value_text_mut(&mut value.name),
                            );
                            ui.list_item_flat_noninteractive(
                                PropertyContent::new("Type").value_fn(|ui, _| {
                                    ComboBox::new(item_id.with("type"), "")
                                        .selected_text(match value.value {
                                            AnimationParameterValue::Float(_) => "Float",
                                            AnimationParameterValue::Int(_) => "Integer",
                                            AnimationParameterValue::Bool(_) => "Boolean",
                                            AnimationParameterValue::Trigger => "Trigger",
                                        })
                                        .show_ui(ui, |ui| {
                                            if ui
                                                .selectable_label(value.value.is_float(), "Float")
                                                .clicked()
                                            {
                                                value.value = AnimationParameterValue::Float(0.0);
                                            }
                                            if ui
                                                .selectable_label(value.value.is_int(), "Integer")
                                                .clicked()
                                            {
                                                value.value = AnimationParameterValue::Int(0);
                                            }
                                            if ui
                                                .selectable_label(value.value.is_bool(), "Boolean")
                                                .clicked()
                                            {
                                                value.value = AnimationParameterValue::Bool(false);
                                            }
                                            if ui
                                                .selectable_label(
                                                    value.value.is_trigger(),
                                                    "Trigger",
                                                )
                                                .clicked()
                                            {
                                                value.value = AnimationParameterValue::Trigger;
                                            }
                                        });
                                }),
                            );
                            let param_value = values
                                .as_mut()
                                .map(|values| {
                                    values
                                        .entry(value.id)
                                        .or_insert(AnimationParameterValue::Float(0.0))
                                })
                                .unwrap_or_else(|| &mut value.value);
                            ui.list_item_flat_noninteractive(
                                PropertyContent::new("Value").value_fn(|ui, _| match param_value {
                                    AnimationParameterValue::Float(value) => {
                                        ui.add(DragValue::new(value).speed(0.01));
                                    }
                                    AnimationParameterValue::Int(value) => {
                                        ui.add(DragValue::new(value));
                                    }
                                    AnimationParameterValue::Bool(value) => {
                                        ui.checkbox(value, "");
                                    }
                                    AnimationParameterValue::Trigger => {
                                        let trigger_id = item_id.with("trigger");
                                        let last_trigger = ui.memory_mut(|mem| {
                                            mem.data
                                                .get_temp::<Option<Instant>>(trigger_id)
                                                .unwrap_or_default()
                                        });
                                        let selected = last_trigger
                                            .map(|time| {
                                                (Instant::now() - time) < Duration::from_secs(3)
                                            })
                                            .unwrap_or(false);
                                        if ui.radio(selected, "").clicked() && !selected {
                                            ui.memory_mut(|mem| {
                                                mem.data
                                                    .insert_temp(trigger_id, Some(Instant::now()))
                                            });
                                        }
                                    }
                                }),
                            );
                        },
                    )
            },
        );
    }

    pub fn node(ui: &mut Ui, asset_registry: &AssetRegistry, graph: &mut AnimationGraph, id: Uuid) {
        let Some(node_idx) = graph.node_indices().find(|ni| graph[*ni].id == id) else {
            return;
        };
        let header_id = Id::new("animation_node_header");
        ListItem::new()
            .interactive(true)
            .force_background(re_ui::design_tokens().section_collapsing_header_color())
            .show_hierarchical_with_children_unindented(
                ui,
                header_id,
                true,
                LabelContent::new("Animation Node").truncate(true),
                |ui| {
                    let AnimationGraph {
                        parameters, graph, ..
                    } = graph;
                    let node = &mut graph[node_idx];
                    let buttons_width = ListButtons::width(ui);
                    ui.list_item_flat_noninteractive(PropertyContent::new("ID").value_fn(
                        |ui, _| {
                            ui.add_sized(
                                ui.available_size() - (buttons_width, 0.0).into(),
                                Label::new(node.id.to_string()).truncate(),
                            );
                        },
                    ));
                    ui.list_item_flat_noninteractive(PropertyContent::new("Name").value_fn(
                        |ui, _| {
                            ui.add(
                                TextEdit::singleline(&mut node.name)
                                    .desired_width(ui.available_width() - buttons_width),
                            );
                        },
                    ));
                    AnimationMotionInspector::motion(
                        ui,
                        asset_registry,
                        &mut node.motion,
                        parameters,
                    );
                },
            );
    }

    pub fn transition(ui: &mut Ui, graph: &mut AnimationGraph, id: Uuid) {
        let Some(edge_idx) = graph.edge_indices().find(|ei| graph[*ei].id == id) else {
            return;
        };
        let Some((source, target)) = graph.edge_endpoints(edge_idx) else {
            return;
        };
        let header_id = Id::new("animation_transition_header");
        let AnimationGraph {
            parameters, graph, ..
        } = graph;
        ListItem::new()
            .interactive(true)
            .force_background(re_ui::design_tokens().section_collapsing_header_color())
            .show_hierarchical_with_children_unindented(
                ui,
                header_id,
                true,
                LabelContent::new("Animation Transition").truncate(true),
                |ui| {
                    let source_name = graph[source].name.clone();
                    let target_name = graph[target].name.clone();
                    ui.list_item_flat_noninteractive(
                        LabelContent::new(format!("{} -> {}", source_name, target_name))
                            .truncate(true),
                    );
                    ui.list_item_flat_noninteractive(
                        PropertyContent::new("Name").value_text_mut(&mut graph[edge_idx].name),
                    );
                    ui.list_item_flat_noninteractive(
                        PropertyContent::new("Has Exit Time")
                            .value_bool_mut(&mut graph[edge_idx].has_exit_time),
                    );
                    ui.list_item_flat_noninteractive(PropertyContent::new("Exit Time").value_fn(
                        |ui, _| {
                            ui.add_enabled_ui(graph[edge_idx].has_exit_time, |ui| {
                                ui.add(DragValue::new(&mut graph[edge_idx].exit_time).speed(0.01));
                            });
                        },
                    ));
                    ui.list_item_flat_noninteractive(PropertyContent::new("Duration").value_fn(
                        |ui, _| {
                            ui.add(DragValue::new(&mut graph[edge_idx].duration).speed(0.01));
                        },
                    ));
                    List::new("Conditions", &mut graph[edge_idx].conditions, |_| {}).show(
                        ui,
                        |ui,
                         ListItemContext {
                             selected, value, ..
                         }| {
                            let item_response = ListItem::new()
                                .selected(selected)
                                .interactive(true)
                                .draggable(true)
                                .show_flat(
                                    ui,
                                    CustomContent::new(|ui, _| {
                                        let mut content_ui =
                                            ui.new_child(UiBuilder::new().max_rect(ui.min_rect()));
                                        content_ui.horizontal_centered(|ui| {
                                            Self::parameter_select(
                                                ui,
                                                parameters,
                                                &mut value.parameter,
                                            );
                                            let param = parameters.iter().find_map(|p| {
                                                if p.id == value.parameter {
                                                    Some(p.value)
                                                } else {
                                                    None
                                                }
                                            });
                                            Self::condition_select(ui, param, &mut value.condition);
                                        });
                                    }),
                                );
                            ShowCollapsingResponse {
                                item_response,
                                body_response: None,
                                openness: 0.0,
                            }
                        },
                    );
                },
            );
    }

    pub fn start_node_select(
        ui: &mut Ui,
        graph: &StableGraph<AnimationNode, AnimationTransition>,
        value: &mut Option<Uuid>,
    ) {
        ui.list_item_flat_noninteractive(PropertyContent::new("Start Node").value_fn(|ui, _| {
            ComboBox::new(value as *const _, "")
                .truncate()
                .width(100.0)
                .selected_text(
                    value
                        .and_then(|id| {
                            graph.node_weights().find_map(|n| {
                                if n.id == id {
                                    Some(n.name.clone())
                                } else {
                                    None
                                }
                            })
                        })
                        .unwrap_or_else(|| String::from("None")),
                )
                .show_ui(ui, |ui| {
                    for node in graph.node_weights() {
                        ui.selectable_value(value, Some(node.id), node.name.as_str());
                    }
                });
        }));
    }

    pub fn parameter_select(ui: &mut Ui, parameters: &Vec<AnimationParameter>, value: &mut Uuid) {
        ui.add_enabled_ui(!parameters.is_empty(), |ui| {
            let selected_parameter = parameters.iter().find(|p| p.id == *value);
            ComboBox::new(Id::new(value as *const _), "")
                .truncate()
                .width(50.0)
                .selected_text(
                    selected_parameter
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| String::from("None")),
                )
                .show_ui(ui, |ui| {
                    for param in parameters {
                        ui.selectable_value(value, param.id, param.name.clone());
                    }
                });
        });
    }

    pub fn condition_select(
        ui: &mut Ui,
        parameter: Option<AnimationParameterValue>,
        value: &mut AnimationCondition,
    ) {
        let id = Id::new(value as *const _);
        match parameter {
            Some(AnimationParameterValue::Float(_)) => {
                if !matches!(value, AnimationCondition::Float(_)) {
                    *value = AnimationCondition::Float(FloatCondition::Less(0.0));
                }
                ComboBox::new(id.with("condition"), "")
                    .truncate()
                    .width(25.0)
                    .selected_text(match *value {
                        AnimationCondition::Float(FloatCondition::Less(_)) => "<",
                        AnimationCondition::Float(FloatCondition::Greater(_)) => ">",
                        _ => "",
                    })
                    .show_ui(ui, |ui| {
                        let matches =
                            matches!(value, AnimationCondition::Float(FloatCondition::Less(_)));
                        if ui.selectable_label(matches, "<").clicked() && !matches {
                            *value = AnimationCondition::Float(FloatCondition::Less(0.0));
                        }
                        let matches =
                            matches!(value, AnimationCondition::Float(FloatCondition::Greater(_)));
                        if ui.selectable_label(matches, ">").clicked() && !matches {
                            *value = AnimationCondition::Float(FloatCondition::Greater(0.0));
                        }
                    });
                let Some(condition_value) = (match value {
                    AnimationCondition::Float(FloatCondition::Greater(value)) => Some(value),
                    AnimationCondition::Float(FloatCondition::Less(value)) => Some(value),
                    _ => None,
                }) else {
                    return;
                };
                ui.add(DragValue::new(condition_value));
            }
            Some(AnimationParameterValue::Int(_)) => {
                if !matches!(value, AnimationCondition::Int(_)) {
                    *value = AnimationCondition::Int(IntCondition::Less(0));
                }
                ComboBox::new(id.with("condition"), "")
                    .truncate()
                    .width(25.0)
                    .selected_text(match *value {
                        AnimationCondition::Int(IntCondition::Less(_)) => "<",
                        AnimationCondition::Int(IntCondition::Greater(_)) => ">",
                        AnimationCondition::Int(IntCondition::Equal(_)) => "==",
                        AnimationCondition::Int(IntCondition::NotEqual(_)) => "!=",
                        _ => "",
                    })
                    .show_ui(ui, |ui| {
                        let matches =
                            matches!(value, AnimationCondition::Int(IntCondition::Less(_)));
                        if ui.selectable_label(matches, "<").clicked() && !matches {
                            *value = AnimationCondition::Int(IntCondition::Less(0));
                        }
                        let matches =
                            matches!(value, AnimationCondition::Int(IntCondition::Greater(_)));
                        if ui.selectable_label(matches, ">").clicked() && !matches {
                            *value = AnimationCondition::Int(IntCondition::Greater(0));
                        }
                        let matches =
                            matches!(value, AnimationCondition::Int(IntCondition::Equal(_)));
                        if ui.selectable_label(matches, "==").clicked() && !matches {
                            *value = AnimationCondition::Int(IntCondition::Equal(0));
                        }
                        let matches =
                            matches!(value, AnimationCondition::Int(IntCondition::NotEqual(_)));
                        if ui.selectable_label(matches, "!=").clicked() && !matches {
                            *value = AnimationCondition::Int(IntCondition::NotEqual(0));
                        }
                    });
                let Some(condition_value) = (match value {
                    AnimationCondition::Int(IntCondition::Greater(value)) => Some(value),
                    AnimationCondition::Int(IntCondition::Less(value)) => Some(value),
                    AnimationCondition::Int(IntCondition::Equal(value)) => Some(value),
                    AnimationCondition::Int(IntCondition::NotEqual(value)) => Some(value),
                    _ => None,
                }) else {
                    return;
                };
                ui.add(DragValue::new(condition_value));
            }
            Some(AnimationParameterValue::Bool(_)) => {
                if !matches!(value, AnimationCondition::Bool(_)) {
                    *value = AnimationCondition::Bool(BoolCondition::True);
                }
                ComboBox::new(id.with("condition"), "")
                    .truncate()
                    .width(25.0)
                    .selected_text(match *value {
                        AnimationCondition::Bool(BoolCondition::True) => "true",
                        AnimationCondition::Bool(BoolCondition::False) => "false",
                        _ => "",
                    })
                    .show_ui(ui, |ui| {
                        let matches =
                            matches!(value, AnimationCondition::Bool(BoolCondition::True));
                        if ui.selectable_label(matches, "true").clicked() && !matches {
                            *value = AnimationCondition::Bool(BoolCondition::True);
                        }
                        let matches =
                            matches!(value, AnimationCondition::Bool(BoolCondition::False));
                        if ui.selectable_label(matches, "false").clicked() && !matches {
                            *value = AnimationCondition::Bool(BoolCondition::False);
                        }
                    });
            }
            Some(AnimationParameterValue::Trigger) => {
                if !matches!(value, AnimationCondition::Trigger) {
                    *value = AnimationCondition::Trigger;
                }
            }
            _ => {}
        }
    }
}

pub struct AnimationMotionInspector;

impl AnimationMotionInspector {
    pub fn motion(
        ui: &mut Ui,
        asset_registry: &AssetRegistry,
        motion: &mut AnimationMotion,
        params: &Vec<AnimationParameter>,
    ) {
        let id = Id::new(motion as *const _);
        match motion {
            AnimationMotion::AnimationClip(AnimationClip { animation, speed }) => {
                ui.list_item_flat_noninteractive(PropertyContent::new("Animation").value_fn(
                    |ui, _| {
                        Widgets::asset_select_t(
                            ui,
                            asset_registry,
                            id.with("animation"),
                            Some(Animation::type_uuid()),
                            animation,
                        );
                    },
                ));
                ui.list_item_flat_noninteractive(PropertyContent::new("Speed").value_fn(
                    |ui, _| {
                        ui.add(DragValue::new(speed).speed(0.01));
                    },
                ));
            }
            AnimationMotion::BlendTree1D(BlendTree {
                parameters,
                motions,
            }) => {
                parameters.resize(1, Uuid::nil());
                ui.list_item_flat_noninteractive(PropertyContent::new("Parameter").value_fn(
                    |ui, _| {
                        ComboBox::from_id_salt(id.with("parameter"))
                            .selected_text(
                                params
                                    .iter()
                                    .filter(|p| {
                                        matches!(p.value, AnimationParameterValue::Float(_))
                                    })
                                    .find_map(|p| {
                                        if p.id == parameters[0] {
                                            Some(p.name.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or_else(|| String::from("None")),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut parameters[0], Uuid::nil(), "None");
                                for param in params.iter().filter(|p| {
                                    matches!(p.value, AnimationParameterValue::Float(_))
                                }) {
                                    ui.selectable_value(
                                        &mut parameters[0],
                                        param.id,
                                        param.name.clone(),
                                    );
                                }
                            });
                    },
                ));
                List::new("Motions", motions, |_| {}).show(
                    ui,
                    |ui,
                     ListItemContext {
                         value,
                         index,
                         selected,
                     }| {
                        let item_id = id.with(index);
                        ListItem::new()
                            .selected(selected)
                            .draggable(true)
                            .show_hierarchical_with_children(
                                ui,
                                item_id,
                                false,
                                LabelContent::new(format!("Motion {}", index + 1)).truncate(true),
                                |ui| {
                                    Self::motion(ui, asset_registry, &mut value.motion, params);
                                    ui.list_item_flat_noninteractive(
                                        PropertyContent::new("Threshold").value_fn(|ui, _| {
                                            ui.add(
                                                DragValue::new(&mut value.threshold[0]).speed(0.1),
                                            );
                                        }),
                                    );
                                },
                            )
                    },
                );
            }
            AnimationMotion::BlendTree2D(_) => {}
        }
    }
}
