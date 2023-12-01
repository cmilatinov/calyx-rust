#[macro_export]
macro_rules! type_ids {
    ($($ty:ty),*) => {
        vec![
            $(std::any::TypeId::of::<$ty>()),*
        ]
    };
}

pub use type_ids;
