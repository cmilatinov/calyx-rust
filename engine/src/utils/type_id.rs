#[macro_export]
macro_rules! type_uuids {
    ($($ty:ty),*) => {{
        use engine::utils::TypeUuid;
        vec![
            $(<$ty>::type_uuid()),*
        ]
    }};
}
