//! Short-lived firing effects. Emitters live outside the projectile's
//! hierarchy so smoke already emitted can drain after a hit or expiry.
use calyx_engine::assets::{texture::Texture, AssetRef};
use calyx_engine::component::{
    Component, ComponentEventContext, ComponentID, ComponentMesh, ComponentParticleSystem,
    ComponentPointLight, ComponentRef, ComponentUpdate, ParticleBillboard, ParticleBlendMode,
    ParticleColorOverLifetime, ParticleEmission, ParticleForces, ParticleScalarRange,
    ParticleSizeOverLifetime, ParticleSpawnShape, ParticleVelocity, ReflectComponent,
    ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::math::{Gradient, Transform};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, GameObjectRef, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};
use serde::{Deserialize, Serialize};

const SMOKE_LIFETIME: f32 = 0.45;

/// The map's combat signals use the same palette as its look-dev rig.
#[derive(Clone, Copy, Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "0c190930-9a40-49c8-84a0-c35a48c57c0e"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Combat Palette")]
#[serde(default)]
pub struct ComponentCombatPalette {
    pub sunbaked: bool,
}

impl Component for ComponentCombatPalette {}

impl ComponentCombatPalette {
    fn in_scene(scene: &Scene) -> Self {
        scene
            .objects()
            .find_map(|object| scene.read_component::<Self, _, _>(object, |palette| *palette))
            .unwrap_or_default()
    }

    fn material(self) -> &'static str {
        if self.sunbaked {
            "b8f3abdd-57c2-56a0-8178-3aae785d9351"
        } else {
            "38769fe0-eb3a-543a-bfc5-9ad1ce971c3d"
        }
    }

    fn intensity(self) -> f32 {
        if self.sunbaked {
            40.0
        } else {
            30.0
        }
    }

    fn color(self) -> ParticleColorOverLifetime {
        let rgb = if self.sunbaked {
            [1.0, 0.46208, 0.06301]
        } else {
            [1.0, 0.43415, 0.0]
        };
        ParticleColorOverLifetime {
            gradient: Gradient::linear(
                [rgb[0], rgb[1], rgb[2], 1.0],
                [rgb[0], rgb[1], rgb[2], 0.0],
            ),
        }
    }
}

/// Deletes a transient emitter after emission and its final particles end.
#[derive(Clone, Copy, Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "f6245bf4-8c20-4cb9-a352-1b1389029a9c"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Combat Effect Lifetime")]
#[serde(default)]
pub struct ComponentCombatEffectLifetime {
    pub remaining: f32,
    pub follow: GameObjectRef,
}

impl Component for ComponentCombatEffectLifetime {}

impl ComponentUpdate for ComponentCombatEffectLifetime {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext { scene, .. }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        update_effect(scene, this.game_object(), resources.time().delta_time());
    }
}

fn update_effect(scene: &mut Scene, object: GameObject, dt: f32) {
    let Some(mut effect) =
        scene.read_component::<ComponentCombatEffectLifetime, _, _>(object, |e| *e)
    else {
        return;
    };
    if !effect.follow.id().is_nil() {
        if let Some(source) = effect.follow.game_object(scene) {
            let position = scene.world_transform(source).position;
            scene.set_world_transform(
                object,
                Transform::from_xyz(position.x, position.y, position.z).matrix(),
            );
        } else {
            // Keep world-space particles alive, with no more emission.
            scene.write_component::<ComponentParticleSystem, _>(object, |system| {
                system.active = false
            });
            effect.follow = Default::default();
            effect.remaining = effect.remaining.min(SMOKE_LIFETIME);
        }
    }
    effect.remaining -= dt.max(0.0);
    if effect.remaining <= 0.0 {
        scene.delete(object);
    } else {
        scene.write_component::<ComponentCombatEffectLifetime, _>(object, |e| *e = effect);
    }
}

