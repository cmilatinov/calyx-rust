use std::sync::{Arc, RwLock};

pub type Ref<T> = Arc<RwLock<T>>;
pub type OptionRef<T> = Option<Ref<T>>;

pub fn create_ref<T>(value: T) -> Ref<T> {
    Arc::new(RwLock::new(value))
}
pub fn create_option_ref<T>(value: T) -> OptionRef<T> { Some(create_ref(value)) }