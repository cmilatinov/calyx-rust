use std::sync::{Arc, RwLock};

pub type Ref<T> = Arc<RwLock<T>>;

pub fn create_ref<T>(value: T) -> Ref<T> {
    Arc::new(RwLock::new(value))
}