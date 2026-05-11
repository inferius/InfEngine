//! 3D physics, built on `rapier3d`.
//!
//! Phase 7.1 ships a handle-based API independent of the scene model:
//! callers register rigid bodies via [`PhysicsWorld3D::create_body`]
//! and read updated translations + rotations after each [`step`]. The
//! scene-driven path lands once the scene model gains 3D entities.

use std::collections::HashMap;

use glam::{Quat, Vec3};
use rapier3d::na::{UnitQuaternion, Vector3};
use rapier3d::prelude::*;

/// 3D body kind. Mirrors rapier's `RigidBodyType` minus options the
/// engine does not need yet.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BodyKind3D {
    Dynamic,
    Static,
    Kinematic,
}

/// Collider shape for the 3D world.
#[derive(Debug, Copy, Clone)]
pub enum ColliderShape3D {
    /// Axis-aligned box with full-extent half-sizes.
    Box { half_extents: [f32; 3] },
    /// Sphere with radius.
    Sphere { radius: f32 },
    /// Capsule along the Y axis. `half_height` excludes the hemispherical caps.
    Capsule { half_height: f32, radius: f32 },
}

/// Opaque handle returned by [`PhysicsWorld3D::create_body`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct BodyId(u32);

/// Description of a body to create.
#[derive(Debug, Clone, Copy)]
pub struct BodyDesc {
    pub kind: BodyKind3D,
    pub shape: ColliderShape3D,
    pub position: Vec3,
    pub rotation: Quat,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub restitution: f32,
    pub friction: f32,
}

impl Default for BodyDesc {
    fn default() -> Self {
        Self {
            kind: BodyKind3D::Dynamic,
            shape: ColliderShape3D::Box {
                half_extents: [0.5; 3],
            },
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            linear_damping: 0.0,
            angular_damping: 0.05,
            restitution: 0.3,
            friction: 0.5,
        }
    }
}

struct BodyLink3D {
    body: RigidBodyHandle,
}

/// 3D physics world wrapping `rapier3d`.
pub struct PhysicsWorld3D {
    gravity: Vector3<f32>,
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
    bodies: HashMap<BodyId, BodyLink3D>,
    next_id: u32,
}

impl std::fmt::Debug for PhysicsWorld3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsWorld3D")
            .field("bodies", &self.rigid_body_set.len())
            .field("colliders", &self.collider_set.len())
            .finish()
    }
}

impl Default for PhysicsWorld3D {
    fn default() -> Self {
        Self::new()
    }
}

impl PhysicsWorld3D {
    pub fn new() -> Self {
        Self {
            gravity: Vector3::new(0.0, -9.81, 0.0),
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
            bodies: HashMap::new(),
            next_id: 0,
        }
    }

    pub fn set_gravity(&mut self, gravity: Vec3) {
        self.gravity = Vector3::new(gravity.x, gravity.y, gravity.z);
    }

    pub fn create_body(&mut self, desc: BodyDesc) -> BodyId {
        let mut builder = match desc.kind {
            BodyKind3D::Dynamic => RigidBodyBuilder::dynamic(),
            BodyKind3D::Static => RigidBodyBuilder::fixed(),
            BodyKind3D::Kinematic => RigidBodyBuilder::kinematic_position_based(),
        };
        let q = desc.rotation.normalize();
        builder = builder
            .translation(Vector3::new(
                desc.position.x,
                desc.position.y,
                desc.position.z,
            ))
            .rotation(
                UnitQuaternion::new_normalize(rapier3d::na::Quaternion::new(q.w, q.x, q.y, q.z))
                    .scaled_axis(),
            )
            .linear_damping(desc.linear_damping)
            .angular_damping(desc.angular_damping);
        let body_handle = self.rigid_body_set.insert(builder.build());

        let collider = match desc.shape {
            ColliderShape3D::Box { half_extents } => {
                ColliderBuilder::cuboid(half_extents[0], half_extents[1], half_extents[2])
            }
            ColliderShape3D::Sphere { radius } => ColliderBuilder::ball(radius),
            ColliderShape3D::Capsule {
                half_height,
                radius,
            } => ColliderBuilder::capsule_y(half_height, radius),
        }
        .restitution(desc.restitution)
        .friction(desc.friction)
        .build();
        let _ =
            self.collider_set
                .insert_with_parent(collider, body_handle, &mut self.rigid_body_set);

        let id = BodyId(self.next_id);
        self.next_id += 1;
        self.bodies.insert(id, BodyLink3D { body: body_handle });
        id
    }

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

    pub fn translation(&self, id: BodyId) -> Option<Vec3> {
        let link = self.bodies.get(&id)?;
        let body = self.rigid_body_set.get(link.body)?;
        let t = body.translation();
        Some(Vec3::new(t.x, t.y, t.z))
    }

    pub fn rotation(&self, id: BodyId) -> Option<Quat> {
        let link = self.bodies.get(&id)?;
        let body = self.rigid_body_set.get(link.body)?;
        let r = body.rotation();
        let q = r.quaternion();
        Some(Quat::from_xyzw(q.i, q.j, q.k, q.w))
    }

    pub fn set_translation(&mut self, id: BodyId, position: Vec3) {
        let Some(link) = self.bodies.get(&id) else {
            return;
        };
        let Some(body) = self.rigid_body_set.get_mut(link.body) else {
            return;
        };
        body.set_translation(Vector3::new(position.x, position.y, position.z), true);
    }

    pub fn body_count(&self) -> usize {
        self.rigid_body_set.len()
    }
}
