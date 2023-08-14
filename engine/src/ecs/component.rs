use bevy_reflect::{Reflect, TypeRegistry};
use crate::scene::Scene;
use egui::Ui;

pub trait Component: Reflect {
    fn start(&mut self, scene: &mut Scene);
    fn update(&mut self, scene: &mut Scene);
    fn destroy(&mut self, scene: &mut Scene);
}

pub struct ComponentInfo {
    pub register: fn(&mut TypeRegistry)
}

inventory::collect!(ComponentInfo);

#[macro_export]
macro_rules! component {
    (pub struct $name:ident { $($field:vis $field_name:ident : $field_type:ty),* }) => {
        use bevy_reflect::Reflect;
        use inventory::submit;

        #[derive(Reflect, Default)]
        pub struct $name {
            $($field $field_name: $field_type),*,
        }

        impl $name {
            pub fn register(registry: &mut TypeRegistry) {
                registry.register::<Self>();
            }
        }

        impl specs::Component for $name {
            type Storage = specs::VecStorage<Self>;
        }

        inventory::submit! {
            ComponentInfo {
                register: $name::register
            }
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
