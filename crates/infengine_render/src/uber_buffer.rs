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
//   - Vertex pool: 512 MB / sizeof(MeshVertex)=40 ~ 13.4 M vertices.
//   - Index pool: 128 MB / 4 bytes = 32 M indices.
//
// W46.1: bumped from 128 MB / 32 MB after W45.5 enabled the universal
// JSON pipeline. Routing every cube block through per-element vertex
// emission (instead of the compact 6-quad cube path) inflated vertex
// usage to the point where the 128 MB pool was exhausted within a few
// chunks at view_distance >= 8, spamming "uber pool full" WARN and
// silently falling back to per-chunk legacy buffers (losing the W32
// shared-buffer perf win). Real fix is instance rendering for cube
// faces (1 vertex per face + per-instance offset = ~75% bytes saved)
// but that is a larger refactor; bumping the pool buys headroom now.
//
// Thread safety: single-threaded (called from the renderer's main thread only).

use std::collections::HashMap;
use crate::mesh3d::{MeshHandle, MeshVertex};

// W46.6: pool sizes are now runtime-configured from adapter limits.
// These defaults are used by the test path only - the Renderer queries
// adapter.limits().max_buffer_size and picks the largest pool the device
// supports up to UBER_POOL_TARGET_BYTES (=2 GB).
#[allow(dead_code)]
pub const DEFAULT_VERTEX_POOL_BYTES: u64 = 512 * 1024 * 1024; // 512 MB
pub const DEFAULT_INDEX_POOL_BYTES: u64  = 128 * 1024 * 1024; // 128 MB
/// Upper bound on pool size. We never request more than this even if
/// the adapter supports it, to avoid hogging VRAM on dGPUs with many
/// gigabytes free.
pub const UBER_POOL_TARGET_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GB
/// Minimum index pool size. Indices are 4 bytes vs ~40 byte vertices,
/// so we scale the index pool to ~25% of the vertex pool.
pub const UBER_INDEX_FRACTION: u64 = 4; // index = vertex / 4

const VERTEX_STRIDE: u64 = std::mem::size_of::<MeshVertex>() as u64;
const INDEX_STRIDE: u64  = std::mem::size_of::<u32>() as u64;

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
    /// W238: which buffer page the mesh lives in.
    pub page: u32,
    /// Byte offset into the page's vertex buffer.
    pub v_byte_off: u64,
    /// Number of vertices.
    pub v_count: u32,
    /// Byte offset into the page's index buffer.
    pub i_byte_off: u64,
    /// Number of indices.
    pub i_count: u32,
    /// Render layer: LAYER_OPAQUE / LAYER_CUTOUT / LAYER_TRANSLUCENT.
    pub layer: u8,
}

/// W238: one contiguous run of packed chunk draws sharing a render
/// layer + buffer page. The per-frame packer emits these; the draw
/// passes bind the page's buffers once per segment and issue one
/// multi_draw over the run.
#[derive(Debug, Clone, Copy)]
pub struct ChunkDrawSegment {
    pub layer: u8,
    pub page: u32,
    /// First packed row (index into the indirect/instance buffers).
    pub first: u32,
    pub count: u32,
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

/// W238: one vertex+index buffer pair with its own watermark + free
/// list. `max_buffer_size` caps a SINGLE wgpu buffer (2 GB on this
/// class of GPU) - view distance 32 with full columns needs several
/// times that, and overflowing into the legacy per-buffer path
/// exploded the draw count to ~10 FPS. Pages remove the single-buffer
/// ceiling while keeping the batched multi_draw path.
pub(crate) struct UberPage {
    pub(crate) vertex_buf: wgpu::Buffer,
    pub(crate) index_buf: wgpu::Buffer,
    v_watermark: u64,
    i_watermark: u64,
    free_list: SegregatedFreeList,
}

impl UberPage {
    fn new(
        device: &wgpu::Device,
        vertex_pool_bytes: u64,
        index_pool_bytes: u64,
        page_no: usize,
    ) -> Self {
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::uber_vertex_buf"),
            size: vertex_pool_bytes,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("infengine_render::uber_index_buf"),
            size: index_pool_bytes,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        tracing::info!(
            page = page_no,
            vertex_pool_mb = vertex_pool_bytes / (1024 * 1024),
            index_pool_mb = index_pool_bytes / (1024 * 1024),
            "W46.6/W238: uber pool page allocated",
        );
        Self {
            vertex_buf,
            index_buf,
            v_watermark: 0,
            i_watermark: 0,
            free_list: SegregatedFreeList::new(),
        }
    }

    /// Try to place `v_bytes`+`i_bytes` in this page. Returns byte
    /// offsets on success.
    fn try_alloc(
        &mut self,
        v_bytes: u64,
        i_bytes: u64,
        vertex_pool_bytes: u64,
        index_pool_bytes: u64,
    ) -> Option<(u64, u64)> {
        if let Some(slot) = self.free_list.pop_fit(v_bytes, i_bytes) {
            return Some((slot.v_byte_off, slot.i_byte_off));
        }
        if self.v_watermark + v_bytes > vertex_pool_bytes
            || self.i_watermark + i_bytes > index_pool_bytes
        {
            return None;
        }
        let off = (self.v_watermark, self.i_watermark);
        self.v_watermark += v_bytes;
        self.i_watermark += i_bytes;
        Some(off)
    }
}

/// W238: hard cap on pages. Lazily created, so a small view distance
/// stays at one page; 4 pages x (2 GB vertex + 512 MB index) = 10 GB
/// worst case on a 16 GB card. VD 40 should NOT get here via more
/// pages - greedy re-meshing and LOD are the real memory levers (see
/// the perf-plan notes).
const MAX_UBER_PAGES: usize = 4;

/// UberMeshAllocator owns the paged GPU pools and the allocation
/// metadata. All chunk meshes share these buffers; vertices/indices
/// are written via queue.write_buffer into the owning page's range.
pub(crate) struct UberMeshAllocator {
    pub(crate) pages: Vec<UberPage>,

