use std::path::PathBuf;

use engine::component::{ComponentID, ComponentTransform};
use engine::scene::{GameObject, Scene};
use engine::utils::TypeUuid;
use nalgebra::UnitQuaternion;
use nalgebra_glm::Vec3;
use remote_protocol::{
    CaptureTarget, Command, CoordSpace, ErrorKind, InputEventSpec, ModifiersSpec, PickTarget,
    PointerButtonSpec, RequestEnvelope, TransformSpace,
};
use uuid::Uuid;

use super::runtime::{color_image_to_rgba, save_png, PendingOp, RemoteRuntime, ResponseSlot};
use super::server::{parse_error_response, ClientId, Incoming};
use crate::panel::{Panel, PanelGame};
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
                    runtime.pending.retain(|op| op.slot().client != client);
                    runtime
                        .pending_window_shots
                        .retain(|shot| shot.slot.client != client);
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
        let result = self.execute_remote_command(ctx, runtime, &slot, request.command);
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
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                Ok(Some(serde_json::Value::Null))
            }

            Command::Play => {
                self.state.flush_pending_inspector_value_edits();
                self.state.game.scenes.start_simulation();
                Ok(Some(serde_json::Value::Null))
            }
            Command::Pause => {
                self.state.game.scenes.pause_simulation();
                Ok(Some(serde_json::Value::Null))
            }
            Command::Stop => {
                self.state.game.scenes.stop_simulation();
                Ok(Some(serde_json::Value::Null))
            }
            Command::StepFrames { frames } => {
                if frames == 0 {
                    return Err((ErrorKind::BadRequest, "frames must be at least 1".into()));
                }
                self.state.flush_pending_inspector_value_edits();
                self.state.game.scenes.start_simulation();
                runtime.pending.push(PendingOp::StepFrames {
                    slot: ResponseSlot { ..*slot },
                    remaining: frames,
                });
                Ok(None)
            }
            Command::LoadScene { path } => self.remote_load_scene(path),
            Command::SaveScene { path } => self.remote_save_scene(path),

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
                scene.delete(game_object);
                self.state.mark_scene_dirty();
                Ok(Some(serde_json::Value::Null))
            }
            Command::AddComponent { object, component } => {
                let type_uuid = resolve_component_type(&self.state, &component)?;
                let scene = self.state.game.scenes.simulation_scene_mut();
                let game_object = resolve_object(scene, &object)?;
                scene.bind_component_dyn(game_object, type_uuid);
                self.state.mark_scene_dirty();
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
                let done_at_frame = runtime.schedule_events(buckets);
                runtime.pending.push(PendingOp::InputDrain {
                    slot: ResponseSlot { ..*slot },
                    done_at_frame,
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
            "port": runtime.server.bound_port(),
        })))
    }

    fn remote_load_scene(&mut self, path: PathBuf) -> CommandResult {
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
        let file = match path.or_else(|| self.state.game.scenes.current_scene_meta().file.clone()) {
            Some(file) => file,
            None => {
                return Err((
                    ErrorKind::BadRequest,
                    "scene has no file yet; pass an explicit path".into(),
                ))
            }
        };
        if self.state.save_current_scene(file.clone()) {
            Ok(Some(serde_json::json!({
                "scene_file": file.display().to_string(),
            })))
        } else {
            Err((
                ErrorKind::Internal,
                format!("failed to save scene to {}", file.display()),
            ))
        }
    }

    fn remote_list_objects(&self) -> serde_json::Value {
        let scene = self.state.game.scenes.simulation_scene();
        let components = self.state.game.assets.registries.components.read();
        let types = self.state.game.assets.registries.types.read();
        let objects = scene
            .objects()
            .map(|game_object| {
                let component_names = match scene.entry(game_object) {
                    Some(entry) => components
                        .components()
                        .filter(|(_, component)| component.get_instance(&entry).is_some())
                        .map(|(uuid, _)| component_type_name(&types, *uuid))
                        .collect::<Vec<_>>(),
                    None => Vec::new(),
                };
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
        let component_names = match scene.entry(game_object) {
            Some(entry) => components
                .components()
                .filter(|(_, component)| component.get_instance(&entry).is_some())
                .map(|(uuid, _)| component_type_name(&types, *uuid))
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        Ok(Some(serde_json::json!({
            "id": scene.uuid(game_object).to_string(),
            "name": scene.name(game_object),
            "parent": scene.parent(game_object).map(|parent| scene.uuid(parent).to_string()),
            "local_transform": transform_json(&scene.transform(game_object)),
            "world_transform": transform_json(&scene.world_transform(game_object)),
            "components": component_names,
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
        let entry = scene.entry(game_object).ok_or_else(|| {
            (
                ErrorKind::NotFound,
                "game object entry disappeared".to_string(),
            )
        })?;
        let instance = prototype.get_instance(&entry).ok_or_else(|| {
            (
                ErrorKind::NotFound,
                format!("object has no {component} component"),
            )
        })?;
        instance
            .serialize()
            .map(Some)
            .ok_or_else(|| (ErrorKind::Internal, "component is not serializable".into()))
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
        let Some(instance) = (unsafe {
            scene
                .get_component_ptr(game_object, prototype)
                .map(|ptr| &mut *ptr)
        }) else {
            return Err((
                ErrorKind::NotFound,
                format!("object has no {component} component"),
            ));
        };
        if !instance.deserialize_in_place(value) {
            return Err((
                ErrorKind::BadRequest,
                "value does not deserialize into the component type".into(),
            ));
        }
        if type_uuid == ComponentTransform::type_uuid() {
            scene.clear_transform_cache();
        }
        self.state.mark_scene_dirty();
        Ok(Some(serde_json::Value::Null))
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
        self.state.mark_scene_dirty();
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
        self.state.mark_scene_dirty();
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

fn rect_json(rect: egui::Rect) -> serde_json::Value {
    serde_json::json!({
        "min": [rect.min.x, rect.min.y],
        "max": [rect.max.x, rect.max.y],
        "size": [rect.width(), rect.height()],
    })
}

fn transform_json(transform: &engine::math::Transform) -> serde_json::Value {
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

fn component_type_name(types: &engine::reflect::type_registry::TypeRegistry, uuid: Uuid) -> String {
    use engine::reflect::TypeInfo;
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

    fn bucket(buckets: &mut Vec<Vec<egui::Event>>, index: usize) -> &mut Vec<egui::Event> {
        if buckets.len() <= index {
            buckets.resize_with(index + 1, Vec::new);
        }
        &mut buckets[index]
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
                bucket(&mut buckets, cursor).push(key_event(key, true, modifiers));
            }
            InputEventSpec::KeyUp { key, modifiers } => {
                let key = parse_key(key)?;
                bucket(&mut buckets, cursor).push(key_event(key, false, modifiers));
            }
            InputEventSpec::KeyPress { key, hold_frames } => {
                let key = parse_key(key)?;
                let hold = hold_frames.unwrap_or(1).max(1) as usize;
                bucket(&mut buckets, cursor).push(key_event(key, true, &None));
                bucket(&mut buckets, cursor + hold).push(key_event(key, false, &None));
            }
            InputEventSpec::Text { text } => {
                bucket(&mut buckets, cursor).push(egui::Event::Text(text.clone()));
            }
            InputEventSpec::PointerMove { x, y, space } => {
                let pos = resolve_pos(*x, *y, *space)?;
                bucket(&mut buckets, cursor).push(egui::Event::PointerMoved(pos));
            }
            InputEventSpec::PointerDown {
                x,
                y,
                button,
                space,
            } => {
                let pos = resolve_pos(*x, *y, *space)?;
                let events = bucket(&mut buckets, cursor);
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
                let events = bucket(&mut buckets, cursor);
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
                let events = bucket(&mut buckets, cursor);
                events.push(egui::Event::PointerMoved(pos));
                events.push(pointer_button_event(pos, *button, true));
                bucket(&mut buckets, cursor + 1).push(pointer_button_event(pos, *button, false));
            }
            InputEventSpec::Scroll { dx, dy } => {
                bucket(&mut buckets, cursor).push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(*dx, *dy),
                    modifiers: egui::Modifiers::default(),
                });
            }
            InputEventSpec::Wait { frames } => {
                cursor += *frames as usize;
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
