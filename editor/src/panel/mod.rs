
use std::collections::HashMap;

use egui::{Ui, WidgetText};

use engine::*;
use engine::egui_dock::TabStyle;

pub use self::content_browser::*;
pub use self::inspector::*;
pub use self::scene_hierarchy::*;
pub use self::terminal::*;
pub use self::viewport::*;

mod content_browser;
mod inspector;
mod scene_hierarchy;
mod terminal;
mod viewport;

pub trait Panel {
    fn name() -> &'static str where Self: Sized;
    fn ui(&self, ui: &mut Ui);
    fn tab_style_override(&self, _global_style: &TabStyle) -> Option<TabStyle> {
        None
    }
}

pub struct PanelManager {
    panels: HashMap<String, Box<dyn Panel>>
}

impl Default for PanelManager {
    fn default() -> Self {
        let mut panels: HashMap<String, Box<dyn Panel>> = HashMap::new();
        panels.insert(PanelContentBrowser::name().to_string(), Box::new(PanelContentBrowser));
        panels.insert(PanelInspector::name().to_string(), Box::new(PanelInspector));
        panels.insert(PanelSceneHierarchy::name().to_string(), Box::new(PanelSceneHierarchy::default()));
        panels.insert(PanelTerminal::name().to_string(), Box::new(PanelTerminal::default()));
        panels.insert(PanelViewport::name().to_string(), Box::new(PanelViewport::default()));
        PanelManager {
            panels
        }
    }
}

impl egui_dock::TabViewer for PanelManager {
    type Tab = String;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        if let Some(panel) = self.panels.get_mut(tab) {
            panel.ui(ui);
        };
    }

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.as_str().into()
    }

    fn tab_style_override(&self, tab: &Self::Tab, global_style: &TabStyle) -> Option<TabStyle> {
        if let Some(panel) = self.panels.get(tab) {
            panel.tab_style_override(global_style)
        } else {
            None
        }
    }
}