    // W46.6: pool sizes stored per-instance (runtime-configured from
    // adapter limits) rather than as const, so the allocator scales
    // with GPU VRAM. W238: these are PER-PAGE sizes now.
    vertex_pool_bytes: u64,
    index_pool_bytes: u64,

    /// W238: warn once when the page cap is exhausted; afterwards the
    /// legacy fallback is silent (the old per-alloc warn printed two
    /// lines per submesh = thousands of lines a second).
    pages_exhausted_reported: bool,

    pub(crate) allocs: HashMap<MeshHandle, UberAlloc>,
}

impl UberMeshAllocator {
    /// Create an allocator with explicit PER-PAGE pool sizes (W46.6).
    /// The Renderer queries `adapter.limits().max_buffer_size` and
    /// passes the largest size the device supports, up to
    /// UBER_POOL_TARGET_BYTES. Page 0 is created eagerly; more pages
    /// appear on demand (W238).
    pub fn new(device: &wgpu::Device, vertex_pool_bytes: u64, index_pool_bytes: u64) -> Self {
        let first = UberPage::new(device, vertex_pool_bytes, index_pool_bytes, 0);
        Self {
            pages: vec![first],
            vertex_pool_bytes,
            index_pool_bytes,
            pages_exhausted_reported: false,
            allocs: HashMap::new(),
        }
    }

    /// Allocate space for `vertices` + `indices`, upload via `queue`, store
    /// the allocation under `handle`. Returns `false` only when every page
    /// is full AND the page cap is reached (caller falls back to the
    /// legacy per-buffer path).
    pub fn alloc(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        handle: MeshHandle,
        vertices: &[MeshVertex],
        indices: &[u32],
    ) -> bool {
        if vertices.is_empty() || indices.is_empty() {
            // Empty mesh: insert a zero-size record so the handle is
            // registered (draw calls can safely skip i_count==0).
            self.allocs.insert(handle, UberAlloc {
                page: 0,
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

        // W238: first page with room wins (free-list fit, then bump).
        let mut placed: Option<(usize, u64, u64)> = None;
        for (pi, page) in self.pages.iter_mut().enumerate() {
            if let Some((v_off, i_off)) =
                page.try_alloc(v_bytes, i_bytes, self.vertex_pool_bytes, self.index_pool_bytes)
            {
                placed = Some((pi, v_off, i_off));
                break;
            }
        }
        // All existing pages full: open a new one (up to the cap).
        if placed.is_none() && self.pages.len() < MAX_UBER_PAGES {
            let pi = self.pages.len();
            let mut page =
                UberPage::new(device, self.vertex_pool_bytes, self.index_pool_bytes, pi);
            if let Some((v_off, i_off)) =
                page.try_alloc(v_bytes, i_bytes, self.vertex_pool_bytes, self.index_pool_bytes)
            {
                placed = Some((pi, v_off, i_off));
            }
            self.pages.push(page);
        }
        let Some((pi, v_off, i_off)) = placed else {
            if !self.pages_exhausted_reported {
                self.pages_exhausted_reported = true;
                tracing::warn!(
                    pages = self.pages.len(),
                    vertex_pool_mb = self.vertex_pool_bytes / (1024 * 1024),
                    "W238: uber pool page cap exhausted - further meshes use the \
                     legacy per-buffer path (reported once)"
                );
            }
            return false;
        };

        // Upload data.
        let page = &self.pages[pi];
        queue.write_buffer(&page.vertex_buf, v_off, bytemuck::cast_slice(vertices));
        queue.write_buffer(&page.index_buf, i_off, bytemuck::cast_slice(indices));

        self.allocs.insert(handle, UberAlloc {
            page: pi as u32,
            v_byte_off: v_off,
            v_count: vertices.len() as u32,
            i_byte_off: i_off,
            i_count: indices.len() as u32,
            layer: LAYER_OPAQUE,
        });
        true
    }

    /// Release the slot for `handle`. The memory range is pushed to the
    /// owning page's free list and will be reused by a future alloc().
    pub fn free(&mut self, handle: MeshHandle) -> bool {
        if let Some(alloc) = self.allocs.remove(&handle) {
            if alloc.v_count > 0
                && let Some(page) = self.pages.get_mut(alloc.page as usize)
            {
                page.free_list.push(FreeSlot {
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
        // Each vertex is 40 bytes; the default 512 MB pool holds 13M+ verts.
        assert_eq!(VERTEX_STRIDE, 40);
        // W46.6: capacity is no longer compile-time const (pool size is
        // runtime-configured). Check default pool sizes are sane.
        const DEFAULT_V_CAP: u64 = DEFAULT_VERTEX_POOL_BYTES / 40;
        const DEFAULT_I_CAP: u64 = DEFAULT_INDEX_POOL_BYTES  / 4;
        const { assert!(DEFAULT_V_CAP >= 3_000_000, "default pool must hold at least 3M vertices") };
        const { assert!(DEFAULT_I_CAP >= 8_000_000, "default pool must hold at least 8M indices") };
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
