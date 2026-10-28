//! Prefab mode's editor UI: the banner naming the open prefab, and the prompt
//! about its unsaved changes before it closes or the window does.

use egui::{Align, Button, Color32, Frame, Layout, Margin};

use super::PrefabPrompt;
use crate::EditorAppState;

/// Hover text of the scene file actions prefab mode disables.
pub(crate) const CLOSE_PREFAB_FIRST: &str = "Close the prefab to return to the scene first";
/// Why play is refused while a prefab is open.
pub(crate) const PLAY_IN_PREFAB_MODE: &str =
    "Play is not available in prefab mode; close the prefab to play the scene";
/// The fill of the banner that marks prefab mode.
const PREFAB_MODE_COLOR: Color32 = Color32::from_rgb(28, 74, 120);

/// Shows, while a prefab is open, the banner naming the prefab asset with
/// its save and close actions.
pub(crate) fn show_banner(ctx: &egui::Context, state: &mut EditorAppState) {
    let Some(prefab) = state.prefab_mode.prefab() else {
        return;
    };
    let label = prefab.label().to_owned();
    let dirty = state.is_scene_dirty();
    let root_count = state.game.scenes.current_scene().root_objects().count();
    let mut save = false;
    let mut close = false;
    egui::TopBottomPanel::top("prefab_mode_banner")
        .frame(Frame {
            inner_margin: Margin::symmetric(8, 4),
            fill: PREFAB_MODE_COLOR,
            ..Default::default()
        })
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("PREFAB MODE")
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.separator();
                let dirty_marker = if dirty { " *" } else { "" };
                ui.label(
                    egui::RichText::new(format!("{label}{dirty_marker}"))
                        .monospace()
                        .color(Color32::WHITE),
                );
                if root_count != 1 {
                    ui.label(
                        egui::RichText::new(format!(
                            "A prefab needs exactly one root object ({root_count} now)"
                        ))
                        .color(Color32::from_rgb(255, 200, 80)),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui
                        .button("Close Prefab")
                        .on_hover_text("Return to the scene")
                        .clicked();
                    save = ui
                        .add_enabled(dirty, Button::new("Save Prefab"))
                        .on_hover_text(
                            "Write the prefab asset; scene instances change only through                              Edit > Update Prefab Instances",
                        )
                        .clicked();
                    ui.label(
                        egui::RichText::new("Scene instances update only on request")
                            .italics()
                            .color(Color32::from_gray(200)),
                    );
                });
            });
        });
    if save {
        if let Err(error) = state.prefab_mode_mut().save() {
            log::error!("{error}");
        }
    }
    if close {
        state.prefab_mode_mut().request_close();
    }
}

/// Asks what to do with the open prefab's unsaved changes before the action
/// waiting on the answer: save them, discard them, or cancel. Closing the
/// window waits on it too while the prefab has unsaved changes.
pub(crate) fn show_unsaved_changes_prompt(ctx: &egui::Context, state: &mut EditorAppState) {
    intercept_close_with_unsaved_prefab(ctx, state);
    let mode = &mut state.prefab_mode;
    let (Some(action), Some(prefab)) = (mode.prompt, mode.prefab()) else {
        mode.prompt = None;
        return;
    };
    let label = prefab.label().to_owned();
    enum Answer {
        Save,
        Discard,
        Cancel,
    }
    let mut answer = None;
    egui::Modal::new(egui::Id::new("prefab_unsaved_changes")).show(ctx, |ui| {
        ui.heading("Unsaved Prefab Changes");
        ui.separator();
        ui.label(format!("{label} has unsaved changes."));
        ui.label("Save them to the prefab asset before closing it?");
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                answer = Some(Answer::Save);
            }
            if ui.button("Don't Save").clicked() {
                answer = Some(Answer::Discard);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(Answer::Cancel);
            }
        });
    });
    let Some(answer) = answer else {
        return;
    };
    state.prefab_mode.prompt = None;
    let mut prefab_mode = state.prefab_mode_mut();
    match answer {
        Answer::Cancel => return,
        Answer::Save => {
            if let Err(error) = prefab_mode.save() {
                log::error!("{error}");
                return;
            }
        }
        Answer::Discard => {}
    }
    match action {
        PrefabPrompt::Close => {
            prefab_mode.close();
        }
        PrefabPrompt::Open(asset_id) => {
            prefab_mode.close();
            if let Err(error) = prefab_mode.open(asset_id) {
                log::error!("Failed to open prefab: {error}");
            }
        }
        PrefabPrompt::Quit => {
            state.prefab_mode.confirm_close();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Keeps the window open while the open prefab has unsaved changes, and
/// asks the user about them instead.
fn intercept_close_with_unsaved_prefab(ctx: &egui::Context, state: &mut EditorAppState) {
    let close_requested = ctx.input(|input| input.viewport().close_requested());
    if !close_requested
        || state.prefab_mode.close_confirmed
        || !state.prefab_mode.is_open()
        || !state.is_scene_dirty()
    {
        return;
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
    state.prefab_mode.prompt = Some(PrefabPrompt::Quit);
}
