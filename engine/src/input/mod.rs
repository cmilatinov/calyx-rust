use std::collections::HashMap;
use std::io::Error;
use std::path::Path;
use std::sync::Arc;

use crate as engine;
use crate::assets::error::{AssetError, AssetErrorKind};
use crate::assets::{Asset, AssetRegistry, LoadedAsset};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::resource::Resource;
use crate::utils::TypeUuid;
use egui::{Key, Modifiers, PointerButton};
use serde::{Deserialize, Serialize};

/// Name of the project asset (`input.cxinput`) holding the project's action map.
pub const PROJECT_ACTION_MAP_NAME: &str = "input";

/// Persistent input state that carries the action map and pointer history across frames.
#[derive(Clone, Resource, TypeUuid)]
#[uuid = "5b0a8e0f-3c1d-4a59-9d52-7f6b2a1c8e47"]
#[repr(C)]
pub struct InputState {
    /// Pass and pointer position of the last cursor sample, used for cursor deltas.
    last_cursor_sample: Option<CursorSample>,
    /// Action map used to interpret raw egui input.
    action_map: ActionMapSource,
}

/// Cursor movement sampled during one egui pass, shared by every input surface in it.
#[derive(Clone, Copy)]
struct CursorSample {
    pass: u64,
    pos: Option<egui::Pos2>,
    delta: egui::Vec2,
}

/// Where the active action map lives.
#[derive(Clone)]
enum ActionMapSource {
    /// A map owned by the input state.
    Owned(Arc<ActionMap>),
    /// A project asset handle, so hot reloads reach the running input.
    Asset(Ref<ActionMap>),
}

impl ActionMapSource {
    fn with<R>(&self, reader: impl FnOnce(&ActionMap) -> R) -> R {
        match self {
            Self::Owned(map) => reader(map),
            Self::Asset(map) => reader(&map.read()),
        }
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new(ActionMap::default())
    }
}

impl InputState {
    /// Creates input state that interprets input through `action_map`.
    pub fn new(action_map: ActionMap) -> Self {
        Self {
            last_cursor_sample: None,
            action_map: ActionMapSource::Owned(Arc::new(action_map)),
        }
    }

    /// Replaces the active action map, no longer following the project asset.
    pub fn set_action_map(&mut self, action_map: ActionMap) {
        self.action_map = ActionMapSource::Owned(Arc::new(action_map));
    }

    /// Follows the project's action map asset when one exists. A project without
    /// one keeps the current map; a map that fails to load is reported as an error
    /// and also leaves the current map in place. Hot reloads of the asset apply to
    /// later frames.
    pub fn load_project_action_map(&mut self, assets: &AssetRegistry) -> Result<(), AssetError> {
        match assets.load::<ActionMap>(PROJECT_ACTION_MAP_NAME) {
            Ok(action_map) => {
                self.action_map = ActionMapSource::Asset(action_map);
                Ok(())
            }
            Err(error) if error.kind == AssetErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Returns cursor movement for the current egui pass, sampling the pointer once
    /// per pass so several input surfaces in one frame see the same delta.
    fn cursor_sample(&mut self, context: &egui::Context) -> CursorSample {
        let pass = context.cumulative_pass_nr();
        if let Some(sample) = self.last_cursor_sample.filter(|sample| sample.pass == pass) {
            return sample;
        }
        let pos = context.input(|input| input.pointer.latest_pos());
        let delta = match (self.last_cursor_sample.and_then(|s| s.pos), pos) {
            (Some(last), Some(pos)) => pos - last,
            _ => egui::Vec2::ZERO,
        };
        let sample = CursorSample { pass, pos, delta };
        self.last_cursor_sample = Some(sample);
        sample
    }
}

/// Read-only input view for the current frame.
pub struct Input<'a> {
    context: &'a egui::Context,
    res: Option<&'a egui::Response>,
    is_active: bool,
    action_map: ActionMapSource,
    cursor_delta: egui::Vec2,
}

impl<'a> Input<'a> {
    /// Creates a frame input wrapper from egui state, advancing the pointer history
    /// kept in `state`.
    pub fn from_ctx(
        context: &'a egui::Context,
        res: Option<&'a egui::Response>,
        state: &mut InputState,
        is_active: bool,
    ) -> Self {
        let cursor_delta = state.cursor_sample(context).delta;
        Self {
            context,
            res,
            is_active,
            action_map: state.action_map.clone(),
            cursor_delta: if is_active {
                cursor_delta
            } else {
                egui::Vec2::ZERO
            },
        }
    }
    /// Reads egui input when input is active.
    fn read<R: Default>(&self, reader: impl FnOnce(&egui::InputState) -> R) -> R {
        if !self.is_active {
            return R::default();
        }
        self.context.input(reader)
    }

