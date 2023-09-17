pub trait Init {
    fn initialize(&mut self) {}
}

#[macro_export]
macro_rules! singleton {
    ($t:tt) => {
        use lazy_static::lazy_static;
        use std::ops::DerefMut;
        use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

        lazy_static! {
            static ref INSTANCE: RwLock<$t> = RwLock::new($t::default());
        }

        impl $t {
            pub fn get() -> RwLockReadGuard<'static, $t> {
                INSTANCE.read().unwrap()
            }

            pub fn get_mut() -> RwLockWriteGuard<'static, $t> {
                INSTANCE.write().unwrap()
            }

            pub fn init() {
                let mut binding = INSTANCE.write().unwrap();
                let instance = binding.deref_mut();
                instance.initialize();
            }
        }
    };
}

#[macro_export]
macro_rules! singleton_with_init {
    ($t:tt) => {
        use ::utils::Init;
        ::utils::singleton!($t);

        impl Init for $t {}
    };
}

pub use singleton;
pub use singleton_with_init;
