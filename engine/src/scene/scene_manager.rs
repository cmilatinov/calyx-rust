use std::ops::DerefMut;

use crate as engine;
use crate::utils::singleton_with_init;

#[derive(Default)]
pub struct SceneManager;
singleton_with_init!(SceneManager);
