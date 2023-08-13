use specs::{Component, VecStorage};
use uuid::Uuid;

#[derive(Default)]
pub struct ComponentID {
    pub id: Uuid
}

impl Component for ComponentID {
    type Storage = VecStorage<Self>;
}

impl ComponentID {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4()
        }
    }
}