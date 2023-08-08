mod panel;
pub mod syntax_highlighting;

use engine::*;
use eframe::{egui};
use egui_dock::{DockArea, NodeIndex, Style, Tree};
use engine::assets::AssetRegistry;
use engine::core::time::Time;
use engine::render::{SceneRenderer};
use self::panel::*;

pub struct EditorApp {
    fps: i32,
    tree: Tree<String>,
    panel_manager: PanelManager
}

pub struct EditorAppResources {
    pub scene_renderer: SceneRenderer
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        Time::init();
        AssetRegistry::init();

        let wgpu_render_state = cc.wgpu_render_state.as_ref().unwrap();
        wgpu_render_state.renderer
            .write()
            .paint_callback_resources
            .insert(EditorAppResources {
                scene_renderer: SceneRenderer::new(wgpu_render_state)
            });

        let mut tree = Tree::new(vec![
            PanelSceneHierarchy::name().to_owned(),
        ]);
        let [_, b] = tree.split_right(NodeIndex::root(), 0.2, vec![
            PanelViewport::name().to_owned(),
            PanelCodeEditor::name().to_owned()
        ]);
        let [c, _] = tree.split_right(b, 0.8, vec![PanelInspector::name().to_owned()]);
        let [_, _] = tree.split_below(c, 0.7, vec![PanelContentBrowser::name().to_owned()]);

        Self {
            fps: 0,
            tree,
            panel_manager: PanelManager::default()
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        Time::update_time();
        DockArea::new(&mut self.tree)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show(ctx, &mut self.panel_manager);
        self.fps += 1;
        if Time::timer("fps") >= 1.0 {
            println!("{}", self.fps);
            self.fps = 0;
            Time::reset_timer("fps");
        }
        ctx.request_repaint();
    }
}