use std::collections::HashMap;

use egui::{Key, PointerButton};

#[derive(Default)]
pub struct InputState {
    pub is_active: bool,
    pub last_cursor_pos: Option<egui::Pos2>,
    pub action_map: ActionMap,
}

pub struct Input<'a> {
    context: &'a egui::Context,
    res: Option<&'a egui::Response>,
    state: InputState,
}

impl<'a> Input<'a> {
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

    pub fn ctx(&self) -> &egui::Context {
        self.context
    }

    pub fn res(&self) -> Option<&egui::Response> {
        self.res
    }

    pub fn input<R>(&self, reader: impl FnOnce(&egui::InputState) -> R) -> Option<R> {
        self.context.input(|input| {
            if self.state.is_active {
                Some(reader(input))
            } else {
                None
            }
        })
    }

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

    pub fn axis(&self, name: &str) -> f32 {
        if !self.state.is_active {
            return 0.0;
        }

        self.context
            .input(|input| self.state.action_map.axis(name, input).unwrap_or_default())
    }

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

#[derive(Clone, Debug)]
pub struct ActionMap {
    actions: HashMap<String, Vec<InputBinding>>,
    axes: HashMap<String, AxisBinding>,
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
    pub fn new() -> Self {
        Self {
            actions: HashMap::new(),
            axes: HashMap::new(),
        }
    }

    pub fn bind_action(&mut self, name: impl Into<String>, binding: InputBinding) {
        self.actions.entry(name.into()).or_default().push(binding);
    }

    pub fn set_action_bindings(
        &mut self,
        name: impl Into<String>,
        bindings: impl IntoIterator<Item = InputBinding>,
    ) {
        self.actions
            .insert(name.into(), bindings.into_iter().collect());
    }

    pub fn bind_axis(
        &mut self,
        name: impl Into<String>,
        positive: InputBinding,
        negative: InputBinding,
    ) {
        self.axes
            .insert(name.into(), AxisBinding { positive, negative });
    }

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

    pub fn axis(&self, name: &str, input: &egui::InputState) -> Option<f32> {
        self.axes.get(name).map(|axis| axis.value(input))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AxisBinding {
    pub positive: InputBinding,
    pub negative: InputBinding,
}

impl AxisBinding {
    fn value(&self, input: &egui::InputState) -> f32 {
        let positive = self.positive.is_down(input) as u8 as f32;
        let negative = self.negative.is_down(input) as u8 as f32;
        positive - negative
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputBinding {
    Key(Key),
    PointerButton(PointerButton),
}

impl InputBinding {
    fn is_down(&self, input: &egui::InputState) -> bool {
        match self {
            Self::Key(key) => input.key_down(*key),
            Self::PointerButton(button) => input.pointer.button_down(*button),
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
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionState {
    pub pressed: bool,
    pub just_pressed: bool,
    pub just_released: bool,
}

impl ActionState {
    pub fn pressed(self) -> bool {
        self.pressed
    }

    pub fn just_pressed(self) -> bool {
        self.just_pressed
    }

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
    use super::{ActionMap, InputBinding};
    use egui::Key;

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
}