    /// Resolves a named action into a frame-local [`ActionState`].
    pub fn action(&self, name: &str) -> ActionState {
        if !self.is_active {
            return ActionState::default();
        }

        self.context.input(|input| {
            self.action_map
                .with(|map| map.action(name, input).unwrap_or_default())
        })
    }

    /// Resolves a named axis into a scalar value.
    pub fn axis(&self, name: &str) -> f32 {
        if !self.is_active {
            return 0.0;
        }

        self.context.input(|input| {
            self.action_map
                .with(|map| map.axis(name, input).unwrap_or_default())
        })
    }

    /// Returns pointer movement since the previous frame.
    pub fn cursor_delta(&self) -> egui::Vec2 {
        self.cursor_delta
    }

    /// Returns the pointer position used for interaction this frame, while input is
    /// active.
    pub fn pointer_pos(&self) -> Option<egui::Pos2> {
        self.read(|input| input.pointer.interact_pos())
    }

    /// Returns raw pointer motion reported by the device this frame, while input is
    /// active. Unlike [`Self::cursor_delta`], it keeps reporting while the cursor is
    /// confined.
    pub fn pointer_motion(&self) -> egui::Vec2 {
        self.read(|input| input.pointer.motion().unwrap_or_default())
    }

    /// Returns smoothed scroll movement this frame, while input is active.
    pub fn scroll_delta(&self) -> egui::Vec2 {
        self.read(|input| input.smooth_scroll_delta)
    }

    /// Returns how far the pointer dragged this input surface this frame, when
    /// `button` is dragging it and input is active.
    pub fn drag_delta(&self, button: PointerButton) -> Option<egui::Vec2> {
        let res = self.res.filter(|_| self.is_active)?;
        res.dragged_by(button).then(|| res.drag_delta())
    }

    /// Returns the screen rectangle of this input surface, when one is bound.
    pub fn surface_rect(&self) -> Option<egui::Rect> {
        self.res.map(|res| res.rect)
    }
}

/// Asset-backed mapping from action names to bindings and axes. The default map
/// binds nothing: a project ships its bindings as the `input.cxinput` asset.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TypeUuid)]
#[uuid = "ff8bf335-7c0b-4e70-a2f9-4d64d2c2d12d"]
pub struct ActionMap {
    actions: HashMap<String, Vec<InputBinding>>,
    axes: HashMap<String, AxisBinding>,
}

impl Asset for ActionMap {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Action Map"
    }

    fn file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["cxinput"]
    }

    fn from_file(
        _assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, crate::assets::error::AssetError>
    where
        Self: Sized,
    {
        LoadedAsset::<Self>::from_json_file(path)
    }

    fn to_file(&self, path: &Path) -> Result<(), Error> {
        AssetRegistry::write_to_file(self, path)
    }
}

impl ActionMap {
    /// Creates an empty action map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `binding` to the named action.
    pub fn bind_action(&mut self, name: impl Into<String>, binding: InputBinding) {
        self.actions.entry(name.into()).or_default().push(binding);
    }

    /// Replaces the bindings for the named action.
    pub fn set_action_bindings(
        &mut self,
        name: impl Into<String>,
        bindings: impl IntoIterator<Item = InputBinding>,
    ) {
        self.actions
            .insert(name.into(), bindings.into_iter().collect());
    }

    /// Defines a signed axis from two digital bindings.
    pub fn bind_axis(
        &mut self,
        name: impl Into<String>,
        positive: InputBinding,
        negative: InputBinding,
    ) {
        self.axes
            .insert(name.into(), AxisBinding { positive, negative });
    }

    /// Resolves a named action from raw egui input.
    pub fn action(&self, name: &str, input: &egui::InputState) -> Option<ActionState> {
        let bindings = self.actions.get(name)?;
        Some(
            bindings
                .iter()
                .fold(ActionState::default(), |state, binding| {
                    state | binding.action_state(input)
                }),
        )
    }

    /// Resolves a named axis from raw egui input.
    pub fn axis(&self, name: &str, input: &egui::InputState) -> Option<f32> {
        self.axes.get(name).map(|axis| axis.value(input))
    }
}

/// Signed axis backed by positive and negative digital inputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxisBinding {
    /// Binding that contributes `+1`.
    pub positive: InputBinding,
    /// Binding that contributes `-1`.
    pub negative: InputBinding,
}

