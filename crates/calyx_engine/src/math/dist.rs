use nalgebra_glm::{Vec2, Vec3, Vec4};

/// Distance metric implemented for scalars and common vector types.
pub trait Distance {
    /// Returns the distance from `self` to `other`.
    fn distance(&self, other: &Self) -> f32
    where
        Self: Sized;
}

impl Distance for f32 {
    fn distance(&self, other: &Self) -> f32 {
        (*self - *other).abs()
    }
}

impl<const N: usize> Distance for [f32; N] {
    fn distance(&self, other: &Self) -> f32 {
        let mut discriminant = 0.0;
        for (a, b) in self.iter().zip(other.iter()) {
            discriminant += (a - b).powi(2);
        }
        discriminant.sqrt()
    }
}

impl Distance for Vec2 {
    fn distance(&self, other: &Self) -> f32 {
        (self - other).magnitude()
    }
}

impl Distance for Vec3 {
    fn distance(&self, other: &Self) -> f32 {
        (self - other).magnitude()
    }
}

impl Distance for Vec4 {
    fn distance(&self, other: &Self) -> f32 {
        (self - other).magnitude()
    }
}

#[cfg(test)]
mod tests;
