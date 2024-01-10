mod test;

use engine::reflect::type_registry::TypeRegistry;
use engine::reflect::ReflectDefault;
use std::any::TypeId;

pub struct ReflectRegistrationFn(pub fn(&mut TypeRegistry));
engine::inventory::collect!(ReflectRegistrationFn);

#[no_mangle]
pub extern "C" fn plugin_main(registry: &mut TypeRegistry) {
    println!("ReflectDefault - {:?}", TypeId::of::<ReflectDefault>());
    for f in engine::inventory::iter::<ReflectRegistrationFn>() {
        f.0(registry);
    }
}
