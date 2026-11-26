#[test]
fn rotations_follow_the_editors_fixed_xyz_euler_convention() {
    let actual = super::rotation([90.0, 90.0, 0.0]);
    for (actual, expected) in actual.into_iter().zip([0.5, 0.5, -0.5, 0.5]) {
        assert!((actual - expected).abs() < 1e-12);
    }
}
