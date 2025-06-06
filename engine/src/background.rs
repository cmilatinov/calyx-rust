use std::collections::HashSet;
use std::time::Duration;

use crate as engine;
use crate::core::{Ref, WeakRef};
use crate::resource::Resource;
use rusty_pool::{JoinHandle, ThreadPool};

#[derive(Resource)]
pub struct Background {
    thread_pool: ThreadPool,
    task_list: HashSet<isize>,
    background: WeakRef<Background>,
}

impl Background {
    pub fn new() -> Ref<Self> {
        Ref::new_cyclic(|background| Self {
            thread_pool: ThreadPool::new(1, 10, Duration::from_secs(30)),
            task_list: Default::default(),
            background,
        })
    }

    pub fn task_list(&self) -> &HashSet<isize> {
        &self.task_list
    }

    pub fn thread_pool(&self) -> &ThreadPool {
        &self.thread_pool
    }

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
