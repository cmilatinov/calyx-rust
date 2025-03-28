use std::collections::HashSet;
use std::time::Duration;

use crate::core::Ref;
use rusty_pool::{JoinHandle, ThreadPool};

pub struct Background {
    thread_pool: ThreadPool,
    task_list: Ref<HashSet<isize>>,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            thread_pool: ThreadPool::new(1, 10, Duration::from_secs(30)),
            task_list: Ref::new(Default::default()),
        }
    }
}

impl Background {
    pub fn new() -> Ref<Self> {
        Ref::new(Self {
            thread_pool: ThreadPool::new(1, 10, Duration::from_secs(30)),
            task_list: Ref::new(Default::default()),
        })
    }

    pub fn task_list(&self) -> Ref<HashSet<isize>> {
        self.task_list.clone()
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
        let task_list = self.task_list.clone();
        task_list.write().insert(id);
        self.thread_pool.evaluate(move || {
            task();
            task_list.write().remove(&id);
        })
    }
}
