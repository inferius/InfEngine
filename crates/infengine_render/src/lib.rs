//! GPU renderer for InfEngine.
//!
//! Owns a wgpu surface, device, queue, a 2D orthographic camera, and a
//! sprite pipeline. Phase 2.1 introduces world-space coordinates (1 unit
//! = 1 logical pixel at zoom 1.0); higher steps will add sprite batching,
//! texture atlases, tilemaps, and text.

#![forbid(unsafe_code)]

mod camera;
mod camera_3d;
mod debug;
mod debug3d;
mod font;
mod gltf_load;
mod grid;
mod lights;
mod mesh3d;
mod particle_pass;
mod shadow;
mod sprite;
mod tile_anims;
mod tilemap;
mod uber_buffer;

use std::sync::Arc;

use infengine_window::EngineWindow;
use tracing::{debug, info, warn};
use wgpu::{
    Backends, Color, CommandEncoderDescriptor, CompositeAlphaMode, DeviceDescriptor, Instance,
    InstanceDescriptor, LoadOp, MemoryHints, Operations, PowerPreference, PresentMode, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RequestAdapterOptions, StoreOp, Surface,
    SurfaceConfiguration, SurfaceError, TextureUsages, TextureViewDescriptor, Trace,
};

pub use camera::Camera2D;
use camera::CameraResource;
pub use camera_3d::Camera3D;
use camera_3d::Camera3DResource;
use debug::{DebugPass, DebugVertex};
use debug3d::{Debug3DPass, Debug3DVertex};
pub use gltf_load::{GltfError, LoadedPrimitive, load_gltf};
use grid::GridPass;
use lights::{LightsResource, PointLight};
use mesh3d::{Mesh3DPass, Mesh3DRegistry};
pub use mesh3d::{MeshHandle, MeshInstance, MeshVertex, cube_geometry_public};
use uber_buffer::UberMeshAllocator;
pub use particle_pass::{ParticleInstance as TexturedParticleInstance, ParticlePass};
use tile_anims::TileAnimsResource;
pub use tile_anims::{MAX_ANIM_SLOTS, TileAnimEntry, TileAnimsUniform};

/// Reference-counted albedo bind group built once by
/// `Renderer::make_mesh_albedo` and cheaply cloned into any number
/// of mesh slots via `Renderer::bind_mesh_albedo_arc`. Opaque to
/// downstream crates so they don't have to depend on `wgpu`.
pub type AlbedoHandle = std::sync::Arc<wgpu::BindGroup>;
use shadow::ShadowPass;
use sprite::SpritePass;
pub use sprite::{SpriteInstance, demo_atlas};
pub use tilemap::{TileId, Tilemap};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Errors that can occur while creating or running the renderer.
#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("failed to create wgpu surface: {0}")]
    CreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error("no compatible GPU adapter was found")]
    NoAdapter,
    #[error("failed to request wgpu device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    #[error("surface has no compatible configuration on this adapter")]
    NoSurfaceConfig,
}

/// Minimal wgpu-backed renderer.
///
/// Owns the GPU resources required to present frames to a single window.
/// Constructed via [`Renderer::new`] after the engine window has been
/// created (typically inside `AppHandler::on_start`).
pub struct Renderer {
    surface: Surface<'static>,
    device: wgpu::Device,
    queue: Queue,
    config: SurfaceConfiguration,
    clear_color: Color,
    camera: Camera2D,
    camera_resource: CameraResource,
    camera_3d: Camera3D,
    camera_3d_resource: Camera3DResource,
    sprite: SpritePass,
    sprite_queue: Vec<SpriteInstance>,
    debug: DebugPass,
    debug_vertices: Vec<DebugVertex>,
    mesh3d: Mesh3DPass,
    mesh3d_queue: Vec<(MeshHandle, MeshInstance)>,
    meshes: Mesh3DRegistry,
    cube_handle: MeshHandle,
    depth_view: wgpu::TextureView,
    shadow: ShadowPass,
    grid: GridPass,
    show_grid: bool,
    debug3d: Debug3DPass,
    debug3d_vertices: Vec<Debug3DVertex>,
    /// Per-mesh albedo bind groups. When a mesh handle has an entry
    /// here, the mesh pipeline binds it for that mesh's draw call;
    /// otherwise it falls back to the global default (1x1 white).
    mesh_albedos: std::collections::HashMap<MeshHandle, AlbedoHandle>,
    lights: LightsResource,
    light_queue: Vec<PointLight>,
    /// Wave 9.y: GPU-side animation table. The mesh3d pipeline
    /// reads it via @group(4) so the fragment shader can scroll a
    /// vertex's UV.v through a tile's frame strip when the
    /// vertex's `anim_id != 0`. Drivers refresh the uniform via
    /// [`Renderer::upload_tile_anims`].
    tile_anims: TileAnimsResource,
    /// Directional sun direction (points TOWARDS the sun). Used by
    /// the shadow pass + the mesh3d PBR shader. Defaults to a
    /// high-noon angle; drivers can swing this through the day via
    /// [`Renderer::set_sun_dir`].
    sun_dir: glam::Vec3,
    /// Sun lighting intensity (multiplier applied to the
    /// directional light's diffuse + specular contribution). Set
    /// via [`Renderer::set_sun_intensity`]; default 3.0 mirrors
    /// the previous shader-side constant.
    sun_intensity: f32,
    /// Wave 18.1: textured billboard particle pass. Renders after
    /// the opaque mesh3d pass with alpha-blend + depth-read-only
    /// so particles composite over the voxel scene correctly.
    pub particle_pass: ParticlePass,
    /// Queued textured particle instances for the current frame.
    particle_instances: Vec<TexturedParticleInstance>,

    // W32: UberGpuBuffer chunk batching.
    //
    // All chunk meshes share a single vertex + index buffer allocated by
    // `uber`. Per-frame draw commands accumulate in `chunk_draw_queue_*`
    // and are flushed to the GPU via one `multi_draw_indexed_indirect` call
    // per render layer (opaque + transparent) inside `render()`.

