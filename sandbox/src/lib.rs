mod test;
// mod tps_camera;

use engine::reflect::type_registry::TypeRegistry;

pub struct ReflectRegistrationFn {
    pub name: &'static str,
    pub function: fn(&mut TypeRegistry),
}
inventory::collect!(ReflectRegistrationFn);

#[no_mangle]
pub extern "C" fn plugin_main(registry: &mut TypeRegistry) {
    for f in inventory::iter::<ReflectRegistrationFn>() {
        (f.function)(registry);
    }
}
