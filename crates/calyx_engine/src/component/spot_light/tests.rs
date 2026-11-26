use super::*;

#[test]
fn cone_angles_keep_inner_within_outer_and_both_within_the_maximum() {
    let light = |inner_angle, outer_angle| ComponentSpotLight {
        inner_angle,
        outer_angle,
        ..Default::default()
    };
    assert_eq!(light(20.0, 30.0).cone_angles(), (20.0, 30.0));
    assert_eq!(light(40.0, 30.0).cone_angles(), (30.0, 30.0));
    assert_eq!(light(-5.0, 120.0).cone_angles(), (0.0, 89.0));
    assert_eq!(light(f32::NAN, f32::INFINITY).cone_angles(), (0.0, 0.0));
}
