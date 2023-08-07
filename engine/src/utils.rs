pub trait Init<T> {
    fn initialize(_instance: &mut T) {}
}

#[macro_export]
macro_rules! singleton {
    ($t:tt) => {
        use std::ops::DerefMut;
        use std::sync::{Mutex, MutexGuard};
        use lazy_static::lazy_static;

        lazy_static! {
            pub static ref INSTANCE: Mutex<$t> = Mutex::new($t::default());
        }

        impl $t {
            pub fn get() -> MutexGuard<'static, $t> {
                INSTANCE.lock()
                    .expect("Failed to lock singleton instance of $t")
            }

            pub fn init() {
                let mut binding = INSTANCE.lock()
                    .expect("Failed to lock singleton instance of $t");
                let instance = binding.deref_mut();
                $t::initialize(instance);
            }
        }
    }
}

pub use singleton;