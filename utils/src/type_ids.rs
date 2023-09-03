#[macro_export]
macro_rules! type_ids {
    ($($ty:ty),*) => {
        vec![
            $(TypeId::of::<$ty>()),*
        ]
    };
}