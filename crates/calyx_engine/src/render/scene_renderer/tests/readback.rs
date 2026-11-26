use super::super::{half_to_f32, linear_to_srgb};

#[test]
fn half_floats_decode() {
    assert_eq!(half_to_f32(0x0000), 0.0);
    assert_eq!(half_to_f32(0x3c00), 1.0);
    assert_eq!(half_to_f32(0xbc00), -1.0);
    assert_eq!(half_to_f32(0x3800), 0.5);
    assert_eq!(half_to_f32(0x4248), 3.140625);
    // Smallest positive subnormal half.
    assert_eq!(half_to_f32(0x0001), 5.960_464_5e-8);
    assert!(half_to_f32(0x7c00).is_infinite());
    assert!(half_to_f32(0x7e00).is_nan());
}

#[test]
fn linear_values_encode_to_srgb_bytes() {
    assert_eq!(linear_to_srgb(0.0), 0);
    assert_eq!(linear_to_srgb(1.0), 255);
    assert_eq!(linear_to_srgb(2.5), 255);
    assert_eq!(linear_to_srgb(-1.0), 0);
    // Mid grey: linear 0.2158 is approximately sRGB 128.
    assert_eq!(linear_to_srgb(0.2158), 128);
}
