//! Short-lived firing effects. Emitters live outside the projectile's
//! hierarchy so smoke already emitted can drain after a hit or expiry.
use calyx_engine::assets::{texture::Texture, AssetRef};
use calyx_engine::component::{
    Component, ComponentEventContext, ComponentID, ComponentMesh, ComponentParticleSystem,
    ComponentRef, ComponentUpdate, ParticleBlendMode, ParticleColorOverLifetime, ParticleEmission,
    ParticleForces, ParticleScalarRange, ParticleSizeOverLifetime, ParticleSpawnShape,
    ParticleVelocity, ReflectComponent, ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, GameObjectRef, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};
use serde::{Deserialize, Serialize};

const SMOKE_LIFETIME: f32 = 0.45;

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

pub(crate) fn shell_visual() -> ComponentMesh {
    // The stable ids in game/{meshes,materials}/shell.meta. AssetRef resolves
    // them through the view's registry, as authored component references do.
    ComponentMesh {
        mesh: AssetRef::from_id(
            "d0a185fe-7d04-5130-929e-c01d1b36786b"
                .parse()
                .expect("shell mesh id"),
        ),
        material: AssetRef::from_id(
            "57b8503a-cc31-5e36-b9af-757c1c346681"
                .parse()
                .expect("shell material id"),
        ),
        cast_shadows: false,
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
    let mut flash = burst(7, 0.12);
    flash.velocity = ParticleVelocity {
        initial: rotation * vec3(0.0, 0.0, 2.0),
        randomness: vec3(0.6, 0.6, 0.6),
    };
    flash.size = ParticleSizeOverLifetime::linear(0.85, 0.0, 0.2);
    flash.color =
        ParticleColorOverLifetime::linear(Color32::from_rgb(255, 220, 130), Color32::TRANSPARENT);
    emitter(
        scene,
        "Muzzle Flash",
        position,
        flash,
        0.15,
        Default::default(),
    );

    let mut smoke = burst(1, SMOKE_LIFETIME);
    smoke.emission = ParticleEmission {
        looping: false,
        rate: 80.0,
        burst_count: 1,
        max_particles: 40,
        duration: flight_time.max(0.0),
    };
    smoke.shape = ParticleSpawnShape::Sphere { radius: 0.045 };
    smoke.velocity = ParticleVelocity {
        initial: vec3(0.0, 0.15, 0.0),
        randomness: vec3(0.12, 0.12, 0.12),
    };
    smoke.size = ParticleSizeOverLifetime::linear(0.15, 0.55, 0.035);
    smoke.color = ParticleColorOverLifetime::linear(
        Color32::from_rgba_unmultiplied(170, 164, 148, 165),
        Color32::from_rgba_unmultiplied(170, 164, 148, 0),
    );
    smoke.rendering.blend_mode = ParticleBlendMode::Alpha;
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
