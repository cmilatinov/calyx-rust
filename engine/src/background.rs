use std::collections::HashMap;
use std::time::Duration;

use crate as engine;
use crate::core::{Ref, WeakRef};
use crate::resource::Resource;
use crate::utils::TypeUuid;
use rusty_pool::{JoinHandle, ThreadPool};

/// Shared background task executor used by editor and runtime systems.
#[derive(Resource, TypeUuid)]
#[uuid = "38c4c3b8-07c7-4858-a754-d34c6ad1ebc1"]
#[repr(C)]
pub struct Background {
    thread_pool: ThreadPool,
    task_list: HashMap<isize, usize>,
    background: WeakRef<Background>,
}

impl Background {
    /// Creates the default background executor resource.
    pub fn new() -> Ref<Self> {
        Ref::new_cyclic(|background| Self {
            thread_pool: ThreadPool::new(1, 10, Duration::from_secs(30)),
            task_list: Default::default(),
            background,
        })
    }

    /// Returns the number of queued or running tasks for each task kind.
    pub fn task_list(&self) -> &HashMap<isize, usize> {
        &self.task_list
    }

    /// Returns the total number of queued or running tasks across all kinds.
    pub fn task_count(&self) -> usize {
        self.task_list.values().sum()
    }

    /// Schedules `task` on the background pool and tracks it under the kind
    /// `id` until the task completes or panics.
    pub fn execute<F: FnOnce() + Send + 'static>(
        &mut self,
        id: impl Into<isize>,
        task: F,
    ) -> JoinHandle<()> {
        let id = id.into();
        *self.task_list.entry(id).or_insert(0) += 1;
        let guard = TaskGuard {
            id,
            background: WeakRef::new(&self.background.upgrade().unwrap()),
        };
        self.thread_pool.evaluate(move || {
            let _guard = guard;
            task();
        })
    }
}

/// Releases a task's slot in the task list when dropped, including on panic.
struct TaskGuard {
    id: isize,
    background: WeakRef<Background>,
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        let Some(background) = self.background.upgrade() else {
            return;
        };
        let mut background = background.write();
        if let Some(count) = background.task_list.get_mut(&self.id) {
            *count -= 1;
            if *count == 0 {
                background.task_list.remove(&self.id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
}
