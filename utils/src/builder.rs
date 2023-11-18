#[macro_export]
macro_rules! impl_builder_fn {
    ($name:ident: $ty:ty) => {
        pub fn $name(mut self, $name: $ty) -> Self {
            self.$name = $name;
            self
        }
    };
}
