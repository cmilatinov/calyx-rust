extern crate utils_derive;

pub use utils_derive::*;

pub use self::singleton::*;
pub use self::type_ids::*;

mod singleton;
mod type_ids;
