use std::path::PathBuf;

use calyx_engine::class_registry::ComponentRegistry;
use calyx_engine::component::{Component, ComponentID};
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::TypeUuid;
use calyx_remote_protocol::{
    CaptureTarget, Command, CoordSpace, ErrorKind, InputEventSpec, ModifiersSpec, PickTarget,
    PointerButtonSpec, RequestEnvelope, TransformSpace, UnsavedSceneAnswer,
};
use nalgebra::UnitQuaternion;
use nalgebra_glm::Vec3;
use uuid::Uuid;

use super::runtime::{color_image_to_rgba, save_png, PendingOp, RemoteRuntime, ResponseSlot};
use super::server::{parse_error_response, ClientId, Incoming};
use crate::asset_open::{AssetOpenOutcome, SceneSwitchAnswer};
use crate::panel::{Panel, PanelGame};
use crate::prefab_instances;
use crate::scene_edit::SceneEdit;
use crate::selection::{Selection, SelectionType};
use crate::{EditorApp, EditorAppState};

type CommandResult = Result<Option<serde_json::Value>, (ErrorKind, String)>;

impl EditorApp {
    /// Drains and executes remote requests. Called at the top of every editor
    /// frame, before panels run, so mutations are visible in the same frame.
    pub(crate) fn remote_update_begin(&mut self, ctx: &egui::Context) {
        let Some(mut runtime) = self.remote.take() else {
            return;
        };
        for message in runtime.server.drain() {
            match message {
                Incoming::Request(client, request) => {
                    self.handle_remote_command(ctx, &mut runtime, client, request);
                }
                Incoming::ParseError(client, message) => {
                    runtime
                        .server
                        .respond(client, &parse_error_response(message));
                }
                Incoming::Disconnected(client) => {
                    if runtime.drop_client(client) {
                        // `step_frames` started the simulation and only the
                        // op's completion pauses it again; dropping it
                        // silently would leave the editor simulating forever.
                        self.state.game.scenes.pause_simulation();
                    }
                }
            }
        }
        self.remote = Some(runtime);
    }

    /// Advances the remote frame counter and completes deferred operations.
    /// Called at the end of every editor frame, after the simulation update.
    /// `simulated` reports whether the scene simulation ran this frame.
    pub(crate) fn remote_update_end(&mut self, simulated: bool) {
        let Some(mut runtime) = self.remote.take() else {
            return;
        };
        runtime.frame_index += 1;
        if let Some(response) = &self.state.game_response {
            runtime.last_game_rect = Some(response.rect);
        }
        if let Some(response) = &self.state.viewport_response {
            runtime.last_viewport_rect = Some(response.rect);
        }
        runtime.cancel_steps_unless_running(self.state.game.scenes.is_simulating());
        let render_context = self.state.game.assets.render_context.clone();
        if runtime.poll_pending(&render_context, simulated) {
            self.state.game.scenes.pause_simulation();
        }
        self.remote = Some(runtime);
    }

    /// Injects scheduled synthetic events into the frame's raw input and
    /// harvests completed window screenshots.
    pub(crate) fn remote_raw_input_hook(&mut self, raw_input: &mut egui::RawInput) {
        let Some(runtime) = &mut self.remote else {
            return;
        };
        let events = runtime.take_scheduled_events();
        if !events.is_empty() {
            raw_input.events.extend(events);
        }

        if runtime.pending_window_shots.is_empty() {
            return;
        }
        for event in &raw_input.events {
            let egui::Event::Screenshot {
                user_data, image, ..
            } = event
            else {
                continue;
            };
            let Some(&(client, request_id)) = user_data
                .data
                .as_ref()
                .and_then(|data| data.downcast_ref::<(u64, u64)>())
            else {
                continue;
            };
            let Some(index) = runtime
                .pending_window_shots
                .iter()
                .position(|shot| shot.slot.client == client && shot.slot.request_id == request_id)
            else {
                continue;
            };
            let shot = runtime.pending_window_shots.swap_remove(index);
            let result = match color_image_to_rgba(image) {
                Some(rgba) => save_png(&shot.path, rgba),
                None => Err((
                    ErrorKind::Internal,
                    "window screenshot had a malformed image".to_string(),
                )),
            };
            match result {
                Ok(data) => runtime.respond_ok(&shot.slot, data),
                Err((kind, message)) => runtime.respond_error(&shot.slot, kind, message),
            }
        }
    }

    fn handle_remote_command(
        &mut self,
        ctx: &egui::Context,
        runtime: &mut RemoteRuntime,
        client: ClientId,
        request: RequestEnvelope,
    ) {
        let slot = ResponseSlot {
            client,
            request_id: request.id,
        };
        log::debug!("Remote command from client {client}: {:?}", request.command);
        // Remote edits go through the undo history like the panels' edits;
        // otherwise the next undo would restore a snapshot from before them.
        // A command writes the scene wherever it needs to, so the edit
        // captures it up front.
        let scene_edit = scene_edit_label(&request.command).map(|label| {
            self.state.flush_pending_inspector_value_edits();
            let mut edit = SceneEdit::begin(label);
            edit.capture(&self.state.game);
            edit
        });
        let result = self.execute_remote_command(ctx, runtime, &slot, request.command);
        if let Some(edit) = scene_edit {
            match result {
                Ok(_) => self.state.commit_scene_edit(edit),
                Err(_) => edit.cancel(),
            }
        }
        match result {
            Ok(Some(data)) => runtime.respond_ok(&slot, data),
            Ok(None) => {} // Deferred; a pending op owns the response now.
            Err((kind, message)) => runtime.respond_error(&slot, kind, message),
        }
    }

