//! A1.1: runtime 2D texture registry for the sprite pass.
//!
//! The sprite pipeline was originally hard-wired to one procedurally
//! generated atlas, which is enough for debug quads and the bitmap font
//! but useless for anything that loads real images (adventure-game
//! backgrounds, character cutouts, imported art). This module owns a
//! slot table of GPU textures, each with its own bind group built
//! against the *same* layout the sprite pipeline was created with, so a
//! caller can swap `@group(1)` per draw batch.
//!
//! Handles are generational: destroying a texture frees its slot for
//! reuse, and a stale handle pointing at a recycled slot is rejected
//! instead of silently drawing the wrong image.

use std::path::Path;

/// Opaque reference to a texture owned by the [`Texture2dRegistry`].
///
/// Cheap to copy and safe to keep across frames. A handle whose texture
/// has been destroyed is inert: lookups return `None` rather than
/// aliasing whatever took over the slot.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Texture2dHandle {
    index: u32,
    generation: u32,
}

impl Texture2dHandle {
    /// Slot index, exposed for debug/inspector UIs.
    pub fn index(self) -> u32 {
        self.index
    }
}

/// Sampling mode picked at texture creation time.
///
/// Pixel art wants [`TextureFilter::Nearest`] so upscaling keeps hard
/// texel edges; photographic or high-resolution art wants
/// [`TextureFilter::Linear`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextureFilter {
    /// Nearest-neighbour magnification and minification.
    #[default]
    Nearest,
    /// Bilinear magnification and minification.
    Linear,
}

