mod panel;

use eframe::{egui, NativeOptions};
use self::panel::*;
use egui_dock::{DockArea, NodeIndex, Style, Tree};

pub struct Editor;
impl Editor {
    pub fn run(&self) -> eframe::Result<()> {
        let options = NativeOptions {
            decorated: true,
            transparent: true,
            min_window_size: Some(egui::vec2(1280.0, 720.0)),
            initial_window_size: Some(egui::vec2(1280.0, 720.0)),
            ..Default::default()
        };
        eframe::run_native(
            "Calyx",
            options,
            Box::new(|_cc| Box::<EditorApp>::default()),
        )
    }
}

struct EditorApp {
    tree: Tree<String>,
}

impl Default for EditorApp {
    fn default() -> Self {
        let mut tree = Tree::new(vec![
            PanelSceneHierarchy::name().to_owned(),
        ]);

        let [_, b] = tree.split_right(NodeIndex::root(), 0.2, vec![PanelInspector::name().to_owned()]);
        let [_, _] = tree.split_below(b, 0.7, vec![PanelContentBrowser::name().to_owned()]);

        Self {
          tree
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        DockArea::new(&mut self.tree)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show(ctx, &mut PanelManager::default());
    }
}