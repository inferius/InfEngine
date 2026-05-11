//! 2D physics integration for InfEngine.
//!
//! Phase 5.6 wraps `rapier2d` behind a `PhysicsWorld` that consumes the
//! engine's `Scene` model. Entities with a `PhysicsComponent` get a
//! matching rigid body and collider; calling `step` advances the
//! simulation, and `write_back` copies the updated positions into the
//! scene's `Transform2D` for the renderer to pick up.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use infengine_core::scene::{BodyKind, ColliderShape, Entity, EntityId, PhysicsComponent, Scene};
use rapier2d::na::Vector2;
use rapier2d::prelude::*;
use tracing::warn;

/// Per-entity link between a scene id and rapier's handles.
#[derive(Debug, Clone, Copy)]
struct BodyLink {
    body: RigidBodyHandle,
}

/// 2D physics world. Re-build it after structural scene changes
/// (entity add / remove / physics component toggle) via [`rebuild`].
pub struct PhysicsWorld {
    gravity: Vector2<f32>,
    integration_parameters: IntegrationParameters,
    physics_pipeline: PhysicsPipeline,
    island_manager: IslandManager,
    broad_phase: DefaultBroadPhase,
    narrow_phase: NarrowPhase,
    rigid_body_set: RigidBodySet,
    collider_set: ColliderSet,
    impulse_joint_set: ImpulseJointSet,
    multibody_joint_set: MultibodyJointSet,
    ccd_solver: CCDSolver,
    links: HashMap<EntityId, BodyLink>,
}

impl std::fmt::Debug for PhysicsWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsWorld")
            .field("bodies", &self.rigid_body_set.len())
            .field("colliders", &self.collider_set.len())
            .finish()
    }
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            gravity: Vector2::new(0.0, -1200.0),
            integration_parameters: IntegrationParameters::default(),
            physics_pipeline: PhysicsPipeline::new(),
            island_manager: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            rigid_body_set: RigidBodySet::new(),
            collider_set: ColliderSet::new(),
            impulse_joint_set: ImpulseJointSet::new(),
            multibody_joint_set: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            links: HashMap::new(),
        }
    }

    /// Drop every body / collider and re-create them from the scene's
    /// current entity list. Cheap enough for editor use; revisit if the
    /// scene grows to thousands of bodies.
    pub fn rebuild(&mut self, scene: &Scene) {
        self.gravity = Vector2::new(scene.gravity[0], scene.gravity[1]);
        self.rigid_body_set = RigidBodySet::new();
        self.collider_set = ColliderSet::new();
        self.impulse_joint_set = ImpulseJointSet::new();
        self.multibody_joint_set = MultibodyJointSet::new();
        self.island_manager = IslandManager::new();
        self.broad_phase = DefaultBroadPhase::new();
        self.narrow_phase = NarrowPhase::new();
        self.links.clear();

        for entity in &scene.entities {
            if let Some(physics) = &entity.physics {
                self.insert_entity(entity, physics);
            }
        }
    }

    fn insert_entity(&mut self, entity: &Entity, physics: &PhysicsComponent) {
        let pos = entity.transform.position;
        let mut builder = match physics.body {
            BodyKind::Dynamic => RigidBodyBuilder::dynamic(),
            BodyKind::Static => RigidBodyBuilder::fixed(),
            BodyKind::Kinematic => RigidBodyBuilder::kinematic_position_based(),
        };
        builder = builder
            .translation(Vector2::new(pos[0], pos[1]))
            .linear_damping(physics.linear_damping);
        let body_handle = self.rigid_body_set.insert(builder.build());

        let (sprite_w, sprite_h) = match &entity.sprite {
            Some(s) => (
                s.size[0] * entity.transform.scale[0],
                s.size[1] * entity.transform.scale[1],
            ),
            None => (32.0, 32.0),
        };
        let collider = match physics.shape {
            ColliderShape::Box => ColliderBuilder::cuboid(sprite_w * 0.5, sprite_h * 0.5),
            ColliderShape::Circle { radius } => ColliderBuilder::ball(radius),
        }
        .restitution(physics.restitution)
        .friction(physics.friction)
        .build();
        let _ =
            self.collider_set
                .insert_with_parent(collider, body_handle, &mut self.rigid_body_set);

        self.links.insert(entity.id, BodyLink { body: body_handle });
    }

    /// Step the simulation by `dt` seconds. `dt` is clamped to a small
    /// upper bound to keep behavior stable across hitching frames.
    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);
        self.integration_parameters.dt = dt;
        self.physics_pipeline.step(
            &self.gravity,
            &self.integration_parameters,
            &mut self.island_manager,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.rigid_body_set,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            &mut self.ccd_solver,
            &(),
            &(),
        );
    }

    /// Copy world-driven positions back into scene transforms. Static
    /// bodies are left alone so the editor's authored position wins.
    pub fn write_back(&self, scene: &mut Scene) {
        for (id, link) in &self.links {
            let Some(entity) = scene.get_mut(*id) else {
                continue;
            };
            let Some(body) = self.rigid_body_set.get(link.body) else {
                continue;
            };
            if matches!(
                entity.physics.as_ref().map(|p| p.body),
                Some(BodyKind::Static)
            ) {
                continue;
            }
            let t = body.translation();
            entity.transform.position = [t.x, t.y];
        }
    }

    /// Push a teleport / authored position from the editor into the
    /// simulation (call between [`rebuild`] cycles when the user drags
    /// the entity or edits its transform).
    pub fn sync_from_scene(&mut self, scene: &Scene) {
        for (id, link) in &self.links {
            let Some(entity) = scene.get(*id) else {
                continue;
            };
            let Some(body) = self.rigid_body_set.get_mut(link.body) else {
                warn!("missing body for {:?}", id);
                continue;
            };
            body.set_translation(
                Vector2::new(entity.transform.position[0], entity.transform.position[1]),
                true,
            );
        }
    }

    pub fn body_count(&self) -> usize {
        self.rigid_body_set.len()
    }
}
