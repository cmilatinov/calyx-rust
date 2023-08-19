use crate::{singleton_with_init};

#[derive(Default)]
pub struct NetRegistry;

singleton_with_init!(NetRegistry);
