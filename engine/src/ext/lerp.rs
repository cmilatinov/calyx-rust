use crate::math::Transform;
use lerp::Lerp;

impl Lerp<f32> for Transform {
    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            position: self.position.lerp(other.position, t),
            rotation: self.rotation.slerp(&other.rotation, t),
            scale: self.scale.lerp(other.scale, t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use lerp::Lerp;

    #[test]
    fn lerp_at_zero_is_self() {
        let a = Transform::from_xyz(0.0, 0.0, 0.0);
        let b = Transform::from_xyz(10.0, 0.0, 0.0);
        let r = a.lerp(b, 0.0);
        assert_abs_diff_eq!(r.position, a.position, epsilon = 1e-5);
    }

    #[test]
    fn lerp_at_one_is_other() {
        let a = Transform::from_xyz(0.0, 0.0, 0.0);
        let b = Transform::from_xyz(10.0, 0.0, 0.0);
        let r = a.lerp(b, 1.0);
        assert_abs_diff_eq!(r.position, b.position, epsilon = 1e-5);
    }

    #[test]
    fn lerp_midpoint() {
        let a = Transform::from_xyz(0.0, 0.0, 0.0);
        let b = Transform::from_xyz(4.0, 0.0, 0.0);
        let r = a.lerp(b, 0.5);
        assert_abs_diff_eq!(r.position.x, 2.0, epsilon = 1e-5);
    }
}
