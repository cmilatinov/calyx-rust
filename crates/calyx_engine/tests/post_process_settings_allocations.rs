use calyx_engine::assets::post_process_profile::{PostProcessProfile, ProfileEntry};
use calyx_engine::assets::AssetRef;
use calyx_engine::component::{
    Component, ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom,
    ComponentColorGrading, ComponentDepthOfField, ComponentFog, ComponentGlobalPostProcessVolume,
    ComponentLocalPostProcessVolume, ComponentOutline, ComponentTonemap, ComponentVignette,
    LocalVolumeShape, OutlineStyle, PostProcessEffect, ReflectComponent, ReflectPostProcessEffect,
    MAX_OUTLINE_STYLES,
};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::render::postprocess::{BlendEffect, PostProcessSettings};
use calyx_engine::test_support::test_registries;
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::{vec3, Vec3};
use serde::{Deserialize, Serialize};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountAllocations;

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

fn count_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}

// The counter is thread-local so background GPU work and other tests cannot
// affect the assertion about the resolver on this thread.
unsafe impl GlobalAlloc for CountAllocations {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count_allocation();
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountAllocations = CountAllocations;

fn allocations(run: impl FnOnce()) -> usize {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ALLOCATIONS.with(|count| count.set(None));
        }
    }
    ALLOCATIONS.with(|count| count.set(Some(0)));
    let _reset = Reset;
    run();
    ALLOCATIONS.with(|count| count.get().unwrap())
}

// This effect is registered outside calyx_engine. No gather/profile/settings
// code names it: the same API works for effects a project adds.
#[derive(
    Clone, Default, Serialize, Deserialize, TypeUuid, Component, Reflect, PostProcessEffect,
)]
#[uuid = "9ab238b2-d7ef-42e7-8b87-448484ab4ea6"]
#[reflect(Default, TypeUuidDynamic, Component, PostProcessEffect)]
struct TestEffect {
    strength: f32,
}
impl Component for TestEffect {
    fn can_disable(&self) -> bool {
        true
    }
}
impl BlendEffect for TestEffect {
    fn blend(&self, target: &Self, factor: f32) -> Self {
        Self {
            strength: self.strength * (1.0 - factor) + target.strength * factor,
        }
    }
    fn neutral(_: &Self) -> Option<Self> {
        Some(Self::default())
    }
}