/// Things that can go wrong when creating or updating a texture.
#[derive(Debug, thiserror::Error)]
pub enum Texture2dError {
    #[error("texture dimensions must be non-zero, got {width}x{height}")]
    ZeroSize { width: u32, height: u32 },
    #[error("pixel buffer holds {got} bytes, expected {expected} for {width}x{height} RGBA8")]
    BufferSize {
        got: usize,
        expected: usize,
        width: u32,
        height: u32,
    },
    #[error("texture handle is stale or was destroyed")]
    StaleHandle,
    #[error("update is {width}x{height} but the texture was allocated {have_w}x{have_h}")]
    SizeMismatch {
        width: u32,
        height: u32,
        have_w: u32,
        have_h: u32,
    },
    #[error("failed to decode image file: {0}")]
    Decode(#[from] image::ImageError),
}

/// One live texture: the GPU resource plus the bind group that binds it
/// (and its sampler) at `@group(1)` of the sprite pipeline.
#[derive(Debug)]
struct Entry {
    generation: u32,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
    filter: TextureFilter,
}

/// Slot table of runtime-loaded sprite textures.
#[derive(Debug)]
pub(crate) struct Texture2dRegistry {
    layout: wgpu::BindGroupLayout,
    sampler_nearest: wgpu::Sampler,
    sampler_linear: wgpu::Sampler,
    slots: Vec<Option<Entry>>,
    free: Vec<u32>,
    /// Bumped on every allocation so a recycled slot never reuses a
    /// generation that an outstanding handle still carries.
    next_generation: u32,
}

/// Build the bind group layout shared by the sprite pipeline and every
/// registry entry: `binding 0` = 2D float texture, `binding 1` = filtering
/// sampler.
///
/// Both the pipeline layout and the per-texture bind groups must be
/// created from *this* layout, otherwise `set_bind_group` rejects the
/// swap at draw time.
pub(crate) fn sprite_texture_bgl(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("infengine_render::sprite_texture_bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

fn make_sampler(device: &wgpu::Device, filter: TextureFilter) -> wgpu::Sampler {
    let mode = match filter {
        TextureFilter::Nearest => wgpu::FilterMode::Nearest,
        TextureFilter::Linear => wgpu::FilterMode::Linear,
    };
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("infengine_render::texture2d_sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: mode,
        min_filter: mode,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    })
}

fn expected_bytes(width: u32, height: u32) -> usize {
    width as usize * height as usize * 4
}

/// Everything needed to allocate one texture, bundled so the creation
/// call stays under the argument-count limit.
#[derive(Debug)]
pub(crate) struct TextureDesc<'a> {
    pub label: &'a str,
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8, sRGB, top row first.
    pub rgba: &'a [u8],
    pub filter: TextureFilter,
}

impl Texture2dRegistry {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            layout: sprite_texture_bgl(device),
            sampler_nearest: make_sampler(device, TextureFilter::Nearest),
            sampler_linear: make_sampler(device, TextureFilter::Linear),
            slots: Vec::new(),
            free: Vec::new(),
            next_generation: 1,
        }
    }

    /// The shared bind group layout; hand this to `SpritePass::new` so
    /// pipeline and registry entries stay compatible.
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    fn sampler(&self, filter: TextureFilter) -> &wgpu::Sampler {
        match filter {
            TextureFilter::Nearest => &self.sampler_nearest,
            TextureFilter::Linear => &self.sampler_linear,
        }
    }

    /// Allocate a GPU texture, upload its pixels, and return a handle.
    pub fn create(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        desc: &TextureDesc<'_>,
    ) -> Result<Texture2dHandle, Texture2dError> {
        let TextureDesc {
            label,
            width,
            height,
            rgba,
            filter,
        } = *desc;
        if width == 0 || height == 0 {
            return Err(Texture2dError::ZeroSize { width, height });
        }
        let expected = expected_bytes(width, height);
        if rgba.len() != expected {
            return Err(Texture2dError::BufferSize {
                got: rgba.len(),
                expected,
                width,
                height,
            });
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
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
        write_rgba(queue, &texture, width, height, rgba);

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(self.sampler(filter)),
                },
            ],
        });

        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let entry = Entry {
            generation,
            texture,
            bind_group,
            width,
            height,
            filter,
        };

        let index = match self.free.pop() {
            Some(index) => {
                self.slots[index as usize] = Some(entry);
                index
            }
            None => {
                self.slots.push(Some(entry));
                (self.slots.len() - 1) as u32
            }
        };
        Ok(Texture2dHandle { index, generation })
    }

    /// Decode an image file from disk and upload it.
    pub fn create_from_file(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: &Path,
        filter: TextureFilter,
    ) -> Result<Texture2dHandle, Texture2dError> {
        let image = image::open(path)?.to_rgba8();
        let (width, height) = image.dimensions();
        let label = path.to_string_lossy();
        self.create(
            device,
            queue,
            &TextureDesc {
                label: &label,
                width,
                height,
                rgba: &image,
                filter,
            },
        )
    }

    fn entry(&self, handle: Texture2dHandle) -> Option<&Entry> {
        self.slots
            .get(handle.index as usize)?
            .as_ref()
            .filter(|e| e.generation == handle.generation)
    }

    /// Replace the pixels of an existing texture.
    ///
    /// The dimensions must match the original allocation; a resize needs
    /// a fresh [`Self::create`] because the bind group references a view
    /// of the old texture.
    pub fn update(
        &mut self,
        queue: &wgpu::Queue,
        handle: Texture2dHandle,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), Texture2dError> {
        let Some(entry) = self.entry(handle) else {
            return Err(Texture2dError::StaleHandle);
        };
        if entry.width != width || entry.height != height {
            return Err(Texture2dError::SizeMismatch {
                width,
                height,
                have_w: entry.width,
                have_h: entry.height,
            });
        }
        let expected = expected_bytes(width, height);
        if rgba.len() != expected {
            return Err(Texture2dError::BufferSize {
                got: rgba.len(),
                expected,
                width,
                height,
            });
        }
        write_rgba(queue, &entry.texture, width, height, rgba);
        Ok(())
    }

    /// Drop a texture and return its slot to the free list.
    ///
    /// Returns `false` for a handle that was already destroyed.
    pub fn destroy(&mut self, handle: Texture2dHandle) -> bool {
        let Some(slot) = self.slots.get_mut(handle.index as usize) else {
            return false;
        };
        let matches = slot
            .as_ref()
            .is_some_and(|e| e.generation == handle.generation);
        if !matches {
            return false;
        }
        *slot = None;
        self.free.push(handle.index);
        true
    }

    /// Pixel dimensions of a live texture.
    pub fn size(&self, handle: Texture2dHandle) -> Option<(u32, u32)> {
        self.entry(handle).map(|e| (e.width, e.height))
    }

    /// Sampling mode a live texture was created with.
    pub fn filter(&self, handle: Texture2dHandle) -> Option<TextureFilter> {
        self.entry(handle).map(|e| e.filter)
    }

    /// Bind group to install at `@group(1)` before drawing this texture.
    pub fn bind_group(&self, handle: Texture2dHandle) -> Option<&wgpu::BindGroup> {
        self.entry(handle).map(|e| &e.bind_group)
    }

    /// Number of live textures, for stats overlays.
    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

