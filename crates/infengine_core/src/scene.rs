//! Scene + entity model with RON serialization.
//!
//! Phase 5.1 keeps the model deliberately small: a `Scene` holds an
//! ordered list of `Entity` records, each carrying a stable id, a name,
//! a 2D transform, and optional component data (sprite, tilemap ref).
//! Components grow in later phases (physics, audio, scripts, ...).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Stable numeric identifier of an entity inside a scene. Persisted in
/// the saved file so references survive reload.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId(pub u32);

/// Identifier of an atlas region the sprite renders. Mirrors the
/// `demo_atlas` regions exposed by `infengine_render`. Strings instead
/// of an enum so saved scenes survive renderer-side additions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtlasRegion {
    Full,
    Checker,
    Dots,
    Stripes,
    Cross,
}

impl AtlasRegion {
    /// Map to `(uv_min, uv_max)` consistent with the demo atlas. The
    /// numeric ratios match `infengine_render::demo_atlas` so changing
    /// either side requires updating the other.
    pub fn uv_rect(&self) -> ([f32; 2], [f32; 2]) {
        const HALF_V: f32 = 128.0 / 304.0;
        const FULL_V: f32 = 256.0 / 304.0;
        match self {
            AtlasRegion::Full => ([0.0, 0.0], [1.0, 1.0]),
            AtlasRegion::Checker => ([0.0, 0.0], [0.5, HALF_V]),
            AtlasRegion::Dots => ([0.5, 0.0], [1.0, HALF_V]),
            AtlasRegion::Stripes => ([0.0, HALF_V], [0.5, FULL_V]),
            AtlasRegion::Cross => ([0.5, HALF_V], [1.0, FULL_V]),
        }
    }
}

/// 2D transform: world position and a uniform-ish 2-axis scale. Rotation
/// lands in a later step once the sprite shader supports it.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Transform2D {
    pub position: [f32; 2],
    pub scale: [f32; 2],
}

impl Default for Transform2D {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0],
            scale: [1.0, 1.0],
        }
    }
}

/// 3D transform: position, Euler rotation (radians, XYZ order), and
/// per-axis scale. Rotations apply as Rz * Ry * Rx when building the
/// model matrix - matches the convention used by `glam::Mat4::from_euler`
/// with `EulerRot::XYZ`.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Transform3D {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for Transform3D {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

/// Mesh component for a 3D entity. `mesh` names a mesh handle registered
/// at runtime (`"cube"` is built into the renderer). `color` is the base
/// albedo tint multiplied with the sampled albedo texture in the
/// fragment shader. `metallic`/`roughness` are clamped to `[0, 1]` /
/// `[0.04, 1]` respectively.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshComponent {
    pub mesh: String,
    pub color: [f32; 4],
    #[serde(default = "default_metallic")]
    pub metallic: f32,
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    /// Optional albedo texture path (relative to the project root).
    /// The texture is bound to the mesh handle that backs this slot,
    /// so all entities referencing the same `mesh` name share the
    /// last-loaded texture for that handle.
    #[serde(default)]
    pub texture: Option<String>,
}

fn default_metallic() -> f32 {
    0.0
}

fn default_roughness() -> f32 {
    0.5
}

impl MeshComponent {
    pub fn cube(color: [f32; 4]) -> Self {
        Self {
            mesh: "cube".to_owned(),
            color,
            metallic: 0.0,
            roughness: 0.5,
            texture: None,
        }
    }
}

/// 3D body kind mirror, kept independent of `infengine_physics` so the
/// scene model has no rapier dependency.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind3D {
    Dynamic,
    Static,
    Kinematic,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColliderShape3D {
    Box { half_extents: [f32; 3] },
    Sphere { radius: f32 },
    Capsule { half_height: f32, radius: f32 },
}

/// 3D physics component: body kind + collider + material properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Physics3DComponent {
    pub body: BodyKind3D,
    pub shape: ColliderShape3D,
    #[serde(default)]
    pub linear_damping: f32,
    #[serde(default = "default_ang_damping")]
    pub angular_damping: f32,
    #[serde(default)]
    pub restitution: f32,
    #[serde(default = "default_friction")]
    pub friction: f32,
}

fn default_ang_damping() -> f32 {
    0.05
}

