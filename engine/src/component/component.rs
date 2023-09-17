use indextree::NodeId;
use legion::storage::ComponentTypeId;
use legion::world::{Entry, EntryRef};
use reflect::{Reflect, reflect_trait};
use crate::render::Gizmos;
use crate::scene::Scene;

pub trait TypeUUID {
    fn type_uuid(&self) -> uuid::Uuid;
}

pub trait ComponentInstance {
    fn component_type_id(&self) -> ComponentTypeId;
    fn get_instance<'a>(&self, entry: &'a EntryRef) -> Option<&'a dyn Component>;
    fn get_instance_mut<'a>(&self, entry: &'a mut Entry) -> Option<&'a mut dyn Component>;
}

#[reflect_trait]
pub trait Component: TypeUUID + Reflect + ComponentInstance {
    fn start(&mut self, _scene: &Scene) {}
    fn update(&mut self, _scene: &Scene) {}
    fn destroy(&mut self, _scene: &Scene) {}
    fn draw_gizmos(&self, _scene: &Scene, _node: NodeId, _gizmos: &mut Gizmos) {}
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
