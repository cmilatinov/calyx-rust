use std::collections::HashSet;
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
    task_list: HashSet<isize>,
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

    /// Returns the set of task identifiers currently queued or running.
    pub fn task_list(&self) -> &HashSet<isize> {
        &self.task_list
    }

    /// Returns the underlying thread pool.
    pub fn thread_pool(&self) -> &ThreadPool {
        &self.thread_pool
    }

    /// Schedules `task` on the background pool and tracks it by `id` until the
    /// task completes.
    pub fn execute<F: FnOnce() + Send + 'static>(
        &mut self,
        id: impl Into<isize>,
        task: F,
    ) -> JoinHandle<()> {
        let id = id.into();
        self.task_list.insert(id);
        let background = self.background.upgrade().unwrap();
        self.thread_pool.evaluate(move || {
            task();
            background.write().task_list.remove(&id);
        })
    }
}
