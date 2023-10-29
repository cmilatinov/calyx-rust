use num_derive::{FromPrimitive, ToPrimitive};

#[derive(FromPrimitive, ToPrimitive)]
pub enum TaskId {
    Build,
    Clean,
}

impl From<TaskId> for isize {
    fn from(value: TaskId) -> Self {
        value as isize
    }
}

impl TaskId {
    pub fn message(&self) -> &'static str {
        match self {
            TaskId::Build => "Building assemblies",
            TaskId::Clean => "Cleaning build files",
        }
    }
}
