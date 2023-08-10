pub trait Init {
    type Type;
    fn initialize(_instance: &mut Self::Type) {}
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

#[macro_export]
macro_rules! singleton_with_init {
    ($t:tt) => {
        $crate::singleton!($t);

        impl Init for $t {
            type Type = $t;
        }
    }
}

pub use singleton;
pub use singleton_with_init;