    fn execute_remote_command(
        &mut self,
        ctx: &egui::Context,
        runtime: &mut RemoteRuntime,
        slot: &ResponseSlot,
        command: Command,
    ) -> CommandResult {
        match command {
            Command::Ping => Ok(Some(serde_json::Value::Null)),
            Command::Info => self.remote_info(ctx, runtime),
            Command::Shutdown => {
                // A scripted shutdown does not wait on the unsaved-prefab
                // prompt.
                self.state.prefab_mode.confirm_close();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                Ok(Some(serde_json::Value::Null))
            }

            Command::Play { scene } => {
                let scene = scene.map(|path| self.remote_play_scene(path)).transpose()?;
                self.start_simulation_of(scene)
                    .map_err(|error| (ErrorKind::Internal, error))?;
                Ok(Some(serde_json::Value::Null))
            }
            Command::Pause => {
                runtime.cancel_pending_steps("simulation was paused before the step finished");
                self.state.game.scenes.pause_simulation();
                Ok(Some(serde_json::Value::Null))
            }
            Command::Stop => {
                runtime.cancel_pending_steps("simulation was stopped before the step finished");
                self.state.game.scenes.stop_simulation();
                Ok(Some(serde_json::Value::Null))
            }
            Command::StepFrames { frames } => {
                if frames == 0 {
                    return Err((ErrorKind::BadRequest, "frames must be at least 1".into()));
                }
                if runtime.has_pending_step() {
                    return Err((
                        ErrorKind::BadRequest,
                        "a step_frames operation is already in progress".into(),
                    ));
                }
                self.start_simulation()
                    .map_err(|error| (ErrorKind::Internal, error))?;
                runtime.pending.push(PendingOp::StepFrames {
                    slot: ResponseSlot { ..*slot },
                    remaining: frames,
                });
                Ok(None)
            }
            Command::LoadScene { path } => {
                let loaded = self.remote_load_scene(path)?;
                // Only a load that went through stops the simulation; a failed
                // one leaves the step counting down.
                runtime.cancel_pending_steps("a scene was loaded before the step finished");
                Ok(loaded)
            }
            Command::SaveScene { path } => self.remote_save_scene(path),
            Command::OpenPrefab { path } => self.remote_open_prefab(path),
            Command::SavePrefab => {
                self.state
                    .prefab_mode_mut()
                    .save()
                    .map_err(|error| (ErrorKind::BadRequest, error))?;
                Ok(Some(self.remote_prefab_state()))
            }
            Command::ClosePrefab { discard } => {
                if !self.state.prefab_mode.is_open() {
                    return Err((ErrorKind::BadRequest, "no prefab is open".into()));
                }
                if !discard && self.state.is_scene_dirty() {
                    return Err((
                        ErrorKind::BadRequest,
                        "the prefab has unsaved changes; save_prefab first or pass discard".into(),
                    ));
                }
                self.state.prefab_mode_mut().close();
                Ok(Some(self.remote_prefab_state()))
            }
            Command::InstantiatePrefab { path } => {
                let asset_id = self.remote_prefab_asset(path)?;
                self.state.flush_pending_inspector_value_edits();
                let mut edit = SceneEdit::begin("Remote instantiate prefab");
                match prefab_instances::instantiate_prefab(
                    &mut edit,
                    &mut self.state.game,
                    asset_id,
                ) {
                    Ok(root) => {
                        self.state.commit_scene_edit(edit);
                        Ok(Some(serde_json::json!({ "id": root.to_string() })))
                    }
                    Err(error) => {
                        edit.cancel();
                        Err((ErrorKind::BadRequest, error))
                    }
                }
            }
            Command::UpdatePrefabInstances { path } => {
                let asset_id = path
                    .map(|path| self.remote_prefab_asset(path))
                    .transpose()?;
                let update = self
                    .state
                    .update_prefab_instances(asset_id)
                    .map_err(|error| (ErrorKind::BadRequest, error))?;
                Ok(Some(serde_json::json!({
                    "instances": update.instances,
                    "added": update.added,
                    "removed": update.removed,
                    "unlinked": update.unlinked,
                    "kept_overrides": update.kept_overrides,
                    "dropped_components": update.dropped.len(),
                })))
            }

            Command::OpenAsset {
                path,
                unsaved_scene,
            } => self.remote_open_asset(path, unsaved_scene),

            Command::GetSceneState => {
                let scene = self.state.game.scenes.simulation_scene();
                serde_json::to_value(scene)
                    .map(Some)
                    .map_err(|error| (ErrorKind::Internal, error.to_string()))
            }
            Command::ListObjects => Ok(Some(self.remote_list_objects())),
            Command::GetObject { object } => self.remote_get_object(&object),
            Command::GetComponent { object, component } => {
                self.remote_get_component(&object, &component)
            }
            Command::ListComponentTypes => Ok(Some(self.remote_list_component_types())),
            Command::GetComponentEnabled { object, component } => {
                self.remote_get_component_enabled(&object, &component)
            }
            Command::GetSelection => {
                let selection = &self.state.selection;
                Ok(Some(serde_json::json!({
                    "type": format!("{:?}", selection.ty()),
                    "objects": selection.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
                })))
            }
            Command::QueryPanels => Ok(Some(self.remote_query_panels(ctx, runtime))),

            Command::SetComponent {
                object,
                component,
                value,
            } => self.remote_set_component(&object, &component, &value),
            Command::SetComponentEnabled {
                object,
                component,
                enabled,
            } => self.remote_set_component_enabled(&object, &component, enabled),
            Command::SetTransform {
                object,
                position,
                rotation_euler_deg,
                scale,
                space,
            } => self.remote_set_transform(&object, position, rotation_euler_deg, scale, space),
            Command::CreateObject { name, parent } => self.remote_create_object(name, parent),
            Command::DeleteObject { object } => {
                let scene = self.state.game.scenes.simulation_scene_mut();
                let game_object = resolve_object(scene, &object)?;
                let removed = delete_now(scene, game_object);
                // Mirrors the scene-hierarchy delete path: a selection pointing
                // at a removed object would keep the inspector and
                // `get_selection` reporting an object that no longer exists.
                if self.state.selection.is(SelectionType::GameObject)
                    && self
                        .state
                        .selection
                        .iter()
                        .any(|selected| removed.contains(&selected))
                {
                    self.state.selection = Selection::none();
                }
                Ok(Some(serde_json::Value::Null))
            }
            Command::AddComponent { object, component } => {
                let type_uuid = resolve_component_type(&self.state, &component)?;
                if type_uuid == ComponentID::type_uuid() {
                    return Err((
                        ErrorKind::BadRequest,
                        "ComponentID is managed by the scene and cannot be added".into(),
                    ));
                }
                let registry_ref = self.state.game.assets.registries.components.clone();
                let registry = registry_ref.read();
                let prototype = registry.component(type_uuid).ok_or_else(|| {
                    (
                        ErrorKind::NotFound,
                        format!("unknown component {type_uuid}"),
                    )
                })?;
                let scene = self.state.game.scenes.simulation_scene_mut();
                let game_object = resolve_object(scene, &object)?;
                // Rebinding an existing component silently resets its state, so
                // require an explicit `set_component` instead.
                let present = scene
                    .read_component_dyn(game_object, prototype, |_| ())
                    .is_some();
                if present {
                    return Err((
                        ErrorKind::BadRequest,
                        format!(
                            "{} already has component {}",
                            scene.name(game_object),
                            component
                        ),
                    ));
                }
                scene.bind_component_dyn(game_object, type_uuid);
                drop(registry);
                Ok(Some(serde_json::Value::Null))
            }
            Command::Select { objects } => {
                let scene = self.state.game.scenes.simulation_scene();
                let ids = objects
                    .iter()
                    .map(|selector| {
                        resolve_object(scene, selector).map(|game_object| scene.uuid(game_object))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.state.selection =
                    Selection::from_iter(SelectionType::GameObject, ids.into_iter());
                Ok(Some(serde_json::Value::Null))
            }

            Command::Pick {
                x,
                y,
                space,
                target,
            } => self.remote_pick(runtime, x, y, space, target),
            Command::FocusGame { grab } => {
                self.tree.make_active(|_, tile| {
                    matches!(tile, egui_tiles::Tile::Pane(pane) if *pane == PanelGame::name())
                });
                match self.panels.panel_mut::<PanelGame>() {
                    Some(panel) => panel.is_cursor_grabbed = grab,
                    None => return Err((ErrorKind::Internal, "game panel not found".into())),
                }
                Ok(Some(serde_json::Value::Null))
            }

            Command::InjectInput { events } => {
                let buckets = compile_input_events(&events, runtime)?;
                let done_at_hook = runtime.schedule_events(buckets);
                runtime.pending.push(PendingOp::InputDrain {
                    slot: ResponseSlot { ..*slot },
                    done_at_hook,
                });
                Ok(None)
            }

            Command::Screenshot { target, path } => {
                self.remote_screenshot(ctx, runtime, slot, target, path)
            }
            Command::WaitFrames { frames } => {
                runtime.pending.push(PendingOp::WaitFrames {
                    slot: ResponseSlot { ..*slot },
                    done_at_frame: runtime.frame_index + frames.max(1) as u64,
                });
                Ok(None)
            }
        }
    }

    fn remote_info(&self, ctx: &egui::Context, runtime: &RemoteRuntime) -> CommandResult {
        let assemblies_loaded = self.project_manager.read().assemblies_loaded();
        let (keys_down, pointer_pos) = ctx.input(|input| {
            (
                input
                    .keys_down
                    .iter()
                    .map(|key| key.name().to_string())
                    .collect::<Vec<_>>(),
                input.pointer.latest_pos().map(|pos| [pos.x, pos.y]),
            )
        });
        let scenes = &self.state.game.scenes;
        Ok(Some(serde_json::json!({
            "input_debug": {
                "game_focused": self.is_game_focused(),
                "keys_down": keys_down,
                "pointer_pos": pointer_pos,
                "game_rect_known": runtime.last_game_rect.is_some(),
            },
            "version": env!("CARGO_PKG_VERSION"),
            "project_path": runtime.project_path.display().to_string(),
            "assemblies_loaded": assemblies_loaded,
            "is_simulating": scenes.is_simulating(),
            "has_simulation_scene": scenes.has_simulation_scene(),
            "frame_index": runtime.frame_index,
            "scene_file": scenes.current_scene_meta().file.as_ref().map(|f| f.display().to_string()),
            "scene_name": scenes.current_scene_meta().asset_name,
            "object_count": scenes.simulation_scene().objects().count(),
            "prefab": self.remote_prefab_state(),
            "port": runtime.server.bound_port(),
        })))
    }

    /// The open prefab's asset path, file and dirty state, or `null` outside
    /// prefab mode.
    fn remote_prefab_state(&self) -> serde_json::Value {
        match self.state.prefab_mode.prefab() {
            Some(prefab) => serde_json::json!({
                "asset_path": prefab.label(),
                "file": prefab.file().display().to_string(),
                "dirty": self.state.is_scene_dirty(),
            }),
            None => serde_json::Value::Null,
        }
    }

    /// The asset id of the prefab file at `path`.
    fn remote_prefab_asset(&self, path: PathBuf) -> Result<Uuid, (ErrorKind, String)> {
        let path = dunce::canonicalize(&path).map_err(|error| {
            (
                ErrorKind::NotFound,
                format!("prefab file {}: {error}", path.display()),
            )
        })?;
        self.state
            .game
            .assets
            .registries
            .assets
            .read()
            .asset_id_from_path(&path)
            .ok_or_else(|| {
                (
                    ErrorKind::NotFound,
                    format!("{} is not a registered asset", path.display()),
                )
            })
    }

    fn remote_open_prefab(&mut self, path: PathBuf) -> CommandResult {
        let asset_id = self.remote_prefab_asset(path)?;
        self.state
            .prefab_mode_mut()
            .open(asset_id)
            .map_err(|error| (ErrorKind::BadRequest, error))?;
        Ok(Some(self.remote_prefab_state()))
    }

    /// Opens the asset file at `path` through the open-asset dispatch, as a
    /// double-click in the content browser does, answering the unsaved-scene
    /// prompt with `unsaved_scene` when the open raises it.
    fn remote_open_asset(
        &mut self,
        path: PathBuf,
        unsaved_scene: Option<UnsavedSceneAnswer>,
    ) -> CommandResult {
        let path = dunce::canonicalize(&path).map_err(|error| {
            (
                ErrorKind::NotFound,
                format!("asset file {}: {error}", path.display()),
            )
        })?;
        let asset_id = self
            .state
            .game
            .assets
            .registries
            .assets
            .read()
            .asset_id_from_path(&path)
            .ok_or_else(|| {
                (
                    ErrorKind::NotFound,
                    format!("{} is not a registered asset", path.display()),
                )
            })?;
        let mut outcome = self.state.open_asset(asset_id);
        if let (AssetOpenOutcome::Prompted, Some(answer)) = (&outcome, unsaved_scene) {
            if self.state.scene_switch.pending().is_some() {
                let answer = match answer {
                    UnsavedSceneAnswer::Save => SceneSwitchAnswer::Save,
                    UnsavedSceneAnswer::Discard => SceneSwitchAnswer::Discard,
                    UnsavedSceneAnswer::Cancel => SceneSwitchAnswer::Cancel,
                };
                // A remote client has no file picker to answer Save As with.
                outcome = self.state.answer_scene_prompt(answer, || None);
            }
        }
        let outcome = match outcome {
            AssetOpenOutcome::Opened => "opened",
            AssetOpenOutcome::Selected => "selected",
            AssetOpenOutcome::Prompted => "prompted",
            AssetOpenOutcome::Refused(reason) => return Err((ErrorKind::BadRequest, reason)),
        };
        let scenes = &self.state.game.scenes;
        Ok(Some(serde_json::json!({
            "outcome": outcome,
            "asset_id": asset_id.to_string(),
            "selection": self.state.selection.first(SelectionType::Asset).map(|id| id.to_string()),
            "scene_file": scenes.current_scene_meta().file.as_ref().map(|f| f.display().to_string()),
            "scene_dirty": self.state.is_scene_dirty(),
            "prefab": self.remote_prefab_state(),
        })))
    }

    /// Resolves the scene a `play` names, refusing it while a simulation
    /// scene exists, since a resumed simulation keeps the scene it runs.
    fn remote_play_scene(&self, path: PathBuf) -> Result<PathBuf, (ErrorKind, String)> {
        if self.state.game.scenes.has_simulation_scene() {
            return Err((
                ErrorKind::BadRequest,
                "a simulation is running or paused; stop it before playing another scene".into(),
            ));
        }
        // The asset registry keys assets by canonical path.
        dunce::canonicalize(&path).map_err(|error| {
            (
                ErrorKind::NotFound,
                format!("scene file {}: {error}", path.display()),
            )
        })
    }

    fn remote_load_scene(&mut self, path: PathBuf) -> CommandResult {
        if self.state.prefab_mode.is_open() {
            return Err((
                ErrorKind::BadRequest,
                "a prefab is open; close_prefab before loading a scene".into(),
            ));
        }
        // The asset registry keys assets by canonical path, so normalize
        // whatever form the client sent.
        let path = dunce::canonicalize(&path).map_err(|error| {
            (
                ErrorKind::NotFound,
                format!("scene file {}: {error}", path.display()),
            )
        })?;
        if !self.state.open_scene_file(path) {
            return Err((
                ErrorKind::BadRequest,
                "scene did not load; see the editor log for details".into(),
            ));
        }
        let scenes = &self.state.game.scenes;
        Ok(Some(serde_json::json!({
            "scene_file": scenes.current_scene_meta().file.as_ref().map(|f| f.display().to_string()),
            "object_count": scenes.current_scene().objects().count(),
        })))
    }

    fn remote_save_scene(&mut self, path: Option<PathBuf>) -> CommandResult {
        if self.state.prefab_mode.is_open() {
            return Err((
                ErrorKind::BadRequest,
                "a prefab is open; use save_prefab, or close_prefab to return to the scene".into(),
            ));
        }
        let file = match path.or_else(|| self.state.game.scenes.current_scene_meta().file.clone()) {
            Some(file) => file,
            None => {
                return Err((
                    ErrorKind::BadRequest,
                    "scene has no file yet; pass an explicit path".into(),
                ))
            }
        };
        match self.state.save_current_scene(file.clone()) {
            Ok(()) => Ok(Some(serde_json::json!({
                "scene_file": file.display().to_string(),
            }))),
            Err(error) => Err((
                ErrorKind::Internal,
                format!("failed to save scene to {}: {error}", file.display()),
            )),
        }
    }

    fn remote_list_objects(&self) -> serde_json::Value {
        let scene = self.state.game.scenes.simulation_scene();
        let components = self.state.game.assets.registries.components.read();
        let types = self.state.game.assets.registries.types.read();
        let objects = scene
            .objects()
            .map(|game_object| {
                let component_names = object_component_names(scene, &components, &types, game_object);
                serde_json::json!({
                    "id": scene.uuid(game_object).to_string(),
                    "name": scene.name(game_object),
                    "parent": scene.parent(game_object).map(|parent| scene.uuid(parent).to_string()),
                    "components": component_names,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({ "objects": objects })
    }

    fn remote_get_object(&self, selector: &str) -> CommandResult {
        let scene = self.state.game.scenes.simulation_scene();
        let game_object = resolve_object(scene, selector)?;
        let components = self.state.game.assets.registries.components.read();
        let types = self.state.game.assets.registries.types.read();
        let component_names = object_component_names(scene, &components, &types, game_object);
        Ok(Some(serde_json::json!({
            "id": scene.uuid(game_object).to_string(),
            "name": scene.name(game_object),
            "parent": scene.parent(game_object).map(|parent| scene.uuid(parent).to_string()),
            "local_transform": transform_json(&scene.transform(game_object)),
            "world_transform": transform_json(&scene.world_transform(game_object)),
            "components": component_names,
            "disabled_components": disabled_component_names(scene, &components, &types, game_object),
            "prefab_link": scene.prefab_link(game_object).map(|link| serde_json::json!({
                "prefab": link.prefab.to_string(),
                "object": link.object.to_string(),
            })),
        })))
    }

    fn remote_get_component(&self, object: &str, component: &str) -> CommandResult {
        let type_uuid = resolve_component_type(&self.state, component)?;
        let scene = self.state.game.scenes.simulation_scene();
        let game_object = resolve_object(scene, object)?;
        let registry = self.state.game.assets.registries.components.read();
        let prototype = registry.component(type_uuid).ok_or_else(|| {
            (
                ErrorKind::NotFound,
                format!("unknown component {type_uuid}"),
            )
        })?;
        let serialized = scene
            .read_component_dyn(game_object, prototype, |instance| instance.serialize())
            .ok_or_else(|| {
                (
                    ErrorKind::NotFound,
                    format!("object has no {component} component"),
                )
            })?;
        serialized
            .map(Some)
            .ok_or_else(|| (ErrorKind::Internal, "component is not serializable".into()))
    }

    fn remote_get_component_enabled(&self, object: &str, component: &str) -> CommandResult {
        let type_uuid = resolve_component_type(&self.state, component)?;
        let scene = self.state.game.scenes.simulation_scene();
        let game_object = resolve_object(scene, object)?;
        let registry = self.state.game.assets.registries.components.read();
        let prototype = registry.component(type_uuid).ok_or_else(|| {
            (
                ErrorKind::NotFound,
                format!("unknown component {type_uuid}"),
            )
        })?;
        if scene
            .read_component_dyn(game_object, prototype, |_| ())
            .is_none()
        {
            return Err((
                ErrorKind::NotFound,
                format!("object has no {component} component"),
            ));
        }
        Ok(Some(serde_json::json!({
            "enabled": scene.is_component_enabled_dyn(game_object, prototype),
        })))
    }

    fn remote_set_component_enabled(
        &mut self,
        object: &str,
        component: &str,
        enabled: bool,
    ) -> CommandResult {
        let type_uuid = resolve_component_type(&self.state, component)?;
        ensure_can_disable(
            &self.state.game.assets.registries.components.read(),
            type_uuid,
            component,
        )?;
        let scene = self.state.game.scenes.simulation_scene_mut();
        let game_object = resolve_object(scene, object)?;
        if !scene.set_component_enabled_dyn(game_object, type_uuid, enabled) {
            return Err((
                ErrorKind::NotFound,
                format!("object has no {component} component"),
            ));
        }
        Ok(Some(serde_json::Value::Null))
    }

    fn remote_list_component_types(&self) -> serde_json::Value {
        let components = self.state.game.assets.registries.components.read();
        let types = self.state.game.assets.registries.types.read();
        let list = components
            .components()
            .map(|(uuid, _)| {
                serde_json::json!({
                    "uuid": uuid.to_string(),
                    "name": component_type_name(&types, *uuid),
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({ "component_types": list })
    }

    fn remote_set_component(
        &mut self,
        object: &str,
        component: &str,
        value: &serde_json::Value,
    ) -> CommandResult {
        let type_uuid = resolve_component_type(&self.state, component)?;
        let registry_ref = self.state.game.assets.registries.components.clone();
        let registry = registry_ref.read();
        let prototype = registry.component(type_uuid).ok_or_else(|| {
            (
                ErrorKind::NotFound,
                format!("unknown component {type_uuid}"),
            )
        })?;
        let scene = self.state.game.scenes.simulation_scene_mut();
        let game_object = resolve_object(scene, object)?;

        set_component_value(scene, game_object, type_uuid, prototype, value)
    }

    fn remote_set_transform(
        &mut self,
        object: &str,
        position: Option<[f32; 3]>,
        rotation_euler_deg: Option<[f32; 3]>,
        scale: Option<[f32; 3]>,
        space: TransformSpace,
    ) -> CommandResult {
        let scene = self.state.game.scenes.simulation_scene_mut();
        let game_object = resolve_object(scene, object)?;
        let mut transform = match space {
            TransformSpace::Local => scene.transform(game_object),
            TransformSpace::World => scene.world_transform(game_object),
        };
        if let Some([x, y, z]) = position {
            transform.position = Vec3::new(x, y, z);
        }
        if let Some([x, y, z]) = rotation_euler_deg {
            transform.rotation =
                UnitQuaternion::from_euler_angles(x.to_radians(), y.to_radians(), z.to_radians());
        }
        if let Some([x, y, z]) = scale {
            transform.scale = Vec3::new(x, y, z);
        }
        match space {
            TransformSpace::Local => scene.set_transform(game_object, &transform.matrix()),
            TransformSpace::World => scene.set_world_transform(game_object, transform.matrix()),
        }
        Ok(Some(transform_json(&transform)))
    }

    fn remote_create_object(&mut self, name: String, parent: Option<String>) -> CommandResult {
        let scene = self.state.game.scenes.simulation_scene_mut();
        let parent = parent
            .map(|selector| resolve_object(scene, &selector))
            .transpose()?;
        let id = ComponentID {
            name,
            ..Default::default()
        };
        let game_object = scene.create(Some(id), parent);
        let uuid = scene.uuid(game_object);
        Ok(Some(serde_json::json!({ "id": uuid.to_string() })))
    }

    fn remote_query_panels(
        &self,
        ctx: &egui::Context,
        runtime: &RemoteRuntime,
    ) -> serde_json::Value {
        let inner_rect = ctx.input(|input| input.viewport().inner_rect);
        serde_json::json!({
            "pixels_per_point": ctx.pixels_per_point(),
            "window_inner_rect": inner_rect.map(rect_json),
            "panels": {
                "Game": runtime.last_game_rect.map(rect_json),
                "Viewport": runtime.last_viewport_rect.map(rect_json),
            },
            "frame_index": runtime.frame_index,
        })
    }

    fn remote_pick(
        &mut self,
        runtime: &RemoteRuntime,
        x: f32,
        y: f32,
        space: CoordSpace,
        target: PickTarget,
    ) -> CommandResult {
        let panel_rect = match target {
            PickTarget::Viewport => runtime.last_viewport_rect,
            PickTarget::Game => runtime.last_game_rect,
        };
        let renderer = match target {
            PickTarget::Viewport => &mut self.state.scene_renderer,
            PickTarget::Game => &mut self.state.game_renderer,
        };
        let (width, height) = renderer.scene_texture_size();
        if width == 0 || height == 0 {
            return Err((ErrorKind::BadRequest, "render target has no size".into()));
        }
        let (norm_x, norm_y) = match space {
            CoordSpace::Window => {
                let Some(rect) = panel_rect else {
                    return Err((
                        ErrorKind::BadRequest,
                        "panel rect unknown; is the tab visible?".into(),
                    ));
                };
                (
                    (x - rect.left()) / rect.width(),
                    (y - rect.top()) / rect.height(),
                )
            }
            CoordSpace::Game | CoordSpace::Viewport => (x, y),
        };
        let pixel_x = (norm_x.clamp(0.0, 0.999_999) * width as f32).floor() as u32;
        let pixel_y = (norm_y.clamp(0.0, 0.999_999) * height as f32).floor() as u32;
        let picked = renderer.pick_game_object(pixel_x, pixel_y);
        let scene = self.state.game.scenes.simulation_scene();
        Ok(Some(serde_json::json!({
            "object": picked.map(|id| id.to_string()),
            "name": picked.and_then(|id| scene.find(id)).map(|go| scene.name(go)),
            "pixel": [pixel_x, pixel_y],
        })))
    }

    fn remote_screenshot(
        &mut self,
        ctx: &egui::Context,
        runtime: &mut RemoteRuntime,
        slot: &ResponseSlot,
        target: CaptureTarget,
        path: PathBuf,
    ) -> CommandResult {
        match target {
            CaptureTarget::Window => {
                runtime
                    .pending_window_shots
                    .push(super::runtime::WindowShot {
                        slot: ResponseSlot { ..*slot },
                        path,
                    });
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new((
                    slot.client,
                    slot.request_id,
                ))));
                Ok(None)
            }
            CaptureTarget::Game | CaptureTarget::Viewport => {
                if target == CaptureTarget::Game {
                    self.render_game_view_for_capture(ctx)?;
                }
                let renderer = match target {
                    CaptureTarget::Game => &self.state.game_renderer,
                    _ => &self.state.scene_renderer,
                };
                let readback = renderer.submit_scene_texture_readback().ok_or_else(|| {
                    (
                        ErrorKind::BadRequest,
                        "render target has no size yet".to_string(),
                    )
                })?;
                runtime.push_texture_shot(ResponseSlot { ..*slot }, path, readback);
                Ok(None)
            }
        }
    }
}

impl EditorApp {
    /// Renders the game view now when the Game tab has not been laid out
    /// since the editor started, so a game capture works without the tab.
    /// Once the tab has been shown, the game view is rendered every frame at
    /// its size, whether the tab is active or not.
    fn render_game_view_for_capture(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<(), (ErrorKind, String)> {
        let Some(size) = game_capture_size(
            Self::get_physical_size(ctx, self.state.game_size),
            Self::get_physical_size(ctx, self.state.viewport_size),
            Self::get_physical_size(ctx, (1.0, 1.0)),
        )?
        else {
            return Ok(());
        };
        let scene = self.state.game.scenes.simulation_scene();
        let particles = Some(&self.state.game.particles);
        if !Self::render_game_view(&mut self.state.game_renderer, scene, particles, size) {
            return Err((
                ErrorKind::BadRequest,
                "the scene has no enabled camera to render the game view from".to_string(),
            ));
        }
        Ok(())
    }
}

/// The size to render the game view at for a capture, from the physical
/// sizes of the Game panel, the Scene viewport and the window: `None` when
/// the Game panel has a size, so the game view is already rendered each
/// frame, else the Scene viewport's size, which the Game tab shares by
/// default, or the window's.
fn game_capture_size(
    game_panel: (u32, u32),
    viewport: (u32, u32),
    window: (u32, u32),
) -> Result<Option<(u32, u32)>, (ErrorKind, String)> {
    let renderable = |(width, height): (u32, u32)| width > 1 && height > 1;
    if renderable(game_panel) {
        return Ok(None);
    }
    [viewport, window]
        .into_iter()
        .find(|size| renderable(*size))
        .map(Some)
        .ok_or_else(|| {
            (
                ErrorKind::BadRequest,
                "the game view has no size to render at yet; run `calyx_ctl focus-game`, then `calyx_ctl wait --frames 2`, and capture again"
                    .to_string(),
            )
        })
}

/// Replaces `game_object`'s `prototype` component with `value`. A
/// [`ComponentID`] value must keep the object's id, which the scene keeps
/// read-only.
fn set_component_value(
    scene: &mut Scene,
    game_object: GameObject,
    type_uuid: Uuid,
    prototype: &dyn Component,
    value: &serde_json::Value,
) -> CommandResult {
    // The scene puts the object's id back after any write, so a value that
    // omits the id still keeps it; a value naming another id is a mistake
    // the client hears about.
    let indexed_id = (type_uuid == ComponentID::type_uuid()).then(|| scene.uuid(game_object));
    if let Some(indexed_id) = indexed_id {
        let requested = value.get("id").and_then(serde_json::Value::as_str);
        if let Some(requested) = requested {
            if requested.parse::<Uuid>() != Ok(indexed_id) {
                return Err((
                    ErrorKind::BadRequest,
                    format!("game object ids are immutable; expected {indexed_id}"),
                ));
            }
        }
    }

    let Some(deserialized) = scene.write_component_dyn(game_object, prototype, |instance| {
        instance.deserialize_in_place(value)
    }) else {
        return Err((
            ErrorKind::NotFound,
            format!("object has no {} component", prototype.type_name()),
        ));
    };
    if !deserialized {
        return Err((
            ErrorKind::BadRequest,
            "value does not deserialize into the component type".into(),
        ));
    }
    Ok(Some(serde_json::Value::Null))
}

/// Deletes `game_object` and its descendants immediately and returns their
/// ids. `Scene::delete` only queues the removal, and a snapshot taken before
/// the end-of-frame flush still contains the object, so the undo entry for the
/// command would record no change.
fn delete_now(scene: &mut Scene, game_object: GameObject) -> Vec<Uuid> {
    let removed = scene
        .descendants(game_object)
        .map(|object| scene.uuid(object))
        .collect();
    scene.delete(game_object);
    scene.flush_deletes();
    removed
}

/// Names the undo entry of a command that edits the scene, or returns `None`
/// for one that does not.
fn scene_edit_label(command: &Command) -> Option<&'static str> {
    match command {
        Command::SetComponent { .. } => Some("Remote set component"),
        Command::SetTransform { .. } => Some("Remote set transform"),
        Command::CreateObject { .. } => Some("Remote create game object"),
        Command::DeleteObject { .. } => Some("Remote delete game object"),
        Command::AddComponent { .. } => Some("Remote add component"),
        Command::SetComponentEnabled { enabled: true, .. } => Some("Remote enable component"),
        Command::SetComponentEnabled { enabled: false, .. } => Some("Remote disable component"),
        _ => None,
    }
}

fn rect_json(rect: egui::Rect) -> serde_json::Value {
    serde_json::json!({
        "min": [rect.min.x, rect.min.y],
        "max": [rect.max.x, rect.max.y],
        "size": [rect.width(), rect.height()],
    })
}

fn transform_json(transform: &calyx_engine::math::Transform) -> serde_json::Value {
    let (roll, pitch, yaw) = transform.rotation.euler_angles();
    serde_json::json!({
        "position": [transform.position.x, transform.position.y, transform.position.z],
        "rotation_quat": [
            transform.rotation.i,
            transform.rotation.j,
            transform.rotation.k,
            transform.rotation.w,
        ],
        "rotation_euler_deg": [roll.to_degrees(), pitch.to_degrees(), yaw.to_degrees()],
        "scale": [transform.scale.x, transform.scale.y, transform.scale.z],
    })
}

/// Resolves a game-object selector (UUID or name) against `scene`.
fn resolve_object(scene: &Scene, selector: &str) -> Result<GameObject, (ErrorKind, String)> {
    if let Ok(uuid) = Uuid::parse_str(selector) {
        return scene.find(uuid).ok_or_else(|| {
            (
                ErrorKind::NotFound,
                format!("no game object with id {uuid}"),
            )
        });
    }
    let mut matches = scene
        .objects()
        .filter(|game_object| scene.name(*game_object) == selector);
    let Some(first) = matches.next() else {
        return Err((
            ErrorKind::NotFound,
            format!("no game object named {selector:?}"),
        ));
    };
    if matches.next().is_some() {
        return Err((
            ErrorKind::Ambiguous,
            format!("multiple game objects named {selector:?}; use a UUID"),
        ));
    }
    Ok(first)
}

/// Resolves a component-type selector (UUID or type name) against the
/// component registry.
fn resolve_component_type(
    state: &EditorAppState,
    selector: &str,
) -> Result<Uuid, (ErrorKind, String)> {
    let components = state.game.assets.registries.components.read();
    if let Ok(uuid) = Uuid::parse_str(selector) {
        if components.component(uuid).is_some() {
            return Ok(uuid);
        }
        return Err((
            ErrorKind::NotFound,
            format!("no component type with uuid {uuid}"),
        ));
    }
    let types = state.game.assets.registries.types.read();
    let mut matches = components.components().filter(|(uuid, _)| {
        let name = component_type_name(&types, **uuid);
        name == selector || name.rsplit("::").next() == Some(selector)
    });
    let Some((first, _)) = matches.next() else {
        return Err((
            ErrorKind::NotFound,
            format!("no component type named {selector:?}"),
        ));
    };
    let first = *first;
    if matches.next().is_some() {
        return Err((
            ErrorKind::Ambiguous,
            format!("multiple component types named {selector:?}; use a UUID"),
        ));
    }
    Ok(first)
}

/// Returns the type names of the registered components on `game_object`.
fn object_component_names(
    scene: &Scene,
    components: &calyx_engine::class_registry::ComponentRegistry,
    types: &calyx_engine::reflect::type_registry::TypeRegistry,
    game_object: GameObject,
) -> Vec<String> {
    let component_types = scene.component_types(game_object);
    components
        .components()
        .filter(|(_, component)| component_types.contains(&component.component_type_id()))
        .map(|(uuid, _)| component_type_name(types, *uuid))
        .collect()
}

/// Returns the type names of the registered components `game_object` holds
/// disabled.
fn disabled_component_names(
    scene: &Scene,
    components: &calyx_engine::class_registry::ComponentRegistry,
    types: &calyx_engine::reflect::type_registry::TypeRegistry,
    game_object: GameObject,
) -> Vec<String> {
    let component_types = scene.component_types(game_object);
    components
        .components()
        .filter(|(_, component)| {
            component_types.contains(&component.component_type_id())
                && !scene.is_component_enabled_dyn(game_object, component.as_ref())
        })
        .map(|(uuid, _)| component_type_name(types, *uuid))
        .collect()
}

fn component_type_name(
    types: &calyx_engine::reflect::type_registry::TypeRegistry,
    uuid: Uuid,
) -> String {
    use calyx_engine::reflect::TypeInfo;
    let name = types.type_info_by_id(uuid).and_then(|info| match info {
        TypeInfo::Struct(info) => Some(info.type_name),
        TypeInfo::Enum(info) => Some(info.type_name),
        TypeInfo::List(info) => Some(info.type_name),
        _ => None,
    });
    name.map(str::to_owned).unwrap_or_else(|| uuid.to_string())
}

/// Compiles an input script into per-frame egui event buckets. Bucket `i` is
/// delivered `i + 1` frames from now.
fn compile_input_events(
    events: &[InputEventSpec],
    runtime: &RemoteRuntime,
) -> Result<Vec<Vec<egui::Event>>, (ErrorKind, String)> {
    let mut buckets: Vec<Vec<egui::Event>> = Vec::new();
    let mut cursor = 0usize;

    /// One bucket is allocated per frame the script spans, so an unbounded
    /// wait or hold would allocate billions of vectors on the main thread.
    const MAX_SCHEDULE_FRAMES: usize = 36_000; // ten minutes at 60 fps

    fn too_long(frames: usize) -> (ErrorKind, String) {
        (
            ErrorKind::BadRequest,
            format!("input script spans {frames} frames; the limit is {MAX_SCHEDULE_FRAMES}"),
        )
    }

    fn bucket(
        buckets: &mut Vec<Vec<egui::Event>>,
        index: usize,
    ) -> Result<&mut Vec<egui::Event>, (ErrorKind, String)> {
        if index >= MAX_SCHEDULE_FRAMES {
            return Err(too_long(index.saturating_add(1)));
        }
        if buckets.len() <= index {
            buckets.resize_with(index + 1, Vec::new);
        }
        Ok(&mut buckets[index])
    }

    let resolve_pos = |x: f32,
                       y: f32,
                       space: CoordSpace|
     -> Result<egui::Pos2, (ErrorKind, String)> {
        let rect = match space {
            CoordSpace::Window => return Ok(egui::pos2(x, y)),
            CoordSpace::Game => runtime.last_game_rect,
            CoordSpace::Viewport => runtime.last_viewport_rect,
        };
        let Some(rect) = rect else {
            return Err((
                ErrorKind::BadRequest,
                format!(
                    "{space:?} panel rect unknown; focus the tab (e.g. focus_game) and wait a frame first"
                ),
            ));
        };
        Ok(egui::pos2(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        ))
    };

    for event in events {
        match event {
            InputEventSpec::KeyDown { key, modifiers } => {
                let key = parse_key(key)?;
                bucket(&mut buckets, cursor)?.push(key_event(key, true, modifiers));
            }
            InputEventSpec::KeyUp { key, modifiers } => {
                let key = parse_key(key)?;
                bucket(&mut buckets, cursor)?.push(key_event(key, false, modifiers));
            }
            InputEventSpec::KeyPress { key, hold_frames } => {
                let key = parse_key(key)?;
                let hold = hold_frames.unwrap_or(1).max(1) as usize;
                let release = cursor.saturating_add(hold);
                bucket(&mut buckets, cursor)?.push(key_event(key, true, &None));
                bucket(&mut buckets, release)?.push(key_event(key, false, &None));
            }
            InputEventSpec::Text { text } => {
                bucket(&mut buckets, cursor)?.push(egui::Event::Text(text.clone()));
            }
            InputEventSpec::PointerMove { x, y, space } => {
                let pos = resolve_pos(*x, *y, *space)?;
                bucket(&mut buckets, cursor)?.push(egui::Event::PointerMoved(pos));
            }
            InputEventSpec::PointerDown {
                x,
                y,
                button,
                space,
            } => {
                let pos = resolve_pos(*x, *y, *space)?;
                let events = bucket(&mut buckets, cursor)?;
                events.push(egui::Event::PointerMoved(pos));
                events.push(pointer_button_event(pos, *button, true));
            }
            InputEventSpec::PointerUp {
                x,
                y,
                button,
                space,
            } => {
                let pos = resolve_pos(*x, *y, *space)?;
                let events = bucket(&mut buckets, cursor)?;
                events.push(egui::Event::PointerMoved(pos));
                events.push(pointer_button_event(pos, *button, false));
            }
            InputEventSpec::Click {
                x,
                y,
                button,
                space,
            } => {
                let pos = resolve_pos(*x, *y, *space)?;
                let events = bucket(&mut buckets, cursor)?;
                events.push(egui::Event::PointerMoved(pos));
                events.push(pointer_button_event(pos, *button, true));
                bucket(&mut buckets, cursor.saturating_add(1))?
                    .push(pointer_button_event(pos, *button, false));
            }
            InputEventSpec::Scroll { dx, dy } => {
                bucket(&mut buckets, cursor)?.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(*dx, *dy),
                    modifiers: egui::Modifiers::default(),
                });
            }
            InputEventSpec::Wait { frames } => {
                cursor = cursor.saturating_add(*frames as usize);
                if cursor > MAX_SCHEDULE_FRAMES {
                    return Err(too_long(cursor));
                }
            }
        }
    }

    // A trailing wait should still delay the inject_input response.
    if buckets.len() < cursor {
        buckets.resize_with(cursor, Vec::new);
    }
    Ok(buckets)
}

fn parse_key(name: &str) -> Result<egui::Key, (ErrorKind, String)> {
    egui::Key::from_name(name).ok_or_else(|| {
        (
            ErrorKind::BadRequest,
            format!("unknown key {name:?}; use egui key names like \"W\" or \"Space\""),
        )
    })
}

fn key_event(key: egui::Key, pressed: bool, modifiers: &Option<ModifiersSpec>) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: modifiers.map(egui_modifiers).unwrap_or_default(),
    }
}

fn egui_modifiers(modifiers: ModifiersSpec) -> egui::Modifiers {
    egui::Modifiers {
        alt: modifiers.alt,
        ctrl: modifiers.ctrl,
        shift: modifiers.shift,
        mac_cmd: false,
        command: modifiers.ctrl,
    }
}

fn pointer_button_event(pos: egui::Pos2, button: PointerButtonSpec, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: match button {
            PointerButtonSpec::Primary => egui::PointerButton::Primary,
            PointerButtonSpec::Secondary => egui::PointerButton::Secondary,
            PointerButtonSpec::Middle => egui::PointerButton::Middle,
        },
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}

/// Refuses, as a bad request, a component type `component` (type UUID
/// `type_uuid`) that does not opt in to disabling.
fn ensure_can_disable(
    registry: &ComponentRegistry,
    type_uuid: Uuid,
    component: &str,
) -> Result<(), (ErrorKind, String)> {
    if registry.can_disable(type_uuid) {
        return Ok(());
    }
    Err((
        ErrorKind::BadRequest,
        format!(
            "{component} cannot be disabled: only components whose systems honor the enabled state support it"
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::runtime::tests::test_runtime;
    use calyx_engine::component::{
        ComponentBone, ComponentCamera, ComponentMesh, ComponentPointLight, ComponentTransform,
    };

    #[test]
    fn a_game_capture_renders_only_while_the_game_panel_has_no_size() {
        assert_eq!(
            game_capture_size((800, 600), (1200, 700), (1920, 1080)).unwrap(),
            None
        );
        assert_eq!(
            game_capture_size((0, 0), (1200, 700), (1920, 1080)).unwrap(),
            Some((1200, 700))
        );
        assert_eq!(
            game_capture_size((0, 0), (0, 0), (1920, 1080)).unwrap(),
            Some((1920, 1080))
        );
    }

    #[test]
    fn a_game_capture_with_nothing_to_size_it_names_focus_game() {
        let (kind, message) = game_capture_size((0, 0), (1, 1), (0, 0)).unwrap_err();
        assert_eq!(kind, ErrorKind::BadRequest);
        assert!(message.contains("focus-game"), "{message}");
    }

    #[test]
    fn setting_a_parent_transform_moves_the_child() {
        use nalgebra_glm as glm;
        let registries = calyx_engine::test_support::test_registries();
        let mut scene = registries.scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));
        scene.set_transform(parent, &glm::translation(&Vec3::new(1.0, 0.0, 0.0)));
        scene.set_transform(child, &glm::translation(&Vec3::new(0.0, 1.0, 0.0)));
        assert!((scene.world_transform(child).position - Vec3::new(1.0, 1.0, 0.0)).norm() < 1e-5);

        let component_registry = registries.components.read();
        let prototype = component_registry
            .component(ComponentTransform::type_uuid())
            .expect("transform should be registered");
        let value = serde_json::json!({
            "transform": {
                "position": [5.0, 0.0, 0.0],
                "rotation": [0.0, 0.0, 0.0, 1.0],
                "scale": [1.0, 1.0, 1.0],
            }
        });
        set_component_value(
            &mut scene,
            parent,
            ComponentTransform::type_uuid(),
            prototype,
            &value,
        )
        .expect("the transform should be set");

        let child_world = scene.world_transform(child).position;
        assert!(
            (child_world - Vec3::new(5.0, 1.0, 0.0)).norm() < 1e-5,
            "child world position {child_world:?}"
        );
    }

    #[test]
    fn disabling_a_component_that_cannot_be_disabled_is_a_bad_request() {
        let registries = calyx_engine::test_support::test_registries();
        let components = registries.components.read();
        for (type_uuid, name) in [
            (ComponentCamera::type_uuid(), "camera"),
            (ComponentMesh::type_uuid(), "mesh"),
            (ComponentPointLight::type_uuid(), "point light"),
        ] {
            assert!(ensure_can_disable(&components, type_uuid, name).is_ok());
        }
        for (type_uuid, name) in [
            (ComponentBone::type_uuid(), "bone"),
            (ComponentTransform::type_uuid(), "transform"),
        ] {
            let (kind, message) = ensure_can_disable(&components, type_uuid, name).unwrap_err();
            assert_eq!(kind, ErrorKind::BadRequest);
            assert!(
                message.contains(&format!("{name} cannot be disabled")),
                "{message}"
            );
        }
    }

    #[test]
    fn deleting_takes_effect_before_the_edit_is_snapshotted() {
        let mut scene = calyx_engine::test_support::test_scene();
        let parent = scene.create(None, None);
        let child = scene.create(None, Some(parent));
        let ids = [scene.uuid(parent), scene.uuid(child)];
        let before = crate::scene_history::SceneEditSnapshot::capture(&scene);

        let removed = delete_now(&mut scene, parent);

        let after = crate::scene_history::SceneEditSnapshot::capture(&scene);
        assert_ne!(
            before.fingerprint(),
            after.fingerprint(),
            "the undo entry would record no change"
        );
        assert_eq!(removed, ids);
        assert!(ids.iter().all(|id| scene.find(*id).is_none()));
    }

    #[test]
    fn oversized_waits_are_rejected_instead_of_allocating() {
        let runtime = test_runtime();
        let events = [InputEventSpec::Wait { frames: u32::MAX }];
        let (kind, message) = compile_input_events(&events, &runtime)
            .expect_err("an unbounded wait must not allocate a bucket per frame");
        assert_eq!(kind, ErrorKind::BadRequest);
        assert!(message.contains("frames"), "unexpected message: {message}");
    }

    #[test]
    fn oversized_key_holds_are_rejected() {
        let runtime = test_runtime();
        let events = [InputEventSpec::KeyPress {
            key: "W".to_string(),
            hold_frames: Some(u32::MAX),
        }];
        let (kind, _) = compile_input_events(&events, &runtime)
            .expect_err("an unbounded hold must not allocate a bucket per frame");
        assert_eq!(kind, ErrorKind::BadRequest);
    }

    #[test]
    fn key_press_schedules_the_release_after_the_hold() {
        let runtime = test_runtime();
        let events = [InputEventSpec::KeyPress {
            key: "W".to_string(),
            hold_frames: Some(3),
        }];
        let buckets = compile_input_events(&events, &runtime).expect("script should compile");
        assert_eq!(buckets.len(), 4, "press at 0, release at 3");
        assert_eq!(buckets[0].len(), 1);
        assert_eq!(buckets[3].len(), 1);
        assert!(buckets[1].is_empty() && buckets[2].is_empty());
    }

    #[test]
    fn a_trailing_wait_extends_the_schedule() {
        let runtime = test_runtime();
        let events = [
            InputEventSpec::KeyDown {
                key: "W".to_string(),
                modifiers: None,
            },
            InputEventSpec::Wait { frames: 4 },
        ];
        let buckets = compile_input_events(&events, &runtime).expect("script should compile");
        // The response must not land before the waited-out frames elapse.
        assert_eq!(buckets.len(), 4);
        assert_eq!(buckets[0].len(), 1);
    }
}
