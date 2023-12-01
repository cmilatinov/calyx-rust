pub trait Init {
    fn initialize(&mut self) {}
}

#[macro_export]
macro_rules! singleton {
    ($t:tt) => {
        lazy_static::lazy_static! {
            static ref INSTANCE: std::sync::RwLock<$t> = std::sync::RwLock::new($t::default());
        }

        impl $t {
            pub fn get() -> std::sync::RwLockReadGuard<'static, $t> {
                INSTANCE.read().unwrap()
            }

            pub fn get_mut() -> std::sync::RwLockWriteGuard<'static, $t> {
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
        use engine::utils::Init;
        engine::utils::singleton!($t);

        impl Init for $t {}
    };
}

pub use singleton;
pub use singleton_with_init;
