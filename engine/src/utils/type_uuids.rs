#[macro_export]
macro_rules! type_uuids {
    ($($ty:ty),*) => {
        vec![
            $(<$ty>::type_uuid()),*
        ]
    };
}

pub use type_uuids;