#[test]
fn warmed_resolution_allocates_nothing_across_camera_and_style_changes() {
    let registries = test_registries();
    let profile = registries
        .assets
        .read()
        .create(
            "allocation-profile".into(),
            PostProcessProfile::from_entries([
                ProfileEntry::new(ComponentTonemap::default()),
                ProfileEntry::new(ComponentAmbientOcclusion::default()),
                ProfileEntry::new(ComponentAntiAliasing::default()),
                ProfileEntry::new(ComponentBloom::default()),
                ProfileEntry::new(ComponentColorGrading::default()),
                ProfileEntry::new(ComponentDepthOfField::default()),
                ProfileEntry::new(ComponentFog::default()),
                ProfileEntry::new(ComponentVignette::default()),
                ProfileEntry::new(ComponentOutline {
                    styles: vec![OutlineStyle::default()],
                    ..Default::default()
                }),
                ProfileEntry::new(TestEffect { strength: 2.0 }),
            ]),
        )
        .unwrap();
    let mut scene = registries.scene();
    // Warm warning tracking too: both the global selection and local ordering
    // see ties, so formatting a warning key per frame would fail this test.
    for _ in 0..2 {
        let object = scene.create(None, None);
        scene.add_component(
            object,
            ComponentGlobalPostProcessVolume {
                priority: 7,
                profile: AssetRef::from_id(profile.id()),
            },
        );
    }
    let mut locals = Vec::new();
    // Enough local volumes to exercise sorting beyond its tiny-slice path.
    // Only two broad volumes contribute at warm-up (also warming the tie
    // warning); entering all 64 must reuse the same scratch allocation.
    for index in 0..64 {
        let object = scene.create(None, None);
        scene.add_component(
            object,
            ComponentLocalPostProcessVolume {
                shape: LocalVolumeShape::Sphere {
                    radius: if index < 2 { 200.0 } else { 5.0 },
                },
                weight: 0.5,
                priority: 3,
                profile: AssetRef::from_id(profile.id()),
                ..Default::default()
            },
        );
        scene.add_component(
            object,
            ComponentBloom {
                intensity: 0.6,
                ..Default::default()
            },
        );
        locals.push(object);
    }
    let mut settings = PostProcessSettings::default();
    settings.resolve_into(&scene, &vec3(100.0, 0.0, 0.0));
    assert_eq!(
        allocations(|| settings.resolve_into(&scene, &Vec3::zeros())),
        0
    );
    let effect_address = settings.get::<ComponentOutline>().unwrap() as *const _;
    let styles_address = settings.get::<ComponentOutline>().unwrap().styles.as_ptr();
    assert_eq!(settings.get::<TestEffect>().unwrap().strength, 2.0);
    assert!((settings.get::<ComponentBloom>().unwrap().intensity - 0.6).abs() < 1e-5);

    for size in [MAX_OUTLINE_STYLES, 2, 0, MAX_OUTLINE_STYLES, 1] {
        // Authoring writes are deliberately outside the measurement. The
        // resolver first saw one style, never an eight-style source.
        profile
            .write()
            .entry_mut(ComponentOutline::type_uuid())
            .unwrap()
            .settings
            .as_any_mut()
            .downcast_mut::<ComponentOutline>()
            .unwrap()
            .styles
            .resize(size, OutlineStyle::default());
        let count = allocations(|| {
            for position in [vec3(100.0, 0.0, 0.0), Vec3::zeros(), vec3(6.0, 0.0, 0.0)] {
                settings.resolve_into(&scene, &position);
            }
        });
        assert_eq!(
            count, 0,
            "resolving after editing the style count to {size}"
        );
        let outline = settings.get::<ComponentOutline>().unwrap();
        assert_eq!(outline as *const _, effect_address);
        assert_eq!(outline.styles.as_ptr(), styles_address);
        assert_eq!(outline.styles.len(), size);
    }

    // The slots remain allocated while inactive, then accept the same types
    // from a changed scene configuration without exposing stale values.
    for object in scene.objects().collect::<Vec<_>>() {
        scene.set_component_enabled::<ComponentGlobalPostProcessVolume>(object, false);
        scene.set_component_enabled::<ComponentLocalPostProcessVolume>(object, false);
    }
    assert_eq!(
        allocations(|| settings.resolve_into(&scene, &Vec3::zeros())),
        0
    );
    assert!(settings.get::<ComponentOutline>().is_none());
    assert!(settings.get::<TestEffect>().is_none());
    assert_eq!(
        settings.get::<ComponentTonemap>(),
        Some(&ComponentTonemap::default())
    );
    scene.set_component_enabled::<ComponentLocalPostProcessVolume>(locals[0], true);
    assert_eq!(
        allocations(|| settings.resolve_into(&scene, &Vec3::zeros())),
        0
    );
    assert_eq!(settings.get::<TestEffect>().unwrap().strength, 1.0);
    assert_eq!(
        settings.get::<ComponentOutline>().unwrap() as *const _,
        effect_address
    );
}

#[test]
fn a_local_component_overrides_its_profile_before_blending() {
    let registries = test_registries();
    let profile = registries
        .assets
        .read()
        .create(
            "generic-profile".into(),
            PostProcessProfile::from_entries([ProfileEntry::new(TestEffect { strength: 8.0 })]),
        )
        .unwrap();
    let mut scene = registries.scene();
    let global = scene.create(None, None);
    scene.add_component(global, ComponentGlobalPostProcessVolume::default());
    scene.add_component(global, TestEffect { strength: 2.0 });
    let local = scene.create(None, None);
    scene.add_component(
        local,
        ComponentLocalPostProcessVolume {
            weight: 0.5,
            profile: AssetRef::from_id(profile.id()),
            ..Default::default()
        },
    );
    scene.add_component(local, TestEffect { strength: 4.0 });
    let mut settings = PostProcessSettings::default();
    settings.resolve_into(&scene, &Vec3::zeros());
    assert_eq!(settings.get::<TestEffect>().unwrap().strength, 3.0);
    scene.set_component_enabled::<TestEffect>(local, false);
    settings.resolve_into(&scene, &Vec3::zeros());
    assert_eq!(settings.get::<TestEffect>().unwrap().strength, 5.0);
    settings.clear();
    assert!(settings.get::<TestEffect>().is_none());
    settings.resolve_into(&scene, &Vec3::zeros());
    assert_eq!(settings.get::<TestEffect>().unwrap().strength, 5.0);
}
