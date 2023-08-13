use crate::scene::Scene;

pub mod mesh;
pub mod transform;

pub trait Component {
    fn start(&mut self, scene: Scene);
    fn update(&mut self, scene: Scene);
    fn destroy(&mut self, scene: Scene);
}

#[macro_export]
macro_rules! component {
    (pub struct $name:ident { $($field:vis $field_name:ident : $field_type:ty),* }) => {
        pub struct $name {
            $($field $field_name: $field_type),*,
        }

        impl $name {
            pub fn new($($field_name: $field_type),*) -> Self {
                $name {
                    $($field_name),*
                }
            }
        }

        impl specs::Component for $name {
            type specs::Storage = specs::VecStorage<Self>;
        }
    };
}

#[cfg(test)]
mod tests {
    struct Data {
        value: i32,
    }

    #[test]
    pub fn create_component() {
        component! {
            pub struct ComponentTest {
                test_visible: i32
            }
        }
    }
}
