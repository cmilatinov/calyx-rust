mod code_editor;
mod content_browser;
mod inspector;
mod scene_hierarchy;

use std::collections::HashMap;
pub use self::code_editor::*;
pub use self::content_browser::*;
pub use self::inspector::*;
pub use self::scene_hierarchy::*;
use egui::{Ui, WidgetText};

pub trait Panel {
    fn name() -> &'static str where Self: Sized;
    fn ui(&mut self, ui: &mut Ui);
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
        panels.insert(PanelCodeEditor::name().to_string(), Box::new(PanelCodeEditor::default()));

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
}
