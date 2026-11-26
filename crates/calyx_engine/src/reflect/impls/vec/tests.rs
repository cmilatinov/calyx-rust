use super::*;

#[test]
fn vec_type_uuid_depends_on_element_type() {
    assert_ne!(Vec::<f32>::type_uuid(), Vec::<u32>::type_uuid());
    assert_ne!(Vec::<f32>::type_uuid(), f32::type_uuid());
    assert_ne!(Vec::<Vec<f32>>::type_uuid(), Vec::<f32>::type_uuid());
    assert_eq!(Vec::<f32>::type_uuid(), Vec::<f32>::type_uuid());
}

#[test]
fn vec_type_uuid_is_stable() {
    // Saved data and plugins identify list types by this UUID, so it must
    // not drift between builds.
    assert_eq!(
        Vec::<f32>::type_uuid(),
        Uuid::parse_str("c6542891-c3ae-d736-d490-838163fa1989").unwrap()
    );
}

#[test]
fn vec_list_edits_elements_through_reflection() {
    let mut values = vec![1.0f32, 2.0];
    let list = values
        .as_reflect_mut()
        .as_list_mut()
        .expect("Vec is a list");

    *list.element_mut(0).unwrap().downcast_mut::<f32>().unwrap() = 5.0;
    assert!(list.push(Box::new(3.0f32)).is_ok());
    assert!(list.push(Box::new(4u32)).is_err());
    let removed = list.remove(1).expect("index 1 exists");
    assert_eq!(*removed.downcast::<f32>().ok().unwrap(), 2.0);
    assert!(list.remove(5).is_none());
    assert!(list.insert(0, Box::new(8.0f32)).is_ok());
    assert!(list.insert(9, Box::new(8.0f32)).is_err());
    assert!(list.insert(0, Box::new(1u32)).is_err());
    assert!(list.element(3).is_none());

    assert_eq!(values, vec![8.0, 5.0, 3.0]);
}

#[test]
fn non_list_values_are_not_lists() {
    let mut value = 1.0f32;
    assert!(value.as_reflect().as_list().is_none());
    assert!(value.as_reflect_mut().as_list_mut().is_none());
}
