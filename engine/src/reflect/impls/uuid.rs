use uuid::Uuid;

use engine_derive::impl_reflect_value;

use crate as engine;

impl_reflect_value!(Uuid());