impl AxisBinding {
    fn value(&self, input: &egui::InputState) -> f32 {
        let positive = self.positive.is_down(input) as u8 as f32;
        let negative = self.negative.is_down(input) as u8 as f32;
        positive - negative
    }
}

/// Discrete input binding backed by a key or pointer button.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputBinding {
    /// Keyboard key with no modifier requirements.
    Key(Key),
    /// Pointer button with no modifier requirements.
    PointerButton(PointerButton),
    /// Keyboard key gated by modifier requirements.
    ModifiedKey {
        /// Trigger key.
        key: Key,
        /// Modifier requirements.
        modifiers: ModifierBinding,
    },
    /// Pointer button gated by modifier requirements.
    ModifiedPointerButton {
        /// Trigger button.
        button: PointerButton,
        /// Modifier requirements.
        modifiers: ModifierBinding,
    },
    /// Modifier keys held on their own; down while every given modifier is held.
    /// Egui reports no edges for modifiers, so the action is never just pressed or
    /// just released.
    Modifier(Modifiers),
}

impl InputBinding {
    /// Creates an exact key/modifier combination.
    pub fn key_combo(key: Key, modifiers: Modifiers) -> Self {
        Self::ModifiedKey {
            key,
            modifiers: ModifierBinding::exact(modifiers),
        }
    }

    /// Creates an exact pointer-button/modifier combination.
    pub fn pointer_button_combo(button: PointerButton, modifiers: Modifiers) -> Self {
        Self::ModifiedPointerButton {
            button,
            modifiers: ModifierBinding::exact(modifiers),
        }
    }

    /// Creates a key binding with an explicit modifier matcher.
    pub fn key_with_modifiers(key: Key, modifiers: ModifierBinding) -> Self {
        Self::ModifiedKey { key, modifiers }
    }

    /// Creates a pointer button binding with an explicit modifier matcher.
    pub fn pointer_button_with_modifiers(
        button: PointerButton,
        modifiers: ModifierBinding,
    ) -> Self {
        Self::ModifiedPointerButton { button, modifiers }
    }

    fn is_down(&self, input: &egui::InputState) -> bool {
        match self {
            Self::Key(key) => input.key_down(*key),
            Self::PointerButton(button) => input.pointer.button_down(*button),
            Self::ModifiedKey { key, modifiers } => {
                modifiers.matches(input.modifiers) && input.key_down(*key)
            }
            Self::ModifiedPointerButton { button, modifiers } => {
                modifiers.matches(input.modifiers) && input.pointer.button_down(*button)
            }
            Self::Modifier(modifiers) => {
                ModifierBinding::requiring(*modifiers).matches(input.modifiers)
            }
        }
    }

    fn action_state(&self, input: &egui::InputState) -> ActionState {
        match self {
            Self::Key(key) => ActionState {
                pressed: input.key_down(*key),
                just_pressed: input.key_pressed(*key),
                just_released: input.key_released(*key),
            },
            Self::PointerButton(button) => ActionState {
                pressed: input.pointer.button_down(*button),
                just_pressed: input.pointer.button_pressed(*button),
                just_released: input.pointer.button_released(*button),
            },
            Self::ModifiedKey { key, modifiers } => {
                if modifiers.matches(input.modifiers) {
                    ActionState {
                        pressed: input.key_down(*key),
                        just_pressed: input.key_pressed(*key),
                        just_released: input.key_released(*key),
                    }
                } else {
                    ActionState::default()
                }
            }
            Self::ModifiedPointerButton { button, modifiers } => {
                if modifiers.matches(input.modifiers) {
                    ActionState {
                        pressed: input.pointer.button_down(*button),
                        just_pressed: input.pointer.button_pressed(*button),
                        just_released: input.pointer.button_released(*button),
                    }
                } else {
                    ActionState::default()
                }
            }
            Self::Modifier(_) => ActionState {
                pressed: self.is_down(input),
                ..ActionState::default()
            },
        }
    }
}

/// Modifier matcher used by combo input bindings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModifierBinding {
    /// Required modifier state.
    pub modifiers: Modifiers,
    /// Whether modifiers beyond the required set are allowed.
    pub allow_extra: bool,
}

impl ModifierBinding {
    /// Matches modifiers exactly.
    pub const fn exact(modifiers: Modifiers) -> Self {
        Self {
            modifiers,
            allow_extra: false,
        }
    }

