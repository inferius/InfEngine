// W32: UberMeshAllocator - one giant GPU vertex/index buffer shared by all
// chunk meshes. Instead of allocating a separate wgpu::Buffer per chunk
// (which causes per-draw vertex/index rebinds), the allocator hands out
// (vertex_offset, index_offset) slots from two fixed-size pools.
//
// Allocation strategy:
//   - Bump allocator primary path: O(1) alloc, maintains a watermark.
//   - Free list: when free() is called, the slot is pushed to a Vec<FreeSlot>.
//     Next alloc() checks the free list first (first-fit search) before
//     bumping the watermark. Defragmentation / TLSF deferred to W33.
//
// Buffer sizes:
//   - Vertex pool: 128 MB / sizeof(MeshVertex)=40 ~ 3.3 M vertices.
//     At ~2000 verts per greedy-meshed chunk face group, covers ~1600 chunks.
//     Real-world peak at view_distance=8: ~512 chunks * ~3 submeshes * ~1000
//     verts ~ 1.5 M verts. Fits with 2x headroom.
//   - Index pool: 32 MB / 4 bytes = 8 M indices.
//     Typically 1.5x vertex count, so well within budget.
//
// Thread safety: single-threaded (called from the renderer's main thread only).

use std::collections::HashMap;
use crate::mesh3d::{MeshHandle, MeshVertex};

const VERTEX_POOL_BYTES: u64 = 128 * 1024 * 1024; // 128 MB
const INDEX_POOL_BYTES: u64  =  32 * 1024 * 1024; //  32 MB

const VERTEX_STRIDE: u64 = std::mem::size_of::<MeshVertex>() as u64;
const INDEX_STRIDE: u64  = std::mem::size_of::<u32>() as u64;

// Capacity in elements - only used in the compile-time assertions below.
#[allow(dead_code)]
const VERTEX_POOL_CAPACITY: u64 = VERTEX_POOL_BYTES / VERTEX_STRIDE;
#[allow(dead_code)]
const INDEX_POOL_CAPACITY: u64  = INDEX_POOL_BYTES  / INDEX_STRIDE;

/// Render layer constants for uber chunk meshes.
///
/// W33.3: chunks are split into three draw lists with different pipeline
/// states, matching MC's render layer model:
///   0 = OPAQUE      - depth-test ON, depth-write ON, no blend. Front-to-back.
///   1 = CUTOUT      - same depth state as opaque, alpha-discard (< 0.5) in
///                     shader. Leaves, glass panes, doors with windows.
///   2 = TRANSLUCENT - depth-test ON, depth-write OFF, alpha blend.
///                     Water, stained glass. Drawn back-to-front per chunk.
pub const LAYER_OPAQUE: u8 = 0;
pub const LAYER_CUTOUT: u8 = 1;
pub const LAYER_TRANSLUCENT: u8 = 2;

/// One allocation record stored per live MeshHandle.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UberAlloc {
    /// Byte offset into the uber vertex buffer.
    pub v_byte_off: u64,
    /// Number of vertices.
    pub v_count: u32,
    /// Byte offset into the uber index buffer.
    pub i_byte_off: u64,
    /// Number of indices.
    pub i_count: u32,
    /// Render layer: LAYER_OPAQUE / LAYER_CUTOUT / LAYER_TRANSLUCENT.
    pub layer: u8,
}

/// Freed slot that can be re-used by a future alloc.
#[derive(Debug, Clone, Copy)]
struct FreeSlot {
    v_byte_off: u64,
    v_cap_bytes: u64,
    i_byte_off: u64,
    i_cap_bytes: u64,
}

/// UberMeshAllocator owns the two shared GPU pools and the allocation metadata.
/// All chunk meshes share these two buffers; vertices/indices are written via
/// queue.write_buffer into the appropriate sub-range.
pub(crate) struct UberMeshAllocator {
    pub(crate) vertex_buf: wgpu::Buffer,
    pub(crate) index_buf: wgpu::Buffer,

    // Bump watermarks (in bytes).
    v_watermark: u64,
    i_watermark: u64,

    free_list: Vec<FreeSlot>,
    pub(crate) allocs: HashMap<MeshHandle, UberAlloc>,
}