fn shell_visual(palette: ComponentCombatPalette) -> ComponentMesh {
    // The same sphere and emissive material as the approved look-dev rounds.
    ComponentMesh {
        mesh: AssetRef::from_id(
            "52c056b4-6dfb-1859-6023-23693f3ff263"
                .parse()
                .expect("shell mesh id"),
        ),
        material: AssetRef::from_id(palette.material().parse().expect("shell material id")),
        cast_shadows: false,
    }
}

pub(crate) fn spawn_round_visual(scene: &mut Scene, projectile: GameObject) {
    // Keep the physics transform and radius untouched. Visual dimensions are
    // in metres, exactly as authored in tank_direction.cxscene, independently
    // of the firing tank's collision radius.
    let parent_scale = scene.world_transform(projectile).scale.x.max(f32::EPSILON);
    let palette = ComponentCombatPalette::in_scene(scene);
    for (name, position, scale) in [
        ("Round", Vec3::zeros(), vec3(0.2, 0.2, 0.2)),
        ("Luminous Tail", vec3(0.0, 0.0, -0.8), vec3(0.09, 0.09, 0.8)),
    ] {
        let part = scene.create(
            Some(ComponentID {
                name: name.into(),
                ..Default::default()
            }),
            Some(projectile),
        );
        scene.set_transform(
            part,
            &Transform::from_components(
                position / parent_scale,
                UnitQuaternion::identity(),
                scale / parent_scale,
            )
            .matrix(),
        );
        scene.add_component(part, shell_visual(palette));
    }
    scene.add_component(projectile, signal_light(palette, 1.5, 3.0));
}

fn signal_light(
    palette: ComponentCombatPalette,
    intensity: f32,
    radius: f32,
) -> ComponentPointLight {
    ComponentPointLight {
        color: if palette.sunbaked {
            Color32::from_rgb(255, 181, 71)
        } else {
            Color32::from_rgb(255, 176, 0)
        },
        intensity,
        radius,
    }
}

fn puff_texture() -> AssetRef<Texture> {
    AssetRef::from_id(
        "7a2bd53d-83b7-4821-a54e-1fa6586225d4"
            .parse()
            .expect("shared puff texture id"),
    )
}

fn emitter(
    scene: &mut Scene,
    name: &str,
    position: Vec3,
    system: ComponentParticleSystem,
    lifetime: f32,
    follow: GameObjectRef,
) -> GameObject {
    let object = scene.create(
        Some(ComponentID {
            name: name.into(),
            ..Default::default()
        }),
        None,
    );
    scene.set_world_transform(
        object,
        Transform::from_xyz(position.x, position.y, position.z).matrix(),
    );
    scene.add_component(object, system);
    scene.add_component(
        object,
        ComponentCombatEffectLifetime {
            remaining: lifetime,
            follow,
        },
    );
    object
}

fn burst(count: u32, lifetime: f32) -> ComponentParticleSystem {
    let mut system = ComponentParticleSystem::default();
    system.emission = ParticleEmission {
        looping: false,
        rate: 0.0,
        burst_count: count,
        max_particles: count,
        duration: 0.01,
    };
    system.shape = ParticleSpawnShape::Sphere { radius: 0.08 };
    system.lifetime = ParticleScalarRange {
        min: lifetime * 0.65,
        max: lifetime,
    };
    system.forces = ParticleForces {
        acceleration: Vec3::zeros(),
    };
    system.rendering.texture = puff_texture();
    system
}