    /// Shared vertex/index pools for chunk meshes.
    uber: UberMeshAllocator,
    /// Monotonically-increasing handle counter for uber-path meshes.
    /// Kept separate from the legacy counter inside Mesh3DRegistry so
    /// the two handle spaces never collide.
    uber_next_id: u32,
    /// Per-frame opaque indirect draw commands queued by `queue_chunk_draw`.
    chunk_draw_queue_opaque: Vec<(wgpu::util::DrawIndexedIndirectArgs, MeshInstance)>,
    /// Per-frame transparent indirect draw commands.
    chunk_draw_queue_transparent: Vec<(wgpu::util::DrawIndexedIndirectArgs, MeshInstance)>,
    /// GPU buffer for the per-frame indirect command list (opaque + transparent,
    /// written at the start of each render call and used immediately).
    uber_indirect_buf: wgpu::Buffer,
    /// GPU buffer for the per-frame per-chunk instance data (model/color/material).
    /// Written alongside indirect commands; indexed by `first_instance` in the
    /// indirect args.  Size doubles on overflow just like the legacy instance buf.
    uber_instance_buf: wgpu::Buffer,
    uber_instance_capacity: u64,
    /// Whether `INDIRECT_FIRST_INSTANCE` was successfully enabled on this
    /// device. When false the indirect path uses a tight loop of
    /// `draw_indexed_indirect` calls instead (still better than the
    /// legacy path because the vertex/index buffers never rebind).
    has_indirect_first_instance: bool,
}

impl Renderer {
    /// Create a renderer bound to the given engine window.
    ///
    /// Performs blocking GPU initialization via `pollster`. Should be
    /// called once at startup; subsequent resizes happen through
    /// [`Renderer::resize`].
    pub fn new(window: &EngineWindow) -> Result<Self, RendererError> {
        let (width, height) = window.size();
        let raw: Arc<winit::window::Window> = Arc::clone(window.raw());

        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance.create_surface(raw)?;

        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|_| RendererError::NoAdapter)?;

        let adapter_info = adapter.get_info();
        info!(
            backend = ?adapter_info.backend,
            name = %adapter_info.name,
            "selected GPU adapter"
        );

        // W9.y.2 added a 5th bind group (`tile_anims` at @group(4))
        // to the mesh3d pipeline. wgpu's `downlevel_defaults` cap
        // `max_bind_groups = 4`, which trips a device-creation
        // validation error on Vulkan stacks honouring the floor.
        // Bump to 5 - the actual hardware floor across desktop GPUs
        // is 8 (Vulkan / DX12 spec), so this stays within the
        // adapter's real limits while keeping every other downlevel
        // default the same.
        let mut required_limits =
            wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
        required_limits.max_bind_groups = required_limits
            .max_bind_groups
            .max(5)
            .min(adapter.limits().max_bind_groups);

