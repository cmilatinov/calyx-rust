use super::{component_matches_search, menu_groups, MenuEntry};
use uuid::Uuid;

fn entry(name: &'static str, category: Option<&'static str>) -> MenuEntry {
    MenuEntry {
        type_uuid: Uuid::new_v4(),
        name,
        category,
    }
}

fn names(
    groups: &[(Option<&'static str>, Vec<MenuEntry>)],
) -> Vec<(Option<&'static str>, Vec<&'static str>)> {
    groups
        .iter()
        .map(|(category, entries)| (*category, entries.iter().map(|entry| entry.name).collect()))
        .collect()
}

#[test]
fn the_menu_lists_uncategorised_components_first_then_each_category() {
    let post = Some("Post Processing");
    let entries = [
        entry("Bloom", post),
        entry("Camera", None),
        entry("Global Post Process Volume", post),
        entry("Animator", None),
        entry("Tone Mapping", post),
    ];
    assert_eq!(
        names(&menu_groups(entries, "")),
        [
            (None, vec!["Animator", "Camera"]),
            (
                post,
                vec!["Bloom", "Global Post Process Volume", "Tone Mapping"]
            ),
        ]
    );
    assert_eq!(
        names(&menu_groups(entries, "bloom")),
        [(post, vec!["Bloom"])],
        "a group the search empties is dropped"
    );
    assert!(menu_groups(entries, "rigid").is_empty());
}

#[test]
fn component_search_matches_case_insensitive_substrings() {
    assert!(component_matches_search("Tank Controller", "tank"));
    assert!(component_matches_search("Directional Light", "LIGHT"));
    assert!(!component_matches_search("Component Mesh", "camera"));
}

#[test]
fn component_search_treats_blank_query_as_match() {
    assert!(component_matches_search("Camera", ""));
    assert!(component_matches_search("Camera", "   "));
}