    /// Requires `modifiers` while allowing additional active modifiers.
    pub const fn requiring(modifiers: Modifiers) -> Self {
        Self {
            modifiers,
            allow_extra: true,
        }
    }

    fn matches(self, active: Modifiers) -> bool {
        let required_match = (!self.modifiers.alt || active.alt)
            && (!self.modifiers.ctrl || active.ctrl)
            && (!self.modifiers.shift || active.shift)
            && (!self.modifiers.mac_cmd || active.mac_cmd)
            && (!self.modifiers.command || active.command);

        if self.allow_extra {
            return required_match;
        }

        required_match
            && active.alt == self.modifiers.alt
            && active.ctrl == self.modifiers.ctrl
            && active.shift == self.modifiers.shift
            && active.mac_cmd == self.modifiers.mac_cmd
            && active.command == self.modifiers.command
    }
}

/// Aggregated pressed and edge state for an action in one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionState {
    /// Whether the action is currently held.
    pub pressed: bool,
    /// Whether the action was pressed this frame.
    pub just_pressed: bool,
    /// Whether the action was released this frame.
    pub just_released: bool,
}

impl ActionState {
    /// Returns whether the action is currently held.
    pub fn pressed(self) -> bool {
        self.pressed
    }

    /// Returns whether the action was pressed this frame.
    pub fn just_pressed(self) -> bool {
        self.just_pressed
    }

    /// Returns whether the action was released this frame.
    pub fn just_released(self) -> bool {
        self.just_released
    }
}

