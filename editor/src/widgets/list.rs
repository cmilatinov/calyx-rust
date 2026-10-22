use eframe::egui::collapsing_header::CollapsingState;
use eframe::egui::{Color32, CursorIcon, DragAndDrop, Id, Response, Ui};
use re_ui::drag_and_drop::{find_drop_target, ItemContext, ItemKind};
use re_ui::list_item::{
    ItemButton, LabelContent, ListItem, PropertyContent, ShowCollapsingResponse,
};
use re_ui::{DesignTokens, UiExt};
use std::fmt::Debug;

/// A list the [`List`] widget can show and edit.
///
/// Items may be unsized, such as `dyn Reflect`, so the widget never moves an
/// item by value: it adds, removes and reorders through the container.
pub trait ListContainer {
    type Item: ?Sized;
    fn len(&self) -> usize;
    fn nth(&self, index: usize) -> &Self::Item;
    fn nth_mut(&mut self, index: usize) -> &mut Self::Item;
    /// Whether [`Self::push_default`] can add an item.
    fn can_add(&self) -> bool {
        true
    }
    /// Appends a default item.
    fn push_default(&mut self);
    /// Removes the item at `index`.
    fn remove(&mut self, index: usize);
    /// Moves the item at `from` so it ends up at index `to`.
    fn move_item(&mut self, from: usize, to: usize);
}

impl<T: Default> ListContainer for Vec<T> {
    type Item = T;

    fn len(&self) -> usize {
        self.len()
    }

    fn nth(&self, index: usize) -> &Self::Item {
        self.get(index).unwrap()
    }

    fn nth_mut(&mut self, index: usize) -> &mut Self::Item {
        self.get_mut(index).unwrap()
    }

    fn push_default(&mut self) {
        self.push(T::default());
    }

    fn remove(&mut self, index: usize) {
        Vec::remove(self, index);
    }

    fn move_item(&mut self, from: usize, to: usize) {
        let item = Vec::remove(self, from);
        Vec::insert(self, to, item);
    }
}

#[derive(Default, Clone, Copy)]
pub struct ListState {
    pub selected: Option<usize>,
}

pub struct ListButtons<'a> {
    pub on_click_add: Option<Box<dyn FnOnce() + 'a>>,
    pub on_click_remove: Option<Box<dyn FnOnce() + 'a>>,
    pub add_enabled: bool,
    pub remove_enabled: bool,
}

impl<'a> ListButtons<'a> {
    pub fn new() -> Self {
        Self {
            on_click_add: None,
            on_click_remove: None,
            add_enabled: true,
            remove_enabled: true,
        }
    }

    pub fn on_click_add<F: FnOnce() + 'a>(mut self, on_click: F) -> Self {
        self.on_click_add = Some(Box::new(on_click));
        self
    }

    pub fn on_click_remove<F: FnOnce() + 'a>(mut self, on_click: F) -> Self {
        self.on_click_remove = Some(Box::new(on_click));
        self
    }

    pub fn add_enabled(mut self, enabled: bool) -> Self {
        self.add_enabled = enabled;
        self
    }

    pub fn remove_enabled(mut self, enabled: bool) -> Self {
        self.remove_enabled = enabled;
        self
    }
}

impl<'a> ItemButton for ListButtons<'a> {
    fn ui(self: Box<Self>, ui: &mut Ui) -> Response {
        let add_button = ui.small_icon_button_widget(&re_ui::icons::ADD);
        let add = ui.add_enabled(self.add_enabled, add_button);
        if let Some(on_click_add) = self.on_click_add {
            if add.clicked() {
                on_click_add();
            }
        }
        let remove_button = ui.small_icon_button_widget(&re_ui::icons::REMOVE);
        let remove = ui.add_enabled(self.remove_enabled, remove_button);
        if let Some(on_click_remove) = self.on_click_remove {
            if remove.clicked() {
                on_click_remove();
            }
        }
        add | remove
    }
}

