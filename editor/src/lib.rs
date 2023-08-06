mod panel;
pub mod syntax_highlighting;

use engine::*;
use engine::eframe::{egui, NativeOptions};
use engine::egui_dock::{DockArea, NodeIndex, Style, Tree};
use engine::core::time::Time;
use engine::egui_wgpu::WgpuConfiguration;
use self::panel::*;

pub struct Editor;
impl Editor {
    pub fn run(&self) -> eframe::Result<()> {
        let options = NativeOptions {
            decorated: true,
            transparent: true,
            min_window_size: Some(egui::vec2(1280.0, 720.0)),
            initial_window_size: Some(egui::vec2(1280.0, 720.0)),
            renderer: eframe::Renderer::Wgpu,
            wgpu_options: WgpuConfiguration::default(),
            ..Default::default()
        };
        eframe::run_native(
            "Calyx",
            options,
            Box::new(|cc| Box::<EditorApp>::new(EditorApp::new(cc))),
        )
    }
}

struct EditorApp {
    fps: i32,
    tree: Tree<String>,
    panel_manager: PanelManager
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let wgpu_render_state = cc.wgpu_render_state.as_ref().unwrap();

        Time::init();

        let adapter = &wgpu_render_state.adapter;
        let info = adapter.get_info();
        println!("Name: {}", info.name);
        println!("Device Type: {:?}", info.device_type);
        println!("Driver Info: {}", info.driver_info);
        println!("Driver: {}", info.driver);
        println!("Vendor: {}", info.vendor);

        let mut tree = Tree::new(vec![
            PanelSceneHierarchy::name().to_owned(),
        ]);

        let [_, b] = tree.split_right(NodeIndex::root(), 0.2, vec![PanelViewport::name().to_owned()]);
        let [_, c] = tree.split_right(b, 0.8, vec![PanelInspector::name().to_owned()]);
        let [_, _] = tree.split_below(c, 0.7, vec![PanelContentBrowser::name().to_owned()]);

        Self {
            fps: 0,
            tree
        }
    }
}

impl Default for EditorApp {
    fn default() -> Self {
        let mut tree = Tree::new(vec![
            PanelSceneHierarchy::name().to_owned(),
        ]);

        let [_, b] = tree.split_right(NodeIndex::root(), 0.2, vec![PanelViewport::name().to_owned()]);
        let [_, c] = tree.split_right(b, 0.8, vec![PanelInspector::name().to_owned()]);
        let [_, _] = tree.split_below(c, 0.7, vec![PanelContentBrowser::name().to_owned()]);

        Self {
            fps: 0,
            tree
        }
    }
}

impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        Time::update_time();
        DockArea::new(&mut self.tree)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show(ctx, &mut PanelManager::default());
        self.fps += 1;
        if Time::timer("fps") >= 1.0 {
            println!("{}", self.fps);
            self.fps = 0;
            Time::reset_timer("fps");
        }
        ctx.request_repaint();
    }
}