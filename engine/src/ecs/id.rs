use specs::VecStorage;
use uuid::Uuid;

#[derive(Default)]
pub struct ComponentID {
    pub id: Uuid,
    pub name: String
}

impl specs::Component for ComponentID {
    type Storage = VecStorage<Self>;
}

impl ComponentID {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "Game Object".to_string()
        }
    }
}