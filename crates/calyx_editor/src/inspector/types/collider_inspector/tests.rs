use super::group_mask_text;

#[test]
fn group_mask_text_names_groups_from_one() {
    assert_eq!(group_mask_text(u32::MAX), "All");
    assert_eq!(group_mask_text(0), "None");
    assert_eq!(group_mask_text(0b101), "1, 3");
    assert_eq!(group_mask_text(1 << 31), "32");
}
