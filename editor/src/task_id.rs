use num_derive::{FromPrimitive, ToPrimitive};

#[derive(FromPrimitive, ToPrimitive)]
pub enum TaskId {
    Build,
    Rebuild,
    Autosave,
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
            TaskId::Rebuild => "Rebuilding assemblies",
            TaskId::Autosave => "Autosaving scene",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TaskId;
    use num_traits::FromPrimitive;

    #[test]
    fn autosave_task_round_trips_to_status_message() {
        let id = isize::from(TaskId::Autosave);

        assert_eq!(
            TaskId::from_isize(id).unwrap().message(),
            "Autosaving scene"
        );
    }
}
