use std::collections::HashSet;
use std::time::Duration;

use rusty_pool::{JoinHandle, ThreadPool};

use utils::singleton_with_init;

pub struct Background {
    thread_pool: ThreadPool,
    task_list: HashSet<isize>,
}

singleton_with_init!(Background);

impl Default for Background {
    fn default() -> Self {
        Self {
            thread_pool: ThreadPool::new(1, 10, Duration::from_secs(30)),
            task_list: HashSet::new(),
        }
    }
}

impl Background {
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
        self.thread_pool.evaluate(move || {
            task();
            Background::get_mut().task_list.remove(&id);
        })
    }
}
