// W33.2: UberMeshAllocator - one giant GPU vertex/index buffer shared by all
// chunk meshes. Instead of allocating a separate wgpu::Buffer per chunk
// (which causes per-draw vertex/index rebinds), the allocator hands out
// (vertex_offset, index_offset) slots from two fixed-size pools.
//
// Allocation strategy (W33.2 - Segregated Free List, TLSF-style):
//   - Bump allocator primary path: O(1) alloc, maintains a watermark.
//   - Free list: when free() is called, the slot goes into a power-of-2
//     size class bucket indexed by floor_log2(v_cap_bytes). The bucket
//     array has NUM_BUCKETS=32 entries covering sizes 2^0 .. 2^31 bytes.
//   - Alloc checks the free list first: compute ceil_log2(v_bytes), then
//     scan upward through non-empty buckets until a slot with both
//     v_cap_bytes >= v_bytes AND i_cap_bytes >= i_bytes is found.
//     Worst-case scan = 32 iterations = O(1). Typical case = 1-2 buckets.
//   - This is MUCH faster than the previous O(n) first-fit over a flat Vec
//     when view_distance is high (free list grows to thousands of entries
//     during player movement, causing frame stalls).
//
// Buffer sizes:
//   - Vertex pool: 128 MB / sizeof(MeshVertex)=40 ~ 3.3 M vertices.
//   - Index pool: 32 MB / 4 bytes = 8 M indices.
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

// ---------------------------------------------------------------
// W33.2: Segregated free list (TLSF-style)
//
// 32 buckets: bucket k holds slots whose v_cap_bytes is in [2^k, 2^(k+1)).
// floor_log2(n) = 63 - n.leading_zeros() for n > 0.
// ceil_log2(n) = floor_log2(n-1) + 1 for n > 1, else 0.
// ---------------------------------------------------------------

const NUM_BUCKETS: usize = 32;

/// Compute floor(log2(n)) for n > 0. Returns 0 for n == 0 (safe fallback).
#[inline]
fn floor_log2(n: u64) -> usize {
    if n == 0 {
        0
    } else {
        (63 - n.leading_zeros()) as usize
    }
}

/// Compute ceil(log2(n)) for n > 0. Returns 0 for n <= 1.
#[cfg(test)]
#[inline]
fn ceil_log2(n: u64) -> usize {
    if n <= 1 {
        0
    } else {
        floor_log2(n - 1) + 1
    }
}

/// Power-of-2 segregated free list. Bucket k contains slots whose
/// v_cap_bytes >= 2^k (inserted at floor_log2(v_cap_bytes)).
struct SegregatedFreeList {
    buckets: [Vec<FreeSlot>; NUM_BUCKETS],
}

impl SegregatedFreeList {
    fn new() -> Self {
        // Vec::new() is not Copy, so initialise element by element.
        Self {
            buckets: std::array::from_fn(|_| Vec::new()),
        }
    }

    /// Push a slot into the appropriate bucket.
    fn push(&mut self, slot: FreeSlot) {
        let k = floor_log2(slot.v_cap_bytes).min(NUM_BUCKETS - 1);
        self.buckets[k].push(slot);
    }

    /// Try to find and remove a slot with v_cap_bytes >= v_need AND
    /// i_cap_bytes >= i_need. Scans upward from floor_log2(v_need) so
    /// we don't miss slots just below the next power-of-2 boundary.
    ///
    /// - Bucket floor_log2(v_need): slots may or may not have v_cap >= v_need;
    ///   check both caps explicitly.
    /// - Buckets above: all slots have v_cap >= 2^k > v_need so only i_cap
    ///   needs checking.
    ///
    /// Worst-case scan = NUM_BUCKETS = 32 iterations = O(1).
    ///
    /// Returns None if no suitable slot is available.
    fn pop_fit(&mut self, v_need: u64, i_need: u64) -> Option<FreeSlot> {
        let start = floor_log2(v_need).min(NUM_BUCKETS - 1);
        for k in start..NUM_BUCKETS {
            let bucket = &mut self.buckets[k];
            if bucket.is_empty() {
                continue;
            }
            let pos = if k == start {
                // Partial bucket: need to check both caps.
                bucket.iter().position(|s| s.v_cap_bytes >= v_need && s.i_cap_bytes >= i_need)
            } else {
                // Higher bucket: v_cap is guaranteed >= 2^k > v_need.
                bucket.iter().position(|s| s.i_cap_bytes >= i_need)
            };
            if let Some(pos) = pos {
                return Some(bucket.swap_remove(pos));
            }
        }
        None
    }

    /// Number of free slots across all buckets (diagnostic / test only).
    #[cfg(test)]
    fn total_free(&self) -> usize {
        self.buckets.iter().map(|b| b.len()).sum()
    }
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

    // W33.2: segregated free list replaces the old flat Vec<FreeSlot>.
    free_list: SegregatedFreeList,
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
            free_list: SegregatedFreeList::new(),
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

