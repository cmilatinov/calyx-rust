pub mod mesh;
pub mod transform;

struct Data {
    value: i32,
}

#[macro_export]
macro_rules! component {
    (pub struct $name:ident { $($field:vis $field_name:ident : $field_type:ty),* }) => {
        pub struct $name {
            $($field $field_name: $field_type),*,
            pub scene: Rc<RefCell<Data>>,
        }

        impl $name {
            pub fn new($($field_name: $field_type),*) -> Self {
                $name {
                    $($field_name),*,
                    scene: Rc::new(RefCell::new(Data { value: 24 })),
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

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

        let mut component = ComponentTest::new(23);
        let scene = component.scene.borrow();
        assert_eq!(scene.value, 24);
    }
}
