use std::collections::HashMap;
use std::time::Duration;

use crate as calyx_engine;
use crate::core::{Ref, WeakRef};
use crate::resource::Resource;
use crate::utils::TypeUuid;
use rusty_pool::{JoinHandle, ThreadPool};

/// Shared background task executor used by editor and runtime systems.
#[derive(Resource, TypeUuid)]
#[uuid = "38c4c3b8-07c7-4858-a754-d34c6ad1ebc1"]
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
mod tests;