        // W32: try to enable INDIRECT_FIRST_INSTANCE so that
        // multi_draw_indexed_indirect can use non-zero first_instance to
        // index per-chunk instance data. This is required on DX12, Vulkan,
        // and Metal; gracefully fall back to a loop of draw_indexed_indirect
        // if the adapter lacks support (e.g. some OpenGL backends).
        let adapter_features = adapter.features();
        let indirect_first_instance =
            adapter_features.contains(wgpu::Features::INDIRECT_FIRST_INSTANCE);
        let required_features = if indirect_first_instance {
            wgpu::Features::INDIRECT_FIRST_INSTANCE
        } else {
            wgpu::Features::empty()
        };
        if indirect_first_instance {
            info!("W32: INDIRECT_FIRST_INSTANCE enabled - using multi_draw_indexed_indirect");
        } else {
            warn!("W32: INDIRECT_FIRST_INSTANCE unavailable - falling back to draw_indexed_indirect loop");
        }

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            label: Some("infengine_render::device"),
            required_features,
            required_limits,
            memory_hints: MemoryHints::Performance,
            trace: Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::default(),
        }))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or(RendererError::NoSurfaceConfig)?;

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
        };
        surface.configure(&device, &config);

        info!(
            width = config.width,
            height = config.height,
            format = ?config.format,
            present_mode = ?config.present_mode,
            "renderer initialized"
        );

        // Default clear color: muted teal so the demo proves rendering works.
        let clear_color = Color {
            r: 0.05,
            g: 0.10,
            b: 0.18,
            a: 1.0,
        };

        let camera = Camera2D::new(config.width, config.height);
        let camera_resource = CameraResource::new(&device, &camera);
        let camera_3d = Camera3D::new(config.width, config.height);
        let camera_3d_resource = Camera3DResource::new(&device, &camera_3d);
        let sprite = SpritePass::new(&device, &queue, config.format, &camera_resource.layout);
        let debug = DebugPass::new(&device, config.format, &camera_resource.layout);
        let shadow = ShadowPass::new(&device);
        let lights_res = LightsResource::new(&device);
        let tile_anims = TileAnimsResource::new(&device);
        let mesh3d = Mesh3DPass::new(
            &device,
            &queue,
            config.format,
            DEPTH_FORMAT,
            &camera_3d_resource.layout,
            &shadow.sample_bgl,
            &lights_res.layout,
            &tile_anims.layout,
        );
        let grid = GridPass::new(
            &device,
            config.format,
            DEPTH_FORMAT,
            &camera_3d_resource.layout,
        );
        let debug3d = Debug3DPass::new(
            &device,
            config.format,
            DEPTH_FORMAT,
            &camera_3d_resource.layout,
        );
        let particle_pass = ParticlePass::new(
            &device,
            config.format,
            &camera_3d_resource.layout,
        );
        let mut meshes = Mesh3DRegistry::default();
        let (cube_v, cube_i) = mesh3d::cube_geometry_public();
        let cube_handle = meshes.insert(&device, "infengine_render::cube", &cube_v, &cube_i);
        let depth_view = create_depth_view(&device, config.width, config.height);

        // W32: allocate uber GPU pools and per-frame staging buffers.
        // The indirect buffer is pre-allocated for 2048 commands (one per
        // visible chunk submesh); it doubles on overflow like the instance buf.
        const UBER_INITIAL_INDIRECT_CAP: u64 = 2048;
        const UBER_INITIAL_INSTANCE_CAP: u64 = 2048;
        let uber = UberMeshAllocator::new(&device);
        let uber_indirect_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::uber_indirect_buf"),
            size: UBER_INITIAL_INDIRECT_CAP
                * std::mem::size_of::<wgpu::util::DrawIndexedIndirectArgs>() as u64,
            usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uber_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::uber_instance_buf"),
            size: UBER_INITIAL_INSTANCE_CAP
                * std::mem::size_of::<MeshInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            clear_color,
            camera,
            camera_resource,
            camera_3d,
            camera_3d_resource,
            sprite,
            sprite_queue: Vec::new(),
            debug,
            debug_vertices: Vec::new(),
            mesh3d,
            mesh3d_queue: Vec::new(),
            meshes,
            cube_handle,
            depth_view,
            shadow,
            grid,
            show_grid: true,
            debug3d,
            debug3d_vertices: Vec::new(),
            mesh_albedos: std::collections::HashMap::new(),
            lights: lights_res,
            light_queue: Vec::new(),
            tile_anims,
            sun_dir: glam::Vec3::new(0.4, 0.9, 0.5),
            sun_intensity: 3.0,
            particle_pass,
            particle_instances: Vec::new(),
            uber,
            uber_next_id: 0x8000_0000, // high bit set avoids collisions with legacy handles
            chunk_draw_queue_opaque: Vec::new(),
            chunk_draw_queue_transparent: Vec::new(),
            uber_indirect_buf,
            uber_instance_buf,
            uber_instance_capacity: UBER_INITIAL_INSTANCE_CAP,
            has_indirect_first_instance: indirect_first_instance,
        })
    }

    /// Queue a debug line in world space. `color` is RGBA in linear range.
    pub fn debug_line(&mut self, a: [f32; 2], b: [f32; 2], color: [f32; 4]) {
        self.debug_vertices.push(DebugVertex { position: a, color });
        self.debug_vertices.push(DebugVertex { position: b, color });
    }

    /// Queue a debug axis-aligned rectangle outline.
    pub fn debug_rect(&mut self, min: [f32; 2], max: [f32; 2], color: [f32; 4]) {
        let tl = [min[0], max[1]];
        let tr = [max[0], max[1]];
        let br = [max[0], min[1]];
        let bl = [min[0], min[1]];
        self.debug_line(tl, tr, color);
        self.debug_line(tr, br, color);
        self.debug_line(br, bl, color);
        self.debug_line(bl, tl, color);
    }

    /// Queue a debug circle approximated by `segments` line segments.
    /// `segments` is clamped to at least 3.
    pub fn debug_circle(&mut self, center: [f32; 2], radius: f32, color: [f32; 4], segments: u32) {
        let segments = segments.max(3);
        let step = std::f32::consts::TAU / segments as f32;
        let mut prev = [center[0] + radius, center[1]];
        for i in 1..=segments {
            let a = step * i as f32;
            let cur = [center[0] + radius * a.cos(), center[1] + radius * a.sin()];
            self.debug_line(prev, cur, color);
            prev = cur;
        }
    }

    /// Queue a sprite to be drawn during the next `render()` call. The
    /// queue is cleared after each frame, so call this every frame for
    /// every sprite that should appear.
    pub fn draw_sprite(&mut self, instance: SpriteInstance) {
        self.sprite_queue.push(instance);
    }

    /// Queue a 3D cube draw for the next frame. `model` is a column-major
    /// 4x4 transform (use `glam::Mat4::to_cols_array`).
    pub fn draw_cube(&mut self, model: [f32; 16], color: [f32; 4]) {
        self.draw_mesh(self.cube_handle, model, color);
    }

    /// Queue an arbitrary registered mesh for the next frame.
    pub fn draw_mesh(&mut self, handle: MeshHandle, model: [f32; 16], color: [f32; 4]) {
        self.mesh3d_queue
            .push((handle, MeshInstance::new(model, color)));
    }

    /// Queue an arbitrary registered mesh with PBR material parameters
    /// (metallic in `[0, 1]`, roughness in `[0.04, 1]`).
    pub fn draw_mesh_pbr(
        &mut self,
        handle: MeshHandle,
        model: [f32; 16],
        color: [f32; 4],
        metallic: f32,
        roughness: f32,
    ) {
        let instance = MeshInstance::new(model, color).with_material(metallic, roughness);
        self.mesh3d_queue.push((handle, instance));
    }

    /// Same as [`Renderer::draw_mesh_pbr`] but tags the instance as a
    /// water-surface mesh - the shader switches to the animated
    /// caustic-ripple branch and reads the vertex `uv` as a per-vertex
    /// flow direction instead of sampling an albedo texture.
    pub fn draw_mesh_water(
        &mut self,
        handle: MeshHandle,
        model: [f32; 16],
        color: [f32; 4],
        metallic: f32,
        roughness: f32,
    ) {
        let instance = MeshInstance::new(model, color)
            .with_material(metallic, roughness)
            .with_water_marker();
        self.mesh3d_queue.push((handle, instance));
    }

    /// Register a new mesh and return its handle. The caller can then
    /// reference the mesh via [`Renderer::draw_mesh`].
    pub fn create_mesh(
        &mut self,
        name: &str,
        vertices: &[MeshVertex],
        indices: &[u32],
    ) -> MeshHandle {
        self.meshes.insert(&self.device, name, vertices, indices)
    }

    /// Handle of the built-in unit cube.
    pub fn cube_handle(&self) -> MeshHandle {
        self.cube_handle
    }

    /// Create a fresh cube-geometry mesh. Useful when a caller
    /// wants a unit cube with its own albedo binding without
    /// disturbing the global `cube_handle` (e.g. a textured sheep
    /// body alongside an untextured player cube). The mesh data
    /// is identical to `cube_handle()`; only the registry slot
    /// differs.
    pub fn create_cube_mesh(&mut self, name: &str) -> MeshHandle {
        let (v, i) = mesh3d::cube_geometry_public();
        self.meshes.insert(&self.device, name, &v, &i)
    }

    /// Iterate a particle system and queue one cube draw per live
    /// particle. Each cube uses the particle's `size` as a uniform
    /// scale and its `color` as the PBR albedo tint. Emissive
    /// glow + roughness are hardcoded here for the v1 look; a
    /// dedicated particle pass with point sprites lives on the
    /// roadmap once the renderer grows transparent batches.
    pub fn draw_particles(&mut self, system: &infengine_particles::ParticleSystem) {
        let cube = self.cube_handle;
        for p in system.iter_live() {
            let model = glam::Mat4::from_scale_rotation_translation(
                glam::Vec3::splat(p.size.max(0.01)),
                glam::Quat::IDENTITY,
                glam::Vec3::from_array(p.position),
            )
            .to_cols_array();
            self.draw_mesh_pbr(cube, model, p.color, 0.0, 0.8);
        }
    }

    /// Release a previously-registered mesh. Returns `true` when the
    /// handle was active. Caller must guarantee no `draw_mesh_pbr`
    /// for this handle remains in the current frame's queue (a safe
    /// bet is to remove between frames or before any draw call).
    pub fn remove_mesh(&mut self, handle: MeshHandle) -> bool {
        self.mesh_albedos.remove(&handle);
        self.meshes.remove(handle)
    }

    /// Mark a mesh as transparent so it draws via the alpha-blended
    /// pipeline after every opaque mesh. Depth-write is disabled for
    /// the transparent sub-pass so stacked water voxels don't punch
    /// each other out of the depth buffer.
    pub fn set_mesh_transparent(&mut self, handle: MeshHandle, transparent: bool) {
        self.meshes.set_transparent(handle, transparent);
    }

    // --- W32: UberGpuBuffer chunk draw API ---

    /// W32: Register a chunk sub-mesh into the shared uber GPU buffer and
    /// return a `MeshHandle` key. The vertices/indices are written into the
    /// shared pool immediately (via `queue.write_buffer`).
    ///
    /// Returns a handle on success or falls back to `create_mesh` (legacy
    /// per-buffer path) if the uber pool is full (unlikely at normal view
    /// distances but handled gracefully).
    ///
    /// Use `remove_chunk_mesh` to release the slot when the chunk is unloaded.
    pub fn create_chunk_mesh(
        &mut self,
        name: &str,
        vertices: &[MeshVertex],
        indices: &[u32],
    ) -> MeshHandle {
        let handle = MeshHandle(self.uber_next_id);
        self.uber_next_id += 1;
        if !self.uber.alloc(&self.queue, handle, vertices, indices) {
            // Pool full - fall back to legacy path. The handle id already
            // advanced past the uber range; map it into the legacy registry.
            // This is a rare edge case and the mesh will not benefit from
            // the batched draw path this frame.
            warn!(name, "W32: uber pool full, allocating legacy mesh buffer");
            return self.meshes.insert(&self.device, name, vertices, indices);
        }
        handle
    }

    /// W32: Bind an albedo texture to a chunk mesh handle (same semantics as
    /// `bind_mesh_albedo_arc` for legacy meshes - the uber pipeline uses the
    /// global atlas so this is currently a no-op, but kept for API symmetry).
    pub fn bind_chunk_mesh_albedo(&mut self, _handle: MeshHandle, _albedo: AlbedoHandle) {
        // Uber-path chunks share the global atlas set via `set_mesh_albedo`.
        // Per-chunk albedo overrides are not supported in the uber path; the
        // atlas covers all block textures. This method exists so callers can
        // use the same call site for both legacy and uber meshes.
    }

    /// W32: Release a chunk mesh slot. Returns `true` if the handle was live.
    /// The slot is pushed to the free list and will be reused by a future
    /// `create_chunk_mesh` call.
    pub fn remove_chunk_mesh(&mut self, handle: MeshHandle) -> bool {
        if self.uber.free(handle) {
            return true;
        }
        // Handle may be from the legacy fallback path.
        self.mesh_albedos.remove(&handle);
        self.meshes.remove(handle)
    }

    /// W32: Flag a chunk mesh as transparent (e.g. liquid surface).
    /// Transparent chunk meshes are drawn in a separate sub-pass with
    /// alpha blending + no depth write, same as the legacy transparent path.
    pub fn set_chunk_mesh_transparent(&mut self, handle: MeshHandle, transparent: bool) {
        if self.uber.get(handle).is_some() {
            self.uber.set_transparent(handle, transparent);
        } else {
            self.meshes.set_transparent(handle, transparent);
        }
    }

    /// W32: Queue a chunk mesh draw for the current frame.
    ///
    /// The call is O(1) - it just pushes a `DrawIndexedIndirectArgs` + a
    /// `MeshInstance` into a per-frame Vec. The actual GPU work (buffer
    /// upload + `multi_draw_indexed_indirect`) happens once at `render()` time.
    ///
    /// `model`, `color`, `metallic`, `roughness` have the same semantics as
    /// `draw_mesh_pbr`. Handles that were registered via the legacy fallback
    /// path automatically route to the non-batched `draw_mesh_pbr` call.
    pub fn queue_chunk_draw(
        &mut self,
        handle: MeshHandle,
        model: [f32; 16],
        color: [f32; 4],
        metallic: f32,
        roughness: f32,
    ) {
        if let Some(alloc) = self.uber.get(handle) {
            if alloc.i_count == 0 {
                return; // Empty mesh - no draw needed.
            }
            let instance = MeshInstance::new(model, color).with_material(metallic, roughness);
            // first_instance is filled in at flush time (we don't know the
            // packed index yet). Store 0 as placeholder.
            let cmd = wgpu::util::DrawIndexedIndirectArgs {
                index_count: alloc.i_count,
                instance_count: 1,
                first_index: (alloc.i_byte_off / std::mem::size_of::<u32>() as u64) as u32,
                base_vertex: (alloc.v_byte_off / std::mem::size_of::<MeshVertex>() as u64) as i32,
                first_instance: 0, // patched during flush
            };
            if alloc.transparent {
                self.chunk_draw_queue_transparent.push((cmd, instance));
            } else {
                self.chunk_draw_queue_opaque.push((cmd, instance));
            }
        } else {
            // Legacy fallback path (uber pool was full when this mesh was created).
            self.draw_mesh_pbr(handle, model, color, metallic, roughness);
        }
    }

    /// Load every primitive in a glTF / glb file and register each as
    /// its own `MeshHandle`. Returns the list of `(primitive name,
    /// handle)` pairs so the caller can resolve names back to handles
    /// when drawing.
    pub fn import_gltf_meshes(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<Vec<(String, MeshHandle)>, GltfError> {
        let prims = load_gltf(path)?;
        let mut out = Vec::with_capacity(prims.len());
        for prim in prims {
            let handle = self.create_mesh(&prim.name, &prim.vertices, &prim.indices);
            out.push((prim.name, handle));
        }
        Ok(out)
    }

    /// Replace the global albedo texture used by all 3D mesh draws.
    /// `rgba` must be `width * height * 4` bytes in `Rgba8UnormSrgb`
    /// order. Phase 10.2 keeps this engine-wide (one texture shared by
    /// every mesh); per-material slots come later.
    pub fn set_mesh_albedo(&mut self, width: u32, height: u32, rgba: &[u8]) {
        self.mesh3d
            .set_albedo(&self.device, &self.queue, width, height, rgba);
    }

    /// Attach a per-mesh albedo texture to one specific `MeshHandle`.
    /// The pipeline binds this texture only for draws of that handle,
    /// falling back to the global default for every other handle.
    pub fn set_mesh_albedo_for(
        &mut self,
        handle: MeshHandle,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) {
        let bg = self
            .mesh3d
            .make_albedo_bind_group(&self.device, &self.queue, width, height, rgba);
        self.mesh_albedos.insert(handle, std::sync::Arc::new(bg));
    }

    /// Pre-build an albedo texture once + return the shared
    /// `Arc<BindGroup>`. The caller stashes the handle and feeds it
    /// into `bind_mesh_albedo_arc` for as many meshes as needed -
    /// the GPU texture isn't re-uploaded per mesh. Designed for
    /// voxel atlases where every chunk submesh wants the same
    /// texture without paying upload cost on every rebuild.
    pub fn make_mesh_albedo(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> AlbedoHandle {
        let bg = self
            .mesh3d
            .make_albedo_bind_group(&self.device, &self.queue, width, height, rgba);
        std::sync::Arc::new(bg)
    }

    /// Bind a pre-built albedo to a specific mesh handle. Cheap
    /// clone of the `Arc` - no GPU work.
    pub fn bind_mesh_albedo_arc(&mut self, handle: MeshHandle, albedo: AlbedoHandle) {
        self.mesh_albedos.insert(handle, albedo);
    }

    /// Load an albedo texture from a file (PNG / JPG / WEBP / etc. -
    /// anything the `image` crate handles) and bind it to `handle`.
    pub fn load_mesh_albedo_for(
        &mut self,
        handle: MeshHandle,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), image::ImageError> {
        let img = image::open(path)?.to_rgba8();
        let (w, h) = img.dimensions();
        self.set_mesh_albedo_for(handle, w, h, img.as_raw());
        Ok(())
    }

    /// Drop the per-mesh albedo binding for `handle` (reverts to the
    /// global default).
    pub fn clear_mesh_albedo_for(&mut self, handle: MeshHandle) {
        self.mesh_albedos.remove(&handle);
    }

    /// Queue a point light for this frame's lighting pass. The
    /// renderer caps the live lights at the internal MAX_LIGHTS;
    /// extra entries are silently dropped. `color` is linear RGB,
    /// `intensity` scales the colour, `range` is the spherical cut-
    /// off (smooth attenuation falls to zero at `range`).
    pub fn queue_point_light(
        &mut self,
        position: [f32; 3],
        color: [f32; 3],
        intensity: f32,
        range: f32,
    ) {
        if self.light_queue.len() >= lights::MAX_LIGHTS {
            return;
        }
        self.light_queue.push(PointLight {
            position: [position[0], position[1], position[2], 1.0],
            color: [color[0], color[1], color[2], intensity.max(0.0)],
            params: [range.max(0.01), 0.0, 0.0, 0.0],
        });
    }

    /// Mutable access to the 3D camera. Edits take effect on the next
    /// `render()` call.
    pub fn camera_3d_mut(&mut self) -> &mut Camera3D {
        &mut self.camera_3d
    }

    pub fn camera_3d(&self) -> &Camera3D {
        &self.camera_3d
    }

    pub fn show_grid(&self) -> bool {
        self.show_grid
    }

    pub fn set_show_grid(&mut self, show: bool) {
        self.show_grid = show;
    }

    /// Queue a 3D world-space line segment for this frame. Rendered
    /// after the 3D meshes with depth-test only (no depth write).
    pub fn debug_line_3d(&mut self, a: [f32; 3], b: [f32; 3], color: [f32; 4]) {
        self.debug3d_vertices
            .push(Debug3DVertex { position: a, color });
        self.debug3d_vertices
            .push(Debug3DVertex { position: b, color });
    }

    /// Queue an axis-aligned wireframe box (12 edges) in world space.
    pub fn debug_box_3d(&mut self, min: [f32; 3], max: [f32; 3], color: [f32; 4]) {
        let c = [
            [min[0], min[1], min[2]],
            [max[0], min[1], min[2]],
            [max[0], min[1], max[2]],
            [min[0], min[1], max[2]],
            [min[0], max[1], min[2]],
            [max[0], max[1], min[2]],
            [max[0], max[1], max[2]],
            [min[0], max[1], max[2]],
        ];
        // Bottom rectangle 0-1-2-3-0.
        self.debug_line_3d(c[0], c[1], color);
        self.debug_line_3d(c[1], c[2], color);
        self.debug_line_3d(c[2], c[3], color);
        self.debug_line_3d(c[3], c[0], color);
        // Top rectangle 4-5-6-7-4.
        self.debug_line_3d(c[4], c[5], color);
        self.debug_line_3d(c[5], c[6], color);
        self.debug_line_3d(c[6], c[7], color);
        self.debug_line_3d(c[7], c[4], color);
        // Vertical edges.
        self.debug_line_3d(c[0], c[4], color);
        self.debug_line_3d(c[1], c[5], color);
        self.debug_line_3d(c[2], c[6], color);
        self.debug_line_3d(c[3], c[7], color);
    }

    /// Queue every non-empty tile of `map` as a sprite. Convenience
    /// wrapper around `Tilemap::build_instances`.
    pub fn draw_tilemap(&mut self, map: &Tilemap) {
        map.build_instances(&mut self.sprite_queue);
    }

    /// Queue ASCII text for rendering. `position` is the world-space
    /// top-left of the first glyph; `pixel_size` is the world-unit
    /// height of one glyph cell (8 pixels in the font); `color` tints
    /// the glyphs. Characters outside ASCII 32..127 render as blanks.
    pub fn draw_text(&mut self, text: &str, position: [f32; 2], pixel_size: f32, color: [f32; 4]) {
        let glyph_w = pixel_size;
        let glyph_h = pixel_size;
        // Glyphs are 5px wide inside an 8px cell. Advance by 6px so
        // letters do not crowd each other.
        let advance = pixel_size * 6.0 / 8.0;
        let mut cursor_x = position[0];
        let cursor_y_center = position[1] - glyph_h * 0.5;
        for ch in text.chars() {
            let ascii = if ch.is_ascii() { ch as u8 } else { b' ' };
            let region = sprite::glyph_uv(ascii);
            let inst = SpriteInstance {
                position: [cursor_x + glyph_w * 0.5, cursor_y_center],
                size: [glyph_w, glyph_h],
                color,
                uv_min: region.0,
                uv_max: region.1,
            };
            self.sprite_queue.push(inst);
            cursor_x += advance;
        }
    }

    /// Number of sprites currently queued for the next frame (including
    /// tilemap tiles).
    pub fn queued_sprite_count(&self) -> usize {
        self.sprite_queue.len()
    }

    /// Mutable access to the 2D camera. Changes take effect on the next
    /// `render()` call (the matrix is re-uploaded then).
    pub fn camera_mut(&mut self) -> &mut Camera2D {
        &mut self.camera
    }

    /// Access to the underlying wgpu device. Reserved for engine-internal
    /// crates (e.g. `infengine_editor`) that need to create their own
    /// GPU resources sharing the renderer's logical device.
    pub fn wgpu_device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Access to the underlying wgpu queue.
    pub fn wgpu_queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Current swapchain format. Use this when building auxiliary
    /// pipelines that render into the same surface.
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Resize the swapchain to match a new window size in physical pixels.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            debug!("ignoring resize to zero dimensions");
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.camera.set_viewport(width, height);
        self.camera_3d.set_viewport(width, height);
        self.depth_view = create_depth_view(&self.device, width, height);
        debug!(width, height, "surface reconfigured");
    }

    /// Override the clear color used by [`Renderer::render`].
    pub fn set_clear_color(&mut self, r: f64, g: f64, b: f64, a: f64) {
        self.clear_color = Color { r, g, b, a };
    }

    /// Set the directional sun direction (the vector pointing
    /// TOWARDS the sun). The vector is normalised internally;
    /// passing the zero vector falls back to a fixed high-noon
    /// angle so the scene never goes completely flat-lit.
    pub fn set_sun_dir(&mut self, dir: glam::Vec3) {
        let n = dir.normalize_or_zero();
        self.sun_dir = if n.length_squared() > 1e-6 {
            n
        } else {
            glam::Vec3::new(0.4, 0.9, 0.5).normalize()
        };
    }

    /// Read the current sun direction (normalised, points TOWARDS
    /// the sun). Useful for drivers that want to compute the sky
    /// color from the same angle they pushed.
    pub fn sun_dir(&self) -> glam::Vec3 {
        self.sun_dir
    }

    /// Override the sun lighting intensity. Multiplier applied to
    /// the directional light's diffuse + specular contribution in
    /// the PBR mesh shader. Default 3.0; drop to 0 for a moonless
    /// night.
    pub fn set_sun_intensity(&mut self, intensity: f32) {
        self.sun_intensity = intensity.max(0.0);
    }

    /// W30.5: Configure distance fog for the 3D mesh shader.
    ///
    /// `fog_start` - world-unit distance where fog begins blending in.
    ///              Typically `(view_distance - 2) * CHUNK_SIZE`.
    /// `fog_end`   - world-unit distance where fog reaches full opacity.
    ///              Typically `view_distance * CHUNK_SIZE * 0.95`.
    /// `sky_color` - linear RGB colour the geometry fades toward (match
    ///              the renderer's clear_color).
    ///
    /// Passing `fog_start >= fog_end` or both zero disables fog.
    pub fn set_fog(&mut self, fog_start: f32, fog_end: f32, sky_color: [f32; 3]) {
        self.camera_3d.fog_start = fog_start.max(0.0);
        self.camera_3d.fog_end = fog_end.max(0.0);
        self.camera_3d.sky_color = sky_color;
    }

    /// Wave 18.1: queue one textured billboard particle for the next
    /// frame.  Cleared automatically after `render()`.
    pub fn queue_textured_particle(&mut self, inst: TexturedParticleInstance) {
        self.particle_instances.push(inst);
    }

    /// Wave 18.1: replace the particle atlas texture used by the
    /// textured-billboard pass.  Delegates to `ParticlePass::set_atlas`.
    pub fn set_particle_atlas(&mut self, atlas: &wgpu::Texture) {
        self.particle_pass.set_atlas(&self.device, atlas);
    }

    /// Wave 18.1: build a wgpu `Texture` from a raw RGBA pixel buffer
    /// and bind it as the particle atlas.  Convenience wrapper used by
    /// the voxel demo resource-pack load path.
    pub fn load_particle_atlas_rgba(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) {
        assert!(
            rgba.len() as u32 >= width * height * 4,
            "particle atlas buffer too small for {width}x{height}"
        );
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("infengine_render::particle_atlas_tex"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.particle_pass.set_atlas(&self.device, &texture);
    }

    /// Wave 9.y: refresh the per-tile animation uniform that the
    /// mesh3d shader samples when a vertex carries `anim_id != 0`.
    /// Callers build a `TileAnimsUniform` snapshot from their
    /// resource-pack `AnimRegistry` then upload it every time the
    /// pack reloads (slot ids stay stable across frames otherwise).
    pub fn upload_tile_anims(&self, uniform: &TileAnimsUniform) {
        self.tile_anims.upload(&self.queue, uniform);
    }

    /// Render one frame, with an optional overlay callback.
    ///
    /// The closure runs after the engine's main pass with its own
    /// encoder; it receives the wgpu device, queue, a mutable encoder
    /// borrowed from this frame, the swapchain `TextureView`, and the
    /// physical surface size. Use this to layer UI (e.g. egui) on top
    /// of the world.
    ///
    /// Recoverable surface errors (`Lost`, `Outdated`) trigger a
    /// reconfigure and return `Ok(())`.
    pub fn render<F>(&mut self, mut overlay: F) -> Result<(), SurfaceError>
    where
        F: FnMut(
            &wgpu::Device,
            &wgpu::Queue,
            &mut wgpu::CommandEncoder,
            &wgpu::TextureView,
            [u32; 2],
        ),
    {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(SurfaceError::Lost | SurfaceError::Outdated) => {
                warn!("surface lost or outdated, reconfiguring");
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(err) => return Err(err),
        };

        // Upload camera matrices and queued instance data.
        self.camera_resource.upload(&self.queue, &self.camera);
        self.camera_3d_resource.upload(&self.queue, &self.camera_3d);
        // Anchor the sun frustum on the 3D camera's look-at target so
        // the shadow map follows what the user is looking at.
        let target = self.camera_3d.target;
        let light_dir = self.sun_dir;
        self.shadow.upload_light(
            &self.queue,
            target,
            light_dir,
            20.0,
            40.0,
            self.sun_intensity,
        );
        self.sprite
            .upload(&self.device, &self.queue, &self.sprite_queue);
        self.debug
            .upload(&self.device, &self.queue, &self.debug_vertices);
        let mesh_batches = self
            .mesh3d
            .upload(&self.device, &self.queue, &self.mesh3d_queue);
        self.debug3d
            .upload(&self.device, &self.queue, &self.debug3d_vertices);
        // Upload the per-frame point-light array.
        let light_count = self.light_queue.len();
        self.lights
            .upload(&self.queue, &self.light_queue, light_count);
        // Wave 18.1: upload textured particle instances.
        let particle_count = self.particle_instances.len() as u32;
        self.particle_pass
            .upload(&self.device, &self.queue, &self.particle_instances);

        let sprite_count = self.sprite_queue.len() as u32;
        let debug_vertex_count = self.debug_vertices.len() as u32;
        let mesh3d_count = self.mesh3d_queue.len() as u32;
        let debug3d_count = self.debug3d_vertices.len() as u32;

        // W32: pack the opaque + transparent chunk draw queues and upload to
        // GPU. Indirect args and instance data are interleaved: opaque commands
        // first, then transparent. first_instance is patched here to select
        // the right row in the per-frame uber instance buffer.
        let chunk_opaque_count = self.chunk_draw_queue_opaque.len() as u32;
        let chunk_transparent_count = self.chunk_draw_queue_transparent.len() as u32;
        let total_chunk_draws =
            (chunk_opaque_count + chunk_transparent_count) as u64;

        if total_chunk_draws > 0 {
            // Grow per-frame buffers if needed (doubling strategy).
            let indirect_elem = std::mem::size_of::<wgpu::util::DrawIndexedIndirectArgs>() as u64;
            let instance_elem = std::mem::size_of::<MeshInstance>() as u64;
            // uber_instance_capacity tracks current buf capacity in elements.
            // We reuse it as the indirect cap too (both grow together).
            if total_chunk_draws > self.uber_instance_capacity {
                let mut new_cap = self.uber_instance_capacity;
                while new_cap < total_chunk_draws {
                    new_cap *= 2;
                }
                self.uber_indirect_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("infengine_render::uber_indirect_buf"),
                    size: new_cap * indirect_elem,
                    usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.uber_instance_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("infengine_render::uber_instance_buf"),
                    size: new_cap * instance_elem,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.uber_instance_capacity = new_cap;
            }

            // Pack indirect args with patched first_instance, then pack instances.
            let mut indirect_data: Vec<wgpu::util::DrawIndexedIndirectArgs> =
                Vec::with_capacity(total_chunk_draws as usize);
            let mut instance_data: Vec<MeshInstance> =
                Vec::with_capacity(total_chunk_draws as usize);

            for (idx, (base_cmd, inst)) in self
                .chunk_draw_queue_opaque
                .iter()
                .chain(self.chunk_draw_queue_transparent.iter())
                .enumerate()
            {
                let mut cmd = *base_cmd;
                cmd.first_instance = if self.has_indirect_first_instance {
                    idx as u32
                } else {
                    // Without the feature first_instance must be 0.
                    // We fall back to a draw loop instead of multi-draw,
                    // so this value is unused in that path.
                    0
                };
                indirect_data.push(cmd);
                instance_data.push(*inst);
            }

            self.queue.write_buffer(
                &self.uber_indirect_buf,
                0,
                bytemuck::cast_slice(&indirect_data),
            );
            self.queue.write_buffer(
                &self.uber_instance_buf,
                0,
                bytemuck::cast_slice(&instance_data),
            );
        }

        let view = frame.texture.create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("infengine_render::frame_encoder"),
            });

        // Shadow depth pass first - writes the sun-perspective depth
        // map that the mesh fragment shader samples for occlusion.
        let run_shadow = mesh3d_count > 0 || chunk_opaque_count > 0;
        if run_shadow {
            let mut spass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("infengine_render::shadow_pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow.depth_view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if mesh3d_count > 0 {
                self.shadow.draw(
                    &mut spass,
                    &self.meshes,
                    &mesh_batches,
                    self.mesh3d.instance_buffer(),
                );
            }
            // W32: shadow pass for uber chunk meshes (opaque only -
            // transparent meshes like water don't cast shadows).
            if chunk_opaque_count > 0 {
                self.shadow.draw_uber(
                    &mut spass,
                    &self.uber.vertex_buf,
                    &self.uber.index_buf,
                    &self.uber_indirect_buf,
                    &self.uber_instance_buf,
                    chunk_opaque_count,
                    self.has_indirect_first_instance,
                );
            }
        }

        // 3D pass next (with depth), then 2D pass on top (no depth).
        // Always run when meshes, grid, or debug lines are present so
        // the depth/color attachments get cleared.
        let run_scene3d = mesh3d_count > 0
            || total_chunk_draws > 0
            || self.show_grid
            || debug3d_count > 0;
        if run_scene3d {
            let mut rpass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("infengine_render::mesh3d_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(self.clear_color),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_bind_group(0, &self.camera_3d_resource.bind_group, &[]);
            if mesh3d_count > 0 {
                self.mesh3d.draw(
                    &mut rpass,
                    &self.meshes,
                    &mesh_batches,
                    &self.shadow.sample_bg,
                    &self.lights.bind_group,
                    &self.tile_anims.bind_group,
                    &self.mesh_albedos,
                );
            }
            // W32: flush uber chunk draws. The uber pipeline uses the same
            // shader/layout as mesh3d but with a shared vertex+index buffer.
            // Opaque draws first (depth-write on), transparent last.
            if total_chunk_draws > 0 {
                self.mesh3d.draw_uber_chunks(
                    &mut rpass,
                    &self.uber.vertex_buf,
                    &self.uber.index_buf,
                    &self.uber_indirect_buf,
                    &self.uber_instance_buf,
                    &self.shadow.sample_bg,
                    &self.lights.bind_group,
                    &self.tile_anims.bind_group,
                    chunk_opaque_count,
                    chunk_transparent_count,
                    self.has_indirect_first_instance,
                );
            }
            if self.show_grid {
                // Grid renders after meshes so opaque geometry occludes
                // it via the depth test; the grid binds the same camera
                // group at slot 0.
                self.grid.draw(&mut rpass);
            }
            // Debug 3D lines on top of the 3D scene. Same camera bind
            // group at slot 0; pipeline owns depth-test-only state so
            // they overlay geometry naturally.
            self.debug3d.draw(&mut rpass, debug3d_count);
        }

        // Wave 18.1: textured billboard particles. Rendered after the
        // opaque 3D scene (mesh3d_pass already ran the clear) but
        // before the 2D overlay so HUD elements appear on top.
        // The particle pass uses no depth attachment: it runs in a
        // dedicated LoadOp::Load pass so we don't need to carry the
        // depth texture here.
        if particle_count > 0 {
            let mut ppass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("infengine_render::particle_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            ppass.set_bind_group(0, &self.camera_3d_resource.bind_group, &[]);
            self.particle_pass.draw(&mut ppass, particle_count);
        }

        {
            // Clear-or-load the color attachment: if the 3D pass ran it
            // already cleared and populated the surface, otherwise this
            // pass owns the clear.
            let load = if run_scene3d {
                LoadOp::Load
            } else {
                LoadOp::Clear(self.clear_color)
            };
            let mut rpass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("infengine_render::main_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            rpass.set_bind_group(0, &self.camera_resource.bind_group, &[]);
            self.sprite.draw(&mut rpass, sprite_count);
            self.debug.draw(&mut rpass, debug_vertex_count);
        }

        let physical_size = [self.config.width, self.config.height];
        overlay(
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            physical_size,
        );

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.sprite_queue.clear();
        self.debug_vertices.clear();
        self.mesh3d_queue.clear();
        self.debug3d_vertices.clear();
        self.light_queue.clear();
        self.particle_instances.clear();
        // W32: clear the per-frame chunk draw queues.
        self.chunk_draw_queue_opaque.clear();
        self.chunk_draw_queue_transparent.clear();
        Ok(())
    }

    /// Current swapchain dimensions in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("size", &(self.config.width, self.config.height))
            .field("format", &self.config.format)
            .field("present_mode", &self.config.present_mode)
            .finish()
    }
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("infengine_render::depth_texture"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
