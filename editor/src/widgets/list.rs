use engine::egui::{Id, Response, Ui};
use re_ui::list_item::{ItemButton, ListItem, PropertyContent};
use re_ui::UiExt;

#[derive(Default, Clone, Copy)]
struct ListState {
    selected: Option<usize>,
}

struct ListButtons<'a> {
    pub on_click_add: Option<Box<dyn FnOnce() + 'a>>,
    pub on_click_remove: Option<Box<dyn FnOnce() + 'a>>,
    pub remove_enabled: bool,
}

impl<'a> ListButtons<'a> {
    pub fn new() -> Self {
        Self {
            on_click_add: None,
            on_click_remove: None,
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

    pub fn remove_enabled(mut self, enabled: bool) -> Self {
        self.remove_enabled = enabled;
        self
    }
}

impl<'a> ItemButton for ListButtons<'a> {
    fn ui(self: Box<Self>, ui: &mut Ui) -> Response {
        let add = ui.small_icon_button(&re_ui::icons::ADD);
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

pub struct ListItemContext<'a, T> {
    pub value: &'a mut T,
    pub index: usize,
    pub selected: bool,
}

pub struct List<'a, T> {
    id: Id,
    label: String,
    list: &'a mut Vec<T>,
    default_open: bool,
    show_count: bool,
}

impl<'a, T: Default> List<'a, T> {
    pub fn new(label: impl Into<String>, list: &'a mut Vec<T>) -> Self {
        Self {
            id: Id::new(list as *const _),
            label: label.into(),
            list,
            default_open: true,
            show_count: true,
        }
    }

    pub fn show_count(mut self, show_count: bool) -> Self {
        self.show_count = show_count;
        self
    }

    pub fn default_open(mut self, default_open: bool) -> Self {
        self.default_open = default_open;
        self
    }

    pub fn show<F: FnMut(&mut Ui, ListItemContext<T>) -> Response + 'a>(
        self,
        ui: &mut Ui,
        mut list_item_contents: F,
    ) -> Response {
        let Self {
            id,
            label,
            list,
            default_open,
            show_count,
        } = self;
        let mut state = ui.memory_mut(|mem| *mem.data.get_temp_mut_or_default::<ListState>(id));
        let mut add = false;
        let mut remove = false;
        let mut text = label.clone();
        if show_count {
            text = format!("{} ({})", label, list.len());
        }
        let res = ListItem::new()
            .interactive(false)
            .show_hierarchical_with_children(
                ui,
                id,
                default_open,
                PropertyContent::new(text).button(
                    ListButtons::new()
                        .remove_enabled(state.selected.is_some())
                        .on_click_add(|| {
                            add = true;
                        })
                        .on_click_remove(|| {
                            remove = true;
                        }),
                ),
                |ui| {
                    for (index, value) in list.iter_mut().enumerate() {
                        let selected = state.selected == Some(index);
                        let res = list_item_contents(
                            ui,
                            ListItemContext {
                                value,
                                index,
                                selected,
                            },
                        );
                        if res.clicked() {
                            if state.selected == Some(index) {
                                state.selected = None;
                            } else {
                                state.selected = Some(index);
                            }
                        }
                    }
                },
            )
            .item_response;
        if add {
            list.push(Default::default());
        }
        if remove {
            if let Some(index) = state.selected {
                list.remove(index);
                state.selected = None;
            }
        }
        ui.memory_mut(|mem| mem.data.insert_temp(id, state));
        res
    }
}

impl List<'static, ()> {
    pub fn buttons_width(ui: &Ui) -> f32 {
        2.0 * re_ui::DesignTokens::small_icon_size().x + ui.style().spacing.item_spacing.x
    }
}