impl Physics3DComponent {
    pub fn dynamic_box() -> Self {
        Self {
            body: BodyKind3D::Dynamic,
            shape: ColliderShape3D::Box {
                half_extents: [0.5, 0.5, 0.5],
            },
            linear_damping: 0.0,
            angular_damping: 0.05,
            restitution: 0.3,
            friction: 0.5,
        }
    }

    pub fn static_box() -> Self {
        Self {
            body: BodyKind3D::Static,
            shape: ColliderShape3D::Box {
                half_extents: [0.5, 0.5, 0.5],
            },
            linear_damping: 0.0,
            angular_damping: 0.0,
            restitution: 0.0,
            friction: 0.7,
        }
    }
}

/// Visual data for a sprite entity. `size` is in world units; the final
/// rendered size is `size * transform.scale`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteComponent {
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub region: AtlasRegion,
}

impl SpriteComponent {
    pub fn new(size: [f32; 2], region: AtlasRegion) -> Self {
        Self {
            size,
            color: [1.0, 1.0, 1.0, 1.0],
            region,
        }
    }
}

/// Body kind for a physics-enabled entity. Mirrors rapier's
/// `RigidBodyType` minus options the engine does not need yet.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    /// Affected by gravity and forces.
    Dynamic,
    /// Immovable; collides with dynamic bodies.
    Static,
    /// Position is driven by the user; collides with dynamic bodies.
    Kinematic,
}

/// Simple collider shape. Box uses the sprite size; circle uses radius
/// directly. More shapes (capsule, polygon, edge) land later.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColliderShape {
    Box,
    Circle { radius: f32 },
}

/// Physics component: body kind + collider shape + mass-related params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsComponent {
    pub body: BodyKind,
    pub shape: ColliderShape,
    /// Linear damping; 0 means no resistance.
    #[serde(default)]
    pub linear_damping: f32,
    /// Restitution (bounciness) in `[0, 1]`.
    #[serde(default)]
    pub restitution: f32,
    /// Friction coefficient.
    #[serde(default = "default_friction")]
    pub friction: f32,
}

fn default_friction() -> f32 {
    0.5
}

impl PhysicsComponent {
    pub fn dynamic_box() -> Self {
        Self {
            body: BodyKind::Dynamic,
            shape: ColliderShape::Box,
            linear_damping: 0.0,
            restitution: 0.3,
            friction: 0.5,
        }
    }

    pub fn static_box() -> Self {
        Self {
            body: BodyKind::Static,
            shape: ColliderShape::Box,
            linear_damping: 0.0,
            restitution: 0.0,
            friction: 0.7,
        }
    }
}

/// One scene entity. Components live as `Option`al fields; if growth
/// becomes painful, migrate to a real ECS (bevy_ecs) without changing
/// the saved-file shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub transform: Transform2D,
    pub sprite: Option<SpriteComponent>,
    #[serde(default)]
    pub physics: Option<PhysicsComponent>,
    /// Optional 3D transform. Entities with this field set are treated
    /// as 3D objects and drawn via the mesh pipeline; the 2D `transform`
    /// is ignored for them.
    #[serde(default)]
    pub transform_3d: Option<Transform3D>,
    #[serde(default)]
    pub mesh: Option<MeshComponent>,
    #[serde(default)]
    pub physics_3d: Option<Physics3DComponent>,
    /// Parent entity in the scene tree. `None` = top-level. Local
    /// transforms compose multiplicatively from root to leaf at draw
    /// time; the engine doesn't enforce a single root.
    #[serde(default)]
    pub parent: Option<EntityId>,
    /// Optional class membership (Unreal-style). Multiple entities can
    /// share a class to reuse the same blueprint logic. `None` = an
    /// anonymous instance.
    #[serde(default)]
    pub class: Option<String>,
    /// Path (relative to the project root) of this entity's per-
    /// instance blueprint .ron file. `None` = no script attached.
    /// Editing the blueprint via the editor's "BP" button or by
    /// double-clicking the entity binds it to this slot.
    #[serde(default)]
    pub blueprint: Option<String>,
}

