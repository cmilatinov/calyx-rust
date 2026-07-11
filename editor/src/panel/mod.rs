use egui::{Ui, WidgetText};
use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;

pub use self::animator::*;
pub use self::content_browser::*;
pub use self::game::*;
pub use self::inspector::*;
pub use self::scene_hierarchy::*;
pub use self::terminal::*;
pub use self::viewport::*;
use crate::widgets::{TabDesc, TabWidget};
use crate::EditorAppState;
use egui::{Id, InnerResponse, Response};
use egui_tiles::{SimplificationOptions, TabState, Tile, TileId, Tiles, UiResponse};

mod animator;
mod content_browser;
mod game;
mod inspector;
mod scene_hierarchy;
mod terminal;
mod viewport;

#[allow(unused)]
pub trait Panel: Any {
    fn name() -> &'static str
    where
        Self: Sized;
    fn icon(&self) -> Option<&'static re_ui::Icon> {
        None
    }
    fn ui(&mut self, ui: &mut Ui, state: &mut EditorAppState);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub struct Panels {
    inner: HashMap<&'static str, Box<dyn Panel>>,
}

impl Panels {
    pub fn new(root_path: impl Into<PathBuf>) -> Self {
        let mut panels: HashMap<&'static str, Box<dyn Panel>> = HashMap::new();
        panels.insert(
            PanelContentBrowser::name(),
            Box::new(PanelContentBrowser::new(root_path)),
        );
        panels.insert(PanelInspector::name(), Box::<PanelInspector>::default());
        panels.insert(
            PanelSceneHierarchy::name(),
            Box::<PanelSceneHierarchy>::default(),
        );
        panels.insert(PanelTerminal::name(), Box::<PanelTerminal>::default());
        panels.insert(PanelViewport::name(), Box::<PanelViewport>::default());
        panels.insert(PanelGame::name(), Box::<PanelGame>::default());
        panels.insert(PanelAnimator::name(), Box::<PanelAnimator>::default());
        Panels { inner: panels }
    }

    pub fn panel<T: Panel>(&self) -> Option<&T> {
        self.inner
            .get(T::name())
            .and_then(|panel| (**panel).as_any().downcast_ref())
    }

    fn panel_icon(
        &self,
        tiles: &Tiles<&'static str>,
        tile_id: TileId,
    ) -> Option<&'static re_ui::Icon> {
        tiles
            .get(tile_id)
            .and_then(|t| match t {
                Tile::Pane(pane) => self.inner.get(pane),
                _ => None,
            })
            .and_then(|panel| panel.icon())
    }
}

pub struct PanelManager<'a> {
    pub panels: &'a mut Panels,
    pub state: &'a mut EditorAppState,
}

impl egui_tiles::Behavior<&'static str> for PanelManager<'_> {
    fn pane_ui(&mut self, ui: &mut Ui, _tile_id: TileId, pane: &mut &'static str) -> UiResponse {
        if let Some(panel) = self.panels.inner.get_mut(pane) {
            panel.ui(ui, self.state);
        }
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &&'static str) -> WidgetText {
        (*pane).into()
    }

    fn tab_ui(
        &mut self,
        tiles: &mut Tiles<&'static str>,
        ui: &mut Ui,
        id: Id,
        tile_id: TileId,
        tab_state: &TabState,
    ) -> Response {
        let label = self.tab_title_for_tile(tiles, tile_id);
        let icon = self.panels.panel_icon(tiles, tile_id);
        let InnerResponse { response, .. } = TabWidget::new(
            self,
            ui,
            tiles,
            tile_id,
            tab_state,
            TabDesc {
                label,
                icon,
                selected: false,
                hovered: false,
                closeable: false,
            },
            1.0,
        )
        .show(ui, id, tab_state);
        response
    }

    // fn drag_ui(&mut self, tiles: &Tiles<&'static str>, ui: &mut Ui, tile_id: TileId) {
    //     let label = self.tab_title_for_tile(tiles, tile_id);
    //     let icon = self.panel_icon(tiles, tile_id);
    //     let tab_widget = TabWidget::new(
    //         self,
    //         ui,
    //         tiles,
    //         tile_id,
    //         &TabState {
    //             active: true,
    //             is_being_dragged: true,
    //             ..Default::default()
    //         },
    //         TabDesc {
    //             label,
    //             icon,
    //             selected: false,
    //             hovered: true,
    //             closeable: false,
    //         },
    //         0.5,
    //     );
    //
    //     let frame = egui::Frame {
    //         inner_margin: egui::Margin::same(0.),
    //         outer_margin: egui::Margin::same(0.),
    //         rounding: egui::Rounding::ZERO,
    //         shadow: Default::default(),
    //         fill: egui::Color32::TRANSPARENT,
    //         stroke: egui::Stroke::NONE,
    //     };
    //
    //     frame.show(ui, |ui| {
    //         tab_widget.paint(ui);
    //     });
    // }

    /// The height of the bar holding tab titles.
    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        re_ui::DesignTokens::title_bar_height()
    }

    // Styling:

    fn dragged_overlay_color(&self, visuals: &egui::Visuals) -> egui::Color32 {
        visuals.panel_fill.gamma_multiply(0.5)
    }

    fn simplification_options(&self) -> SimplificationOptions {
        SimplificationOptions {
            prune_empty_tabs: true,
            prune_single_child_tabs: true,
            prune_empty_containers: true,
            prune_single_child_containers: true,
            all_panes_must_have_tabs: true,
            join_nested_linear_containers: true,
        }
    }

    fn tab_bar_color(&self, visuals: &egui::Visuals) -> egui::Color32 {
        visuals.panel_fill.gamma_multiply(0.5)
    }

    /// When drag-and-dropping a tile, the candidate area is drawn with this stroke.
    fn drag_preview_stroke(&self, _visuals: &egui::Visuals) -> egui::Stroke {
        egui::Stroke::new(1.0, egui::Color32::WHITE.gamma_multiply(0.5))
    }

    /// When drag-and-dropping a tile, the candidate area is drawn with this background color.
    fn drag_preview_color(&self, _visuals: &egui::Visuals) -> egui::Color32 {
        egui::Color32::WHITE.gamma_multiply(0.1)
    }
}
