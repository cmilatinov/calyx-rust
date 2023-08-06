mod panel;
pub mod syntax_highlighting;

use eframe::{egui, NativeOptions};
use self::panel::*;
use egui_dock::{DockArea, NodeIndex, Style, Tree};

pub struct Editor {
    tree: Tree<String>,
}

impl Default for Editor {
    fn default() -> Self {
        let mut tree = Tree::new(vec![
            PanelSceneHierarchy::name().to_owned(),
        ]);

        let [a, _] = tree.split_below(NodeIndex::root(), 0.7, vec![PanelContentBrowser::name().to_owned()]);
        let [_, b] = tree.split_right(a, 0.2, vec![PanelCodeEditor::name().to_owned()]);
        let [_, _] = tree.split_right(b, 0.8, vec![PanelInspector::name().to_owned()]);

        Self {
          tree
        }
    }
}

impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        DockArea::new(&mut self.tree)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show(ctx, &mut PanelManager::default());
    }
}