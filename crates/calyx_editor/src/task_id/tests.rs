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
