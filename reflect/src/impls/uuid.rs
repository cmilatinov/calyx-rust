use uuid::Uuid;

use reflect_derive::{impl_extern_type_uuid, impl_reflect_value};

use crate as reflect;

impl_extern_type_uuid!(Uuid, "26e09929-d7bb-4928-bed4-49bf8e62d5af");
impl_reflect_value!(Uuid());
