use std::collections::HashMap;
use std::io::Error;
use std::path::Path;

use crate as engine;
use crate::assets::{Asset, AssetRegistry, LoadedAsset};
use crate::context::ReadOnlyAssetContext;
use crate::utils::TypeUuid;
use egui::{Key, Modifiers, PointerButton};
use serde::{Deserialize, Serialize};

/// Mutable input state cached between UI frames.
#[derive(Default)]
pub struct InputState {
    /// Whether input sampling is currently enabled.
    pub is_active: bool,
    /// Last known cursor position used for delta fallbacks.
    pub last_cursor_pos: Option<egui::Pos2>,
    /// Action map used to interpret raw egui input.
    pub action_map: ActionMap,
}

/// Read-only input view for the current frame.
pub struct Input<'a> {
    context: &'a egui::Context,
    res: Option<&'a egui::Response>,
    state: InputState,
}

impl<'a> Input<'a> {
    /// Creates a frame input wrapper from egui state.
    pub fn from_ctx(
        context: &'a egui::Context,
        res: Option<&'a egui::Response>,
        state: InputState,
    ) -> Self {
        Self {
            context,
            res,
            state,
        }
    }

    /// Returns the underlying egui context.
    pub fn ctx(&self) -> &egui::Context {
        self.context
    }

    /// Returns the optional egui response bound to this input surface.
    pub fn res(&self) -> Option<&egui::Response> {
        self.res
    }

    /// Reads raw egui input when input is active.
    pub fn input<R>(&self, reader: impl FnOnce(&egui::InputState) -> R) -> Option<R> {
        self.context.input(|input| {
            if self.state.is_active {
                Some(reader(input))
            } else {
                None
            }
        })
    }

    /// Mutates raw egui input when input is active.
    pub fn input_mut<R: Default>(
        &self,
        reader: impl FnOnce(&mut egui::InputState) -> R,
    ) -> Option<R> {
        self.context.input_mut(|input| {
            if self.state.is_active {
                Some(reader(input))
            } else {
                None
            }
        })
    }

    /// Resolves a named action into a frame-local [`ActionState`].
    pub fn action(&self, name: &str) -> ActionState {
        if !self.state.is_active {
            return ActionState::default();
        }

        self.context.input(|input| {
            self.state
                .action_map
                .action(name, input)
                .unwrap_or_default()
        })
    }

    /// Resolves a named axis into a scalar value.
    pub fn axis(&self, name: &str) -> f32 {
        if !self.state.is_active {
            return 0.0;
        }

        self.context
            .input(|input| self.state.action_map.axis(name, input).unwrap_or_default())
    }

    /// Returns pointer motion for the frame.
    pub fn cursor_delta(&self) -> egui::Vec2 {
        if !self.state.is_active {
            return egui::Vec2::ZERO;
        }
        if let Some(last_pos) = self.state.last_cursor_pos {
            if let Some(pos) = self.context.input(|input| input.pointer.interact_pos()) {
                let diff = pos - last_pos;
                let diff_abs = diff.abs();
                return if diff_abs.x < 0.5 || diff_abs.y < 0.5 {
                    egui::Vec2::ZERO
                } else {
                    diff
                };
            }
        }
        self.context.input(|input| input.pointer.delta())
    }
}

/// Asset-backed mapping from action names to bindings and axes.
#[derive(Clone, Debug, Serialize, Deserialize, TypeUuid)]
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

impl Default for ActionMap {
    fn default() -> Self {
        let mut map = Self::new();
        map.bind_action("shoot", InputBinding::PointerButton(PointerButton::Primary));
        map.bind_action("shoot", InputBinding::Key(Key::Space));
        map.bind_action("jump", InputBinding::Key(Key::Space));
        map.bind_axis(
            "move_forward",
            InputBinding::Key(Key::W),
            InputBinding::Key(Key::S),
        );
        map.bind_axis(
            "move_right",
            InputBinding::Key(Key::D),
            InputBinding::Key(Key::A),
        );
        map
    }
}

impl ActionMap {
    /// Creates an empty action map.
    pub fn new() -> Self {
        Self {
            actions: HashMap::new(),
            axes: HashMap::new(),
        }
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
    use super::{ActionMap, InputBinding, ModifierBinding};
    use crate::assets::{Asset, LoadedAsset};
    use egui::{Key, Modifiers};
    use uuid::Uuid;

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
}
