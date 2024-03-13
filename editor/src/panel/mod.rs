use std::collections::HashMap;

use egui::{Ui, WidgetText};

use engine::egui_dock::TabStyle;
use engine::*;

pub use self::content_browser::*;
pub use self::game::*;
pub use self::inspector::*;
pub use self::scene_hierarchy::*;
pub use self::terminal::*;
pub use self::viewport::*;

mod content_browser;
mod game;
mod inspector;
mod scene_hierarchy;
mod terminal;
mod viewport;

pub trait Panel {
    fn name() -> &'static str
    where
        Self: Sized;
    fn ui(&mut self, ui: &mut Ui);
    fn context_menu(&mut self, _ui: &mut Ui) {}
    fn tab_style_override(&self, _global_style: &TabStyle) -> Option<TabStyle> {
        None
    }
    fn scroll_bars(&self) -> [bool; 2] {
        [true, true]
    }
}

pub struct PanelManager {
    panels: HashMap<String, Box<dyn Panel>>,
}

impl Default for PanelManager {
    fn default() -> Self {
        let mut panels: HashMap<String, Box<dyn Panel>> = HashMap::new();
        panels.insert(
            PanelContentBrowser::name().to_string(),
            Box::<PanelContentBrowser>::default(),
        );
        panels.insert(
            PanelInspector::name().to_string(),
            Box::<PanelInspector>::default(),
        );
        panels.insert(
            PanelSceneHierarchy::name().to_string(),
            Box::<PanelSceneHierarchy>::default(),
        );
        panels.insert(
            PanelTerminal::name().to_string(),
            Box::<PanelTerminal>::default(),
        );
        panels.insert(
            PanelViewport::name().to_string(),
            Box::<PanelViewport>::default(),
        );
        panels.insert(PanelGame::name().to_string(), Box::<PanelGame>::default());
        PanelManager { panels }
    }
}

impl egui_dock::TabViewer for PanelManager {
    type Tab = String;

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.as_str().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        if let Some(panel) = self.panels.get_mut(tab) {
            panel.ui(ui);
        };
    }

    fn tab_style_override(&self, tab: &Self::Tab, global_style: &TabStyle) -> Option<TabStyle> {
        if let Some(panel) = self.panels.get(tab) {
            panel.tab_style_override(global_style)
        } else {
            None
        }
    }

    fn context_menu(
        &mut self,
        ui: &mut Ui,
        tab: &mut Self::Tab,
        _surface: egui_dock::SurfaceIndex,
        _node: egui_dock::NodeIndex,
    ) {
        if let Some(panel) = self.panels.get_mut(tab) {
            panel.context_menu(ui);
        }
    }

    fn scroll_bars(&self, tab: &Self::Tab) -> [bool; 2] {
        if let Some(panel) = self.panels.get(tab) {
            panel.scroll_bars()
        } else {
            [true, true]
        }
    }
}
