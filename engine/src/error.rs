use std::error::Error;

pub type DynError = dyn Error + Send + Sync;

pub type BoxedError = Box<DynError>;
