pub trait Init<T> {
    fn initialize(_instance: &mut T) {}
}

#[macro_export]
macro_rules! singleton {
    ($t:tt) => {
        lazy_static! {
            pub static ref INSTANCE: Mutex<$t> = Mutex::new($t::default());
        }

        impl $t {
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
macro_rules! get_singleton_instance {
    () => {
        INSTANCE.lock()
            .expect("Failed to lock singleton instance")
    }
}

pub use singleton;
pub use get_singleton_instance;