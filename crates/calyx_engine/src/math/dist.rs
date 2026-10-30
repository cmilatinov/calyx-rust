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
mod tests {
    use super::Distance;
    use approx::assert_abs_diff_eq;
    use nalgebra_glm::{vec2, vec3, vec4};

    #[test]
    fn f32_distance() {
        assert_abs_diff_eq!(3.0f32.distance(&7.0), 4.0);
        assert_abs_diff_eq!(7.0f32.distance(&3.0), 4.0);
        assert_abs_diff_eq!(0.0f32.distance(&0.0), 0.0);
    }

    #[test]
    fn array_distance() {
        assert_abs_diff_eq!([3.0f32, 4.0].distance(&[0.0, 0.0]), 5.0);
        assert_abs_diff_eq!([0.0f32; 3].distance(&[0.0; 3]), 0.0);
    }

    #[test]
    fn vec2_distance() {
        assert_abs_diff_eq!(vec2(0.0f32, 0.0).distance(&vec2(3.0, 4.0)), 5.0);
    }

    #[test]
    fn vec3_distance() {
        assert_abs_diff_eq!(vec3(1.0f32, 0.0, 0.0).distance(&vec3(4.0, 0.0, 0.0)), 3.0);
    }

    #[test]
    fn vec4_distance() {
        assert_abs_diff_eq!(
            vec4(0.0f32, 0.0, 0.0, 0.0).distance(&vec4(1.0, 0.0, 0.0, 0.0)),
            1.0
        );
    }
}
