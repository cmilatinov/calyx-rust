#[derive(Default)]
pub struct InputState {
    pub is_active: bool,
    pub last_cursor_pos: Option<egui::Pos2>,
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