impl Entity {
    pub fn new(id: EntityId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            transform: Transform2D::default(),
            sprite: None,
            physics: None,
            transform_3d: None,
            mesh: None,
            physics_3d: None,
            parent: None,
            class: None,
            blueprint: None,
        }
    }

    pub fn is_3d(&self) -> bool {
        self.transform_3d.is_some()
    }

    /// World-space center after applying the transform.
    pub fn world_position(&self) -> [f32; 2] {
        self.transform.position
    }

    /// World-space size of the sprite after applying scale. Returns `None`
    /// if the entity has no sprite component.
    pub fn world_sprite_size(&self) -> Option<[f32; 2]> {
        self.sprite.as_ref().map(|s| {
            [
                s.size[0] * self.transform.scale[0],
                s.size[1] * self.transform.scale[1],
            ]
        })
    }
}

/// A 2D scene: ordered entity list and the next-id counter for fresh
/// allocations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    pub entities: Vec<Entity>,
    pub next_id: u32,
    /// Background color (linear RGB) for the renderer to clear with.
    pub clear_color: [f32; 3],
    /// Gravity used by the physics simulator. World units per second^2;
    /// y is positive downward in world space (visible as -y when drawn
    /// because the camera uses y-up).
    #[serde(default = "default_gravity")]
    pub gravity: [f32; 2],
}

fn default_gravity() -> [f32; 2] {
    [0.0, -1200.0]
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            name: "Untitled".to_owned(),
            entities: Vec::new(),
            next_id: 1,
            clear_color: [0.05, 0.10, 0.18],
            gravity: default_gravity(),
        }
    }
}

/// Errors raised by scene load / save.
#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    #[error("scene file {path} not found: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("scene file {path} parse error: {source}")]
    Parse {
        path: PathBuf,
        source: ron::error::SpannedError,
    },
    #[error("scene file {path} serialize error: {source}")]
    Serialize {
        path: PathBuf,
        source: ron::error::Error,
    },
}

impl Scene {
    /// Allocate a fresh entity id and append a new (initially blank)
    /// entity. Returns the entity index for chained edits.
    pub fn add_entity(&mut self, name: impl Into<String>) -> usize {
        let id = EntityId(self.next_id);
        self.next_id += 1;
        self.entities.push(Entity::new(id, name));
        self.entities.len() - 1
    }

    pub fn find_index(&self, id: EntityId) -> Option<usize> {
        self.entities.iter().position(|e| e.id == id)
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.find_index(id).map(|i| &self.entities[i])
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.find_index(id).map(|i| &mut self.entities[i])
    }

    pub fn remove(&mut self, id: EntityId) -> Option<Entity> {
        let i = self.find_index(id)?;
        // Reparent any children up to the removed entity's parent so
        // the tree stays connected.
        let removed_parent = self.entities[i].parent;
        for e in self.entities.iter_mut() {
            if e.parent == Some(id) {
                e.parent = removed_parent;
            }
        }
        Some(self.entities.remove(i))
    }

    /// Iterate ids of entities whose parent is `parent` (None = top-
    /// level roots).
    pub fn children_of(&self, parent: Option<EntityId>) -> Vec<EntityId> {
        self.entities
            .iter()
            .filter(|e| e.parent == parent)
            .map(|e| e.id)
            .collect()
    }

    /// Load a scene from a RON file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SceneError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| SceneError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        ron::de::from_str(&text).map_err(|source| SceneError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Save the scene to a RON file with pretty-printing.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SceneError> {
        let path = path.as_ref();
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(
            |source| SceneError::Serialize {
                path: path.to_path_buf(),
                source,
            },
        )?;
        std::fs::write(path, text).map_err(|source| SceneError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_lookup() {
        let mut s = Scene::default();
        let i = s.add_entity("player");
        s.entities[i].sprite = Some(SpriteComponent::new([64.0, 64.0], AtlasRegion::Checker));
        let id = s.entities[i].id;
        let got = s.get(id).unwrap();
        assert_eq!(got.name, "player");
        assert!(got.sprite.is_some());
    }

    #[test]
    fn round_trip_serde() {
        let mut s = Scene::default();
        let i = s.add_entity("p");
        s.entities[i].sprite = Some(SpriteComponent::new([8.0, 8.0], AtlasRegion::Dots));
        let text = ron::ser::to_string(&s).unwrap();
        let back: Scene = ron::de::from_str(&text).unwrap();
        assert_eq!(back.entities.len(), 1);
        assert_eq!(back.entities[0].name, "p");
    }

    #[test]
    fn atlas_region_uv_matches_render_layout() {
        let (mn, mx) = AtlasRegion::Cross.uv_rect();
        assert!(mn[0] >= 0.5 && mx[0] <= 1.0);
    }
}