impl UberMeshAllocator {
    pub fn new(device: &wgpu::Device) -> Self {
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::uber_vertex_buf"),
            size: VERTEX_POOL_BYTES,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::uber_index_buf"),
            size: INDEX_POOL_BYTES,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            vertex_buf,
            index_buf,
            v_watermark: 0,
            i_watermark: 0,
            free_list: Vec::new(),
            allocs: HashMap::new(),
        }
    }

    /// Allocate space for `vertices` + `indices`, upload via `queue`, store
    /// the allocation under `handle`. Returns `false` if the pool is full
    /// (caller should fall back to the legacy per-buffer path).
    pub fn alloc(
        &mut self,
        queue: &wgpu::Queue,
        handle: MeshHandle,
        vertices: &[MeshVertex],
        indices: &[u32],
    ) -> bool {
        if vertices.is_empty() || indices.is_empty() {
            // Empty mesh: insert a zero-size record so the handle is
            // registered (draw calls can safely skip i_count==0).
            self.allocs.insert(handle, UberAlloc {
                v_byte_off: 0,
                v_count: 0,
                i_byte_off: 0,
                i_count: 0,
                layer: LAYER_OPAQUE,
            });
            return true;
        }

        let v_bytes = vertices.len() as u64 * VERTEX_STRIDE;
        let i_bytes = indices.len() as u64 * INDEX_STRIDE;

        // Try free list first (first-fit).
        let free_idx = self.free_list.iter().position(|s| {
            s.v_cap_bytes >= v_bytes && s.i_cap_bytes >= i_bytes
        });

        let (v_off, i_off) = if let Some(fi) = free_idx {
            let slot = self.free_list.swap_remove(fi);
            (slot.v_byte_off, slot.i_byte_off)
        } else {
            // Bump allocate. Check capacity.
            if self.v_watermark + v_bytes > VERTEX_POOL_BYTES {
                tracing::warn!(
                    "uber vertex pool full ({} / {} bytes used), falling back to legacy path",
                    self.v_watermark, VERTEX_POOL_BYTES
                );
                return false;
            }
            if self.i_watermark + i_bytes > INDEX_POOL_BYTES {
                tracing::warn!(
                    "uber index pool full ({} / {} bytes used), falling back to legacy path",
                    self.i_watermark, INDEX_POOL_BYTES
                );
                return false;
            }
            let v_off = self.v_watermark;
            let i_off = self.i_watermark;
            self.v_watermark += v_bytes;
            self.i_watermark += i_bytes;
            (v_off, i_off)
        };

        // Upload data.
        queue.write_buffer(&self.vertex_buf, v_off, bytemuck::cast_slice(vertices));
        queue.write_buffer(&self.index_buf, i_off, bytemuck::cast_slice(indices));

        self.allocs.insert(handle, UberAlloc {
            v_byte_off: v_off,
            v_count: vertices.len() as u32,
            i_byte_off: i_off,
            i_count: indices.len() as u32,
            layer: LAYER_OPAQUE,
        });
        true
    }

    /// Release the slot for `handle`. The memory range is pushed to the free
    /// list and will be reused by a future alloc() call.
    pub fn free(&mut self, handle: MeshHandle) -> bool {
        if let Some(alloc) = self.allocs.remove(&handle) {
            if alloc.v_count > 0 {
                self.free_list.push(FreeSlot {
                    v_byte_off:   alloc.v_byte_off,
                    v_cap_bytes:  alloc.v_count as u64 * VERTEX_STRIDE,
                    i_byte_off:   alloc.i_byte_off,
                    i_cap_bytes:  alloc.i_count as u64 * INDEX_STRIDE,
                });
            }
            true
        } else {
            false
        }
    }

    /// Set the render layer for an existing allocation.
    /// Use the LAYER_* constants: LAYER_OPAQUE / LAYER_CUTOUT / LAYER_TRANSLUCENT.
    pub fn set_layer(&mut self, handle: MeshHandle, layer: u8) {
        if let Some(a) = self.allocs.get_mut(&handle) {
            a.layer = layer;
        }
    }

    /// Retrieve the allocation for `handle`, if any.
    pub fn get(&self, handle: MeshHandle) -> Option<&UberAlloc> {
        self.allocs.get(&handle)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    // Allocation math tests run on the CPU side without a wgpu device.
    // We test the FreeSlot accounting by hand-crafting the internal state.

    #[test]
    fn uber_alloc_constants_are_sane() {
        // Each vertex is 40 bytes; 128 MB / 40 = 3,355,443 vertices.
        assert_eq!(VERTEX_STRIDE, 40);
        // Use const blocks so clippy does not warn about assertions_on_constants.
        const { assert!(VERTEX_POOL_CAPACITY >= 3_000_000, "pool must hold at least 3M vertices") };
        const { assert!(INDEX_POOL_CAPACITY  >= 8_000_000, "pool must hold at least 8M indices") };
    }

    #[test]
    fn free_slot_first_fit_reuse() {
        // Simulate free-list reuse without wgpu.
        let mut free_list: Vec<FreeSlot> = vec![
            FreeSlot { v_byte_off: 0,   v_cap_bytes: 400,  i_byte_off: 0,   i_cap_bytes: 100 },
            FreeSlot { v_byte_off: 400, v_cap_bytes: 2000, i_byte_off: 100, i_cap_bytes: 500 },
        ];
        let v_need = 1600u64;
        let i_need = 200u64;
        let fi = free_list.iter().position(|s| s.v_cap_bytes >= v_need && s.i_cap_bytes >= i_need);
        assert_eq!(fi, Some(1), "should pick the second (larger) slot");
        let slot = free_list.swap_remove(fi.unwrap());
        assert_eq!(slot.v_byte_off, 400);
        assert_eq!(free_list.len(), 1);
    }

    #[test]
    fn uber_alloc_offsets_non_overlapping() {
        // Verify that two sequential bump allocations produce non-overlapping
        // byte ranges. We emulate the watermark logic without wgpu.
        let v_stride = VERTEX_STRIDE;
        let i_stride = INDEX_STRIDE;

        let n1_v = 100u64;
        let n1_i = 150u64;
        let n2_v = 200u64;
        let n2_i = 300u64;

        let mut v_wm = 0u64;
        let mut i_wm = 0u64;

        let v_off1 = v_wm; v_wm += n1_v * v_stride;
        let i_off1 = i_wm; i_wm += n1_i * i_stride;
        let v_off2 = v_wm; v_wm += n2_v * v_stride;
        let i_off2 = i_wm; i_wm += n2_i * i_stride;

        // Ranges must not overlap.
        assert!(v_off1 + n1_v * v_stride <= v_off2, "vertex ranges overlap");
        assert!(i_off1 + n1_i * i_stride <= i_off2, "index ranges overlap");
        let _ = v_wm;
        let _ = i_wm;
    }
}
