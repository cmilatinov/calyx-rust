use crate::utils::Init;
use crate::singleton;

#[derive(Default)]
pub struct NetRegistry;

singleton!(NetRegistry);


impl Init for NetRegistry {
    type Type = NetRegistry;
    fn initialize(instance: &mut Self::Type) {
    }
}
