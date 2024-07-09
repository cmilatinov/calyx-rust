pub struct InputState {
    pub is_active: bool,
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
        self.res.clone()
    }

    pub fn input<R: Default>(&self, reader: impl FnOnce(&egui::InputState) -> R) -> R {
        self.context.input(|input| {
            if self.state.is_active {
                reader(input)
            } else {
                Default::default()
            }
        })
    }
}
