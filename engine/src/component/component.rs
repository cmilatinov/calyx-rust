use crate::scene::Scene;

pub trait Component {
    fn start(&mut self, scene: &mut Scene);
    fn update(&mut self, scene: &mut Scene);
    fn destroy(&mut self, scene: &mut Scene);
}

#[macro_export]
macro_rules! component {
    (pub struct $name:ident { $($field:vis $field_name:ident : $field_type:ty),* }) => {
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
    use utils::utils_derive::Component;
    use uuid::Uuid;
    use sha1::{Sha1, Digest};

    #[test]
    pub fn create_component() {
        #[derive(Component)]
        pub struct ComponentTest {
            test_visible: i32
        }
        
        let expected_uuid = {
            let mut hasher = Sha1::new();
            hasher.update(b"ComponentTest");
            let hash = hasher.finalize();
            let uuid_bytes = [
                hash[0], hash[1], hash[2], hash[3],
                hash[4], hash[5], hash[6], hash[7],
                hash[8], hash[9], hash[10], hash[11],
                hash[12], hash[13], hash[14], hash[15]
            ];

            Uuid::from_bytes(uuid_bytes)
        };

        assert_eq!(ComponentTest::type_uuid(), expected_uuid);
    }
}
