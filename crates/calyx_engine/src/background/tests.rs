use super::*;
use std::sync::mpsc;

#[test]
fn concurrent_tasks_of_one_kind_are_tracked_independently() {
    let background = Background::new();
    let (release_a, wait_a) = mpsc::channel::<()>();
    let (release_b, wait_b) = mpsc::channel::<()>();
    let a = background.write().execute(1isize, move || {
        let _ = wait_a.recv();
    });
    let b = background.write().execute(1isize, move || {
        let _ = wait_b.recv();
    });
    assert_eq!(background.read().task_list().get(&1), Some(&2));
    assert_eq!(background.read().task_count(), 2);

    release_a.send(()).unwrap();
    a.await_complete();
    assert_eq!(background.read().task_list().get(&1), Some(&1));

    release_b.send(()).unwrap();
    b.await_complete();
    assert!(background.read().task_list().is_empty());
}

#[test]
fn panicking_task_clears_its_entry() {
    let background = Background::new();
    let handle = background
        .write()
        .execute(2isize, || panic!("task failure"));
    handle.try_await_complete().ok();
    // The guard drops while unwinding, which may race with the join handle.
    for _ in 0..200 {
        if background.read().task_list().is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("panicked task left a stale entry");
}
