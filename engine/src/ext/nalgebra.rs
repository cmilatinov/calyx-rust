use nalgebra::{RealField, SimdValue, UnitQuaternion, Vector2};

pub trait UnitQuaternionExt<T> {
    fn pitch_yaw(&self) -> Vector2<T>;
}

impl<T: SimdValue + RealField> UnitQuaternionExt<T> for UnitQuaternion<T> {
    fn pitch_yaw(&self) -> Vector2<T> {
        let (mut pitch, mut yaw, _) = self.euler_angles();
        if pitch < nalgebra::convert::<f64, T>(-90.0f64.to_radians()) {
            pitch = nalgebra::convert::<f64, T>(std::f64::consts::PI) + pitch;
            yaw = nalgebra::convert::<f64, T>(std::f64::consts::PI) - yaw;
        } else if pitch > nalgebra::convert::<f64, T>(90.0f64.to_radians()) {
            pitch = pitch - nalgebra::convert::<f64, T>(std::f64::consts::PI);
            yaw = nalgebra::convert::<f64, T>(std::f64::consts::PI) - yaw;
        }
        Vector2::new(pitch, yaw)
    }
}