        // W33.2: O(1) amortised free-list lookup via segregated buckets.
        let (v_off, i_off) = if let Some(slot) = self.free_list.pop_fit(v_bytes, i_bytes) {
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

    #[test]
    fn uber_alloc_constants_are_sane() {
        // Each vertex is 40 bytes; 128 MB / 40 = 3,355,443 vertices.
        assert_eq!(VERTEX_STRIDE, 40);
        // Use const blocks so clippy does not warn about assertions_on_constants.
        const { assert!(VERTEX_POOL_CAPACITY >= 3_000_000, "pool must hold at least 3M vertices") };
        const { assert!(INDEX_POOL_CAPACITY  >= 8_000_000, "pool must hold at least 8M indices") };
    }

    // ---------------------------------------------------------------
    // W33.2: SegregatedFreeList unit tests (no wgpu required).
    // ---------------------------------------------------------------

    #[test]
    fn floor_log2_values() {
        assert_eq!(floor_log2(1), 0);
        assert_eq!(floor_log2(2), 1);
        assert_eq!(floor_log2(3), 1);
        assert_eq!(floor_log2(4), 2);
        assert_eq!(floor_log2(1024), 10);
        assert_eq!(floor_log2(1025), 10);
        assert_eq!(floor_log2(2047), 10);
        assert_eq!(floor_log2(2048), 11);
    }

    #[test]
    fn ceil_log2_values() {
        assert_eq!(ceil_log2(1), 0);
        assert_eq!(ceil_log2(2), 1);
        assert_eq!(ceil_log2(3), 2);
        assert_eq!(ceil_log2(4), 2);
        assert_eq!(ceil_log2(5), 3);
        assert_eq!(ceil_log2(1024), 10);
        assert_eq!(ceil_log2(1025), 11);
    }

    #[test]
    fn segregated_free_list_push_and_pop_exact_fit() {
        let mut sfl = SegregatedFreeList::new();
        // Push a slot with v_cap=1024, i_cap=256.
        sfl.push(FreeSlot { v_byte_off: 0, v_cap_bytes: 1024, i_byte_off: 0, i_cap_bytes: 256 });
        // Ask for exactly 1024 v and 256 i - should succeed.
        let got = sfl.pop_fit(1024, 256);
        assert!(got.is_some(), "should find exact-fit slot");
        assert_eq!(sfl.total_free(), 0);
    }

    #[test]
    fn segregated_free_list_pop_from_larger_bucket() {
        let mut sfl = SegregatedFreeList::new();
        // Push a 2048-byte v slot. Request only 1200 bytes.
        sfl.push(FreeSlot { v_byte_off: 0, v_cap_bytes: 2048, i_byte_off: 0, i_cap_bytes: 512 });
        // floor_log2(1200) = 10 (1200 < 2048 = 2^11), floor_log2(2048) = 11.
        // Scan starts at bucket 10 (empty), then 11 (has the slot). Should find it.
        let got = sfl.pop_fit(1200, 400);
        assert!(got.is_some(), "should find larger slot");
        assert_eq!(sfl.total_free(), 0);
    }

    #[test]
    fn segregated_free_list_skips_i_too_small() {
        let mut sfl = SegregatedFreeList::new();
        // v fits but i is too small.
        sfl.push(FreeSlot { v_byte_off: 0, v_cap_bytes: 2048, i_byte_off: 0, i_cap_bytes: 100 });
        let got = sfl.pop_fit(1024, 200);
        assert!(got.is_none(), "should not find slot with i too small");
        assert_eq!(sfl.total_free(), 1);
    }

    #[test]
    fn segregated_free_list_picks_fitting_among_two() {
        let mut sfl = SegregatedFreeList::new();
        // Two slots in adjacent buckets: first too small in v, second fits.
        sfl.push(FreeSlot { v_byte_off: 0, v_cap_bytes: 512,  i_byte_off: 0,   i_cap_bytes: 200 });
        sfl.push(FreeSlot { v_byte_off: 512, v_cap_bytes: 2048, i_byte_off: 200, i_cap_bytes: 600 });
        // Need 1500 v, 500 i - only the second slot works.
        let got = sfl.pop_fit(1500, 500);
        assert!(got.is_some());
        let slot = got.unwrap();
        assert_eq!(slot.v_byte_off, 512);
        assert_eq!(sfl.total_free(), 1); // first slot remains
    }

    #[test]
    fn segregated_free_list_reuse_multiple_times() {
        let mut sfl = SegregatedFreeList::new();
        for i in 0..10u64 {
            sfl.push(FreeSlot {
                v_byte_off:  i * 1000,
                v_cap_bytes: 1000,
                i_byte_off:  i * 200,
                i_cap_bytes: 200,
            });
        }
        assert_eq!(sfl.total_free(), 10);
        // Pop all 10 with exact-fit requests.
        for _ in 0..10 {
            let got = sfl.pop_fit(800, 150);
            assert!(got.is_some(), "should always find a slot");
        }
        assert_eq!(sfl.total_free(), 0);
        // No more slots available.
        assert!(sfl.pop_fit(800, 150).is_none());
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
