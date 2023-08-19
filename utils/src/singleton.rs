pub trait Init {
    type Type;
    fn initialize(_instance: &mut Self::Type) {}
}

#[macro_export]
macro_rules! singleton {
    ($t:tt) => {
        use std::ops::DerefMut;
        use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
        use lazy_static::lazy_static;

        lazy_static! {
            pub static ref INSTANCE: RwLock<$t> = RwLock::new($t::default());
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
                $t::initialize(instance);
            }
        }
    }
}

#[macro_export]
macro_rules! singleton_with_init {
    ($t:tt) => {
        use ::utils::Init;
        ::utils::singleton!($t);

        impl Init for $t {
            type Type = $t;
        }
    }
}

pub use singleton;
pub use singleton_with_init;