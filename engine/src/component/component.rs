use crate::scene::Scene;

pub trait Component {
    fn start(&mut self, scene: &mut Scene);
    fn update(&mut self, scene: &mut Scene);
    fn destroy(&mut self, scene: &mut Scene);
}

#[macro_export]
macro_rules! component {
    (pub struct $name:ident { $($field:vis $field_name:ident : $field_type:ty),* }) => {
        use inventory::submit;

        #[derive(Default)]
        pub struct $name {
            $($field $field_name: $field_type),*,
        }

        impl specs::Component for $name {
            type Storage = specs::VecStorage<Self>;
        }
    };
}

#[cfg(test)]
mod tests {
    #[test]
    pub fn create_component() {
        component! {
            pub struct ComponentTest {
                test_visible: i32
            }
        }

    }
}
