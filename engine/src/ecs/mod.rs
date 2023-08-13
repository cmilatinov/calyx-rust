mod id;
mod transform;
mod mesh;

pub use id::*;
pub use transform::*;
pub use mesh::*;

use crate::scene::Scene;

pub trait Component {
    fn start(&mut self, scene: &mut Scene);
    fn update(&mut self, scene: &mut Scene);
    fn destroy(&mut self, scene: &mut Scene);
}

#[macro_export]
macro_rules! component {
    (pub struct $name:ident { $($field:vis $field_name:ident : $field_type:ty),* }) => {
        use bevy_reflect::Reflect;

        #[derive(Reflect)]
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
            type Storage = specs::VecStorage<Self>;
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::ecs::Component;
    use crate::scene::Scene;

    #[test]
    pub fn create_component() {
        component! {
            pub struct ComponentTest {
                test_visible: i32
            }
        }

        impl Component for ComponentTest {
            fn start(&mut self, scene: &mut Scene) {}

            fn update(&mut self, scene: &mut Scene) {}

            fn destroy(&mut self, scene: &mut Scene) {}
        }

    }
}