impl std::ops::BitOr for ActionState {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            pressed: self.pressed || rhs.pressed,
            just_pressed: self.just_pressed || rhs.just_pressed,
            just_released: self.just_released || rhs.just_released,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ActionMap, Input, InputBinding, InputState, ModifierBinding};
    use crate::assets::{Asset, LoadedAsset};
    use crate::context::GameContext;
    use crate::test_support::test_asset_context_with_assets;
    use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Sense, Vec2};
    use uuid::Uuid;

    fn binds_nothing(state: &InputState) -> bool {
        state
            .action_map
            .with(|map| map.actions.is_empty() && map.axes.is_empty())
    }

    #[test]
    fn default_action_map_binds_nothing() {
        let map = ActionMap::default();
        assert!(map.actions.is_empty());
        assert!(map.axes.is_empty());
        assert!(binds_nothing(&InputState::default()));
    }

    #[test]
    fn project_without_an_action_map_binds_nothing() {
        let dir = std::env::temp_dir().join(format!("calyx-input-empty-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let game = GameContext::new(test_asset_context_with_assets(vec![dir.clone()]));
        assert!(binds_nothing(game.resources.input()));
        drop(game);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn action_bindings_are_configurable() {
        let mut map = ActionMap::new();
        map.bind_action("shoot", InputBinding::Key(Key::Space));
        map.bind_action("shoot", InputBinding::Key(Key::Enter));

        assert_eq!(map.actions["shoot"].len(), 2);
    }

    #[test]
    fn axis_bindings_are_configurable() {
        let mut map = ActionMap::new();
        map.bind_axis(
            "move_forward",
            InputBinding::Key(Key::W),
            InputBinding::Key(Key::S),
        );

        let axis = &map.axes["move_forward"];
        assert_eq!(axis.positive, InputBinding::Key(Key::W));
        assert_eq!(axis.negative, InputBinding::Key(Key::S));
    }

    #[test]
    fn key_combos_store_exact_modifier_requirements() {
        let binding = InputBinding::key_combo(Key::S, Modifiers::CTRL);

        assert_eq!(
            binding,
            InputBinding::ModifiedKey {
                key: Key::S,
                modifiers: ModifierBinding::exact(Modifiers::CTRL),
            }
        );
    }

    #[test]
    fn exact_modifier_bindings_reject_extra_modifiers() {
        let binding = ModifierBinding::exact(Modifiers::SHIFT);
        let shift_ctrl = Modifiers {
            shift: true,
            ctrl: true,
            ..Default::default()
        };

        assert!(binding.matches(Modifiers::SHIFT));
        assert!(!binding.matches(shift_ctrl));
    }

    #[test]
    fn requiring_modifier_bindings_allow_extra_modifiers() {
        let binding = ModifierBinding::requiring(Modifiers::SHIFT);
        let shift_ctrl = Modifiers {
            shift: true,
            ctrl: true,
            ..Default::default()
        };

        assert!(binding.matches(Modifiers::SHIFT));
        assert!(binding.matches(shift_ctrl));
    }

    #[test]
    fn action_maps_load_from_asset_files() {
        let asset_path =
            std::env::temp_dir().join(format!("calyx-input-{}.cxinput", Uuid::new_v4()));
        let mut map = ActionMap::new();
        map.bind_action("save", InputBinding::key_combo(Key::S, Modifiers::CTRL));
        serde_json::to_writer_pretty(
            std::fs::File::create(&asset_path).expect("failed to create action map asset"),
            &map,
        )
        .expect("failed to write action map asset");

        let loaded =
            LoadedAsset::<ActionMap>::from_json_file(&asset_path).expect("action map should load");

        assert_eq!(ActionMap::file_extensions(), &["cxinput"]);
        assert_eq!(loaded.asset.actions["save"].len(), 1);
        std::fs::remove_file(asset_path).expect("failed to remove temp action map asset");
    }

    fn pass(events: Vec<Event>, modifiers: Modifiers) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0))),
            modifiers,
            events,
            ..Default::default()
        }
    }

    #[test]
    fn pointer_state_is_read_only_while_input_is_active() {
        let ctx = egui::Context::default();
        let mut state = InputState::default();
        ctx.begin_pass(pass(
            vec![
                Event::PointerMoved(Pos2::new(20.0, 30.0)),
                Event::MouseMoved(Vec2::new(4.0, -2.0)),
            ],
            Modifiers::NONE,
        ));
        let active = Input::from_ctx(&ctx, None, &mut state, true);
        let inactive = Input::from_ctx(&ctx, None, &mut state, false);

        assert_eq!(active.pointer_pos(), Some(Pos2::new(20.0, 30.0)));
        assert_eq!(active.pointer_motion(), Vec2::new(4.0, -2.0));
        assert_eq!(active.surface_rect(), None);
        assert_eq!(active.drag_delta(PointerButton::Primary), None);
        assert_eq!(inactive.pointer_pos(), None);
        assert_eq!(inactive.pointer_motion(), Vec2::ZERO);
        assert_eq!(inactive.scroll_delta(), Vec2::ZERO);
        let _ = ctx.end_pass();
    }

    #[test]
    fn drag_delta_reports_surface_drags_by_the_given_button() {
        let ctx = egui::Context::default();
        let mut state = InputState::default();
        let surface = Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0));
        let mut frame = |events: Vec<Event>| {
            ctx.begin_pass(pass(events, Modifiers::NONE));
            let mut drags = None;
            egui::CentralPanel::default().show(&ctx, |ui| {
                let res = ui.interact(surface, ui.id().with("surface"), Sense::drag());
                let input = Input::from_ctx(&ctx, Some(&res), &mut state, true);
                assert_eq!(input.surface_rect(), Some(surface));
                drags = Some((
                    input.drag_delta(PointerButton::Secondary),
                    input.drag_delta(PointerButton::Primary),
                ));
            });
            let _ = ctx.end_pass();
            drags.unwrap()
        };
        let button = |pos: Pos2, pressed: bool| Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };

        frame(vec![Event::PointerMoved(Pos2::new(50.0, 50.0))]);
        frame(vec![button(Pos2::new(50.0, 50.0), true)]);
        frame(vec![Event::PointerMoved(Pos2::new(70.0, 50.0))]);
        let (secondary, primary) = frame(vec![Event::PointerMoved(Pos2::new(80.0, 55.0))]);

        assert_eq!(secondary, Some(Vec2::new(10.0, 5.0)));
        assert_eq!(primary, None);
    }

    #[test]
    fn modifier_bindings_hold_while_their_modifiers_are_held() {
        let mut map = ActionMap::new();
        map.bind_action("boost", InputBinding::Modifier(Modifiers::CTRL));
        map.bind_axis(
            "vertical",
            InputBinding::Key(Key::Space),
            InputBinding::Modifier(Modifiers::SHIFT),
        );
        let ctx = egui::Context::default();
        let mut state = InputState::new(map);
        let ctrl_shift = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };

        ctx.begin_pass(pass(Vec::new(), ctrl_shift));
        let input = Input::from_ctx(&ctx, None, &mut state, true);
        assert!(input.action("boost").pressed());
        assert!(!input.action("boost").just_pressed());
        assert_eq!(input.axis("vertical"), -1.0);
        let _ = ctx.end_pass();

        ctx.begin_pass(pass(Vec::new(), Modifiers::NONE));
        let input = Input::from_ctx(&ctx, None, &mut state, true);
        assert!(!input.action("boost").pressed());
        assert_eq!(input.axis("vertical"), 0.0);
        let _ = ctx.end_pass();
    }
}
