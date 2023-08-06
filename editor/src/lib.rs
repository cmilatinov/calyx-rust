mod panel;

use eframe::{egui, NativeOptions};
use self::panel::*;
use egui_dock::{DockArea, NodeIndex, Style, Tree};

pub struct Editor;
impl Editor {
    pub fn run(&self) -> eframe::Result<()> {
        let options = NativeOptions::default();
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
        Self {
            tree: Tree::new(vec![
                PanelContentBrowser::name().to_owned(),
                PanelInspector::name().to_owned(),
                PanelSceneHierarchy::name().to_owned(),
            ])
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