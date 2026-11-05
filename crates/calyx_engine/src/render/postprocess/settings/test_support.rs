//! Test constructors and comparisons; runtime resolution never clones settings.
use super::*;

impl Clone for PostProcessSettings {
    fn clone(&self) -> Self {
        let types = TypeRegistry::new();
        let mut result = Self::empty();
        for slot in self.effects.values().filter(|slot| slot.active) {
            result.apply(slot.effect.as_ref(), &types, None);
        }
        result
    }
}

impl PostProcessSettings {
    pub(crate) fn blend(&self, effects: &Self, factor: f32) -> Self {
        let mut result = self.clone();
        let types = TypeRegistry::new();
        for slot in effects.effects.values().filter(|slot| slot.active) {
            result.apply(slot.effect.as_ref(), &types, Some(factor));
        }
        result
    }

    pub(crate) fn from_profile(profile: &PostProcessProfile) -> Self {
        let mut result = Self::default();
        let types = TypeRegistry::new();
        for effect in profile.enabled_effects() {
            result.apply(effect, &types, None);
        }
        result
    }
}
