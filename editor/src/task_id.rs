use num_derive::{FromPrimitive, ToPrimitive};

#[derive(FromPrimitive, ToPrimitive)]
pub enum TaskId {
    Build,
    Clean
}

impl Into<isize> for TaskId {
    fn into(self) -> isize {
        self as isize
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