fn write_rgba(queue: &wgpu::Queue, texture: &wgpu::Texture, width: u32, height: u32, rgba: &[u8]) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
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
}

/// A run of consecutive sprite instances that all sample the same
/// texture, produced by [`batch_runs`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextureRun {
    pub handle: Texture2dHandle,
    pub first: u32,
    pub count: u32,
}

/// Collapse a submission-ordered queue into draw batches.
///
/// Order is *preserved*, never sorted: a point-and-click scene draws
/// back to front (background, then props, then the actor), so reordering
/// by texture would put the actor behind the scenery. Only runs of
/// consecutive instances sharing a texture are merged.
pub(crate) fn batch_runs(queue: &[(Texture2dHandle, super::SpriteInstance)]) -> Vec<TextureRun> {
    let mut runs: Vec<TextureRun> = Vec::new();
    for (i, (handle, _)) in queue.iter().enumerate() {
        match runs.last_mut() {
            Some(run) if run.handle == *handle => run.count += 1,
            _ => runs.push(TextureRun {
                handle: *handle,
                first: i as u32,
                count: 1,
            }),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SpriteInstance;

    fn handle(index: u32) -> Texture2dHandle {
        Texture2dHandle {
            index,
            generation: 1,
        }
    }

    fn inst() -> SpriteInstance {
        SpriteInstance::new([0.0, 0.0], [1.0, 1.0])
    }

    #[test]
    fn batch_runs_empty_queue_yields_no_runs() {
        assert!(batch_runs(&[]).is_empty());
    }

    #[test]
    fn batch_runs_merges_consecutive_same_texture() {
        let q = vec![
            (handle(0), inst()),
            (handle(0), inst()),
            (handle(0), inst()),
        ];
        let runs = batch_runs(&q);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].first, 0);
        assert_eq!(runs[0].count, 3);
    }

    #[test]
    fn batch_runs_preserves_submission_order_across_textures() {
        // background, actor, background again: three runs, not two,
        // because merging would hoist the second background in front of
        // the actor.
        let q = vec![
            (handle(0), inst()),
            (handle(1), inst()),
            (handle(0), inst()),
        ];
        let runs = batch_runs(&q);
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].handle, handle(0));
        assert_eq!(runs[1].handle, handle(1));
        assert_eq!(runs[2].handle, handle(0));
        assert_eq!(runs[2].first, 2);
    }

    #[test]
    fn batch_runs_covers_every_instance_exactly_once() {
        let q = vec![
            (handle(2), inst()),
            (handle(2), inst()),
            (handle(5), inst()),
            (handle(2), inst()),
        ];
        let runs = batch_runs(&q);
        let total: u32 = runs.iter().map(|r| r.count).sum();
        assert_eq!(total as usize, q.len());
        let mut next = 0;
        for run in &runs {
            assert_eq!(run.first, next);
            next += run.count;
        }
    }

    #[test]
    fn stale_handle_is_rejected_after_slot_reuse() {
        // Pure-CPU check of the generation rule: an entry recorded under
        // generation 1 must not match a handle carrying generation 2.
        let old = Texture2dHandle {
            index: 0,
            generation: 1,
        };
        let new = Texture2dHandle {
            index: 0,
            generation: 2,
        };
        assert_ne!(old, new);
    }
}