impl ListButtons<'static> {
    pub fn width(ui: &Ui) -> f32 {
        2.0 * DesignTokens::small_icon_size().x + ui.style().spacing.item_spacing.x
    }
}

pub struct ListItemContext<'a, T: ?Sized> {
    pub value: &'a mut T,
    pub index: usize,
    pub selected: bool,
}

pub struct List<'a, K, T: ?Sized> {
    id: Id,
    label: String,
    create_key: Box<dyn FnMut(&T) -> K + 'a>,
    list: &'a mut dyn ListContainer<Item = T>,
    default_open: bool,
    show_count: bool,
    header: bool,
    interactive: bool,
    drag_and_drop: bool,
    indented: bool,
}

#[allow(dead_code)]
impl<'a, K: Default + Debug + Copy + Send + Sync + 'static, T: ?Sized> List<'a, K, T> {
    pub fn new<F: FnMut(&T) -> K + 'a>(
        label: impl Into<String>,
        list: &'a mut impl ListContainer<Item = T>,
        create_key: F,
    ) -> Self {
        let string = label.into();
        Self {
            id: Id::new(string.as_str()),
            label: string,
            create_key: Box::new(create_key),
            list,
            default_open: true,
            show_count: true,
            header: false,
            interactive: false,
            drag_and_drop: true,
            indented: true,
        }
    }

    /// Replaces the id that keeps the list's collapsed and selection state,
    /// which defaults to one derived from the label.
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = id.into();
        self
    }

    pub fn indented(mut self, indented: bool) -> Self {
        self.indented = indented;
        self
    }

    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    pub fn header(mut self, header: bool) -> Self {
        self.header = header;
        self
    }

    pub fn show_count(mut self, show_count: bool) -> Self {
        self.show_count = show_count;
        self
    }

    pub fn default_open(mut self, default_open: bool) -> Self {
        self.default_open = default_open;
        self
    }

    pub fn drag_and_drop(mut self, drag_and_drop: bool) -> Self {
        self.drag_and_drop = drag_and_drop;
        self
    }

    pub fn show_default<F: FnMut(ListItemContext<T>) -> String + 'a>(
        self,
        ui: &mut Ui,
        item_labels: F,
    ) -> (ShowCollapsingResponse<()>, ListState) {
        self.show_default_with_response(ui, item_labels, |_, _| {})
    }

    pub fn show_default_with_response<
        F: FnMut(ListItemContext<T>) -> String + 'a,
        R: FnMut(ListItemContext<T>, &Response) + 'a,
    >(
        self,
        ui: &mut Ui,
        mut item_labels: F,
        mut on_response: R,
    ) -> (ShowCollapsingResponse<()>, ListState) {
        let drag_and_drop = self.drag_and_drop;
        self.show(
            ui,
            move |ui,
                  ListItemContext {
                      value,
                      index,
                      selected,
                  }| {
                let response = ui
                    .list_item()
                    .interactive(true)
                    .draggable(drag_and_drop)
                    .selected(selected)
                    .show_flat(
                        ui,
                        LabelContent::new(item_labels(ListItemContext {
                            value,
                            index,
                            selected,
                        })),
                    );
                on_response(
                    ListItemContext {
                        value,
                        index,
                        selected,
                    },
                    &response,
                );
                ShowCollapsingResponse {
                    item_response: response,
                    body_response: None,
                    openness: 0.0,
                }
            },
        )
    }

    pub fn show<F: FnMut(&mut Ui, ListItemContext<T>) -> ShowCollapsingResponse<()> + 'a>(
        self,
        ui: &mut Ui,
        mut list_item_contents: F,
    ) -> (ShowCollapsingResponse<()>, ListState) {
        let Self {
            id,
            label,
            mut create_key,
            list,
            default_open,
            show_count,
            header,
            interactive,
            drag_and_drop,
            indented,
        } = self;
        let mut state = ui.memory_mut(|mem| *mem.data.get_temp_mut_or_default::<ListState>(id));
        let mut add = false;
        let mut remove = false;
        let mut text = label.clone();
        if show_count {
            text = format!("{} ({})", label, list.len());
        }
        let mut list_item = ListItem::new().interactive(interactive);
        if header {
            list_item = list_item
                .force_background(re_ui::design_tokens().section_collapsing_header_color());
        }
        let list_item_content = PropertyContent::new(text).button(
            ListButtons::new()
                .add_enabled(list.can_add())
                .remove_enabled(state.selected.is_some())
                .on_click_add(|| {
                    add = true;
                })
                .on_click_remove(|| {
                    remove = true;
                }),
        );
        let show_list_items = |ui: &mut Ui| {
            for index in 0..list.len() {
                let value = list.nth_mut(index);
                let key = create_key(value);
                let selected = state.selected == Some(index);
                let response = list_item_contents(
                    ui,
                    ListItemContext {
                        value,
                        index,
                        selected,
                    },
                );
                if response.item_response.clicked() {
                    if state.selected == Some(index) {
                        state.selected = None;
                    } else {
                        state.selected = Some(index);
                    }
                }
                if !drag_and_drop {
                    continue;
                }
                if response.item_response.drag_started() {
                    DragAndDrop::set_payload(ui.ctx(), (id, index, key));
                }
                // The payload names its list, so a drag from another list
                // with the same key type never reorders this one.
                let Some(dragged_idx) = DragAndDrop::payload::<(Id, usize, K)>(ui.ctx())
                    .filter(|payload| payload.0 == id)
                    .map(|payload| payload.1)
                else {
                    continue;
                };

                let previous_container_id = if index > 0 {
                    let prev_key = create_key(list.nth(index - 1));
                    Some((index - 1, prev_key))
                } else {
                    None
                };
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                let item_desc = ItemContext {
                    id: (index, key),
                    item_kind: ItemKind::Leaf {
                        parent_id: (usize::MAX, K::default()),
                        position_index_in_parent: index,
                    },
                    previous_container_id,
                };

                let Some(drop_target) = find_drop_target(
                    ui,
                    &item_desc,
                    response.item_response.rect,
                    response.body_response.map(|res| res.response.rect),
                    DesignTokens::list_item_height(),
                ) else {
                    continue;
                };
                let drop_index = (if drop_target.target_parent_id.0 != usize::MAX {
                    drop_target.target_parent_id.0 + 1
                } else {
                    drop_target.target_position_index
                })
                .clamp(0, list.len());

                ui.painter().hline(
                    drop_target.indicator_span_x,
                    drop_target.indicator_position_y,
                    (2.0, Color32::WHITE),
                );

                if ui.input(|i| i.pointer.primary_released()) {
                    let target = if dragged_idx < drop_index {
                        drop_index - 1
                    } else {
                        drop_index
                    };
                    if dragged_idx != target {
                        list.move_item(dragged_idx, target);
                    }
                    DragAndDrop::clear_payload(ui.ctx());
                }
            }
        };
        let res = if indented {
            list_item.show_hierarchical_with_children(
                ui,
                id,
                default_open,
                list_item_content,
                show_list_items,
            )
        } else {
            list_item.show_hierarchical_with_children_unindented(
                ui,
                id,
                default_open,
                list_item_content,
                show_list_items,
            )
        };
        if res.item_response.clicked() {
            if let Some(mut state) = CollapsingState::load(ui.ctx(), id) {
                state.toggle(ui);
                state.store(ui.ctx());
            }
        }
        if add {
            list.push_default();
        }
        if remove {
            if let Some(index) = state.selected {
                list.remove(index);
                state.selected = None;
            }
        }
        ui.memory_mut(|mem| mem.data.insert_temp(id, state));
        (res, state)
    }
}
