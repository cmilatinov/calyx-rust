use crate as reflect;
use reflect_derive::impl_reflect_value;
use uuid::Uuid;

impl_reflect_value!(Uuid());