pub(crate) fn spawn_shot(
    scene: &mut Scene,
    projectile: GameObject,
    position: Vec3,
    rotation: UnitQuaternion<f32>,
    flight_time: f32,
) {
    let palette = ComponentCombatPalette::in_scene(scene);
    let mut flash = burst(1, 0.1);
    flash.shape = ParticleSpawnShape::Sphere { radius: 0.0 };
    flash.lifetime = ParticleScalarRange { min: 0.1, max: 0.1 };
    flash.velocity = ParticleVelocity {
        initial: Vec3::zeros(),
        randomness: Vec3::zeros(),
    };
    flash.size = ParticleSizeOverLifetime::linear(0.64, 0.32, 0.0);
    flash.color = palette.color();
    flash.rendering.texture = Default::default();
    flash.rendering.intensity = palette.intensity();
    flash.rendering.min_screen_size = 0.0;
    flash.rendering.billboard = ParticleBillboard::Directional {
        direction: rotation * vec3(0.0, 0.0, 1.0),
        length_scale: 1.5625,
    };
    let muzzle = emitter(
        scene,
        "Muzzle Flash",
        position + rotation * vec3(0.0, 0.0, 0.25),
        flash,
        0.1,
        Default::default(),
    );
    scene.add_component(muzzle, signal_light(palette, 3.0, 4.0));

    let mut smoke = burst(1, SMOKE_LIFETIME);
    smoke.emission = ParticleEmission {
        looping: false,
        rate: 40.0,
        burst_count: 1,
        max_particles: 24,
        duration: flight_time.max(0.0),
    };
    smoke.shape = ParticleSpawnShape::Sphere { radius: 0.045 };
    smoke.velocity = ParticleVelocity {
        initial: vec3(0.0, 0.15, 0.0),
        randomness: vec3(0.12, 0.12, 0.12),
    };
    smoke.size = ParticleSizeOverLifetime::linear(0.10, 0.32, 0.025);
    smoke.color = ParticleColorOverLifetime::linear(
        Color32::from_rgba_unmultiplied(170, 164, 148, 65),
        Color32::from_rgba_unmultiplied(170, 164, 148, 0),
    );
    smoke.rendering.blend_mode = ParticleBlendMode::Alpha;
    smoke.rendering.min_screen_size = 0.0;
    let source = GameObjectRef::new(scene.uuid(projectile));
    emitter(
        scene,
        "Shell Smoke Trail",
        position,
        smoke,
        flight_time + SMOKE_LIFETIME,
        source,
    );
}

pub(crate) fn spawn_impact(scene: &mut Scene, position: Vec3, tank: bool) {
    let lifetime = if tank { 0.32 } else { 0.55 };
    let mut system = burst(if tank { 14 } else { 10 }, lifetime);
    system.velocity = ParticleVelocity {
        initial: vec3(0.0, 1.2, 0.0),
        randomness: if tank {
            vec3(3.0, 2.0, 3.0)
        } else {
            vec3(1.4, 0.9, 1.4)
        },
    };
    system.forces.acceleration = if tank {
        vec3(0.0, -5.0, 0.0)
    } else {
        vec3(0.0, 0.4, 0.0)
    };
    system.size = if tank {
        ParticleSizeOverLifetime::linear(0.25, 0.0, 0.06)
    } else {
        ParticleSizeOverLifetime::linear(0.3, 0.95, 0.1)
    };
    system.color = if tank {
        ParticleColorOverLifetime::linear(Color32::from_rgb(255, 185, 55), Color32::TRANSPARENT)
    } else {
        ParticleColorOverLifetime::linear(
            Color32::from_rgba_unmultiplied(191, 179, 153, 190),
            Color32::from_rgba_unmultiplied(191, 179, 153, 0),
        )
    };
    system.rendering.blend_mode = if tank {
        ParticleBlendMode::Additive
    } else {
        ParticleBlendMode::Alpha
    };
    system.rendering.min_screen_size = 0.0;
    if tank {
        system.rendering.texture = Default::default();
        system.rendering.intensity = 12.0;
        system.rendering.billboard = ParticleBillboard::VelocityAligned { length_scale: 4.0 };
        system.size = ParticleSizeOverLifetime::linear(0.09, 0.0, 0.025);
        system.color = ComponentCombatPalette::in_scene(scene).color();
    }
    emitter(
        scene,
        if tank {
            "Tank Impact Sparks"
        } else {
            "Cover Impact Dust"
        },
        position,
        system,
        lifetime + 0.05,
        Default::default(),
    );
}

#[cfg(test)]
mod tests;
