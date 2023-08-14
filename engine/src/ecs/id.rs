use uuid::Uuid;
use crate::component;

component! {
    pub struct ComponentID {
        pub id: Uuid,
        pub name: String
    }
}

impl ComponentID {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "Game Object".to_string(),
        }
    }
}
