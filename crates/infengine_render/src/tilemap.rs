//! Simple 2D tilemap.
//!
//! Each tile is identified by a `TileId` whose numeric value selects an
//! atlas region in the renderer. Phase 2.4 maps tile ids `0..4` to the
//! four quadrants of the demo atlas; later phases will replace this with
//! a real tileset/atlas description loaded from disk.

use crate::SpriteInstance;
use crate::sprite::demo_atlas;

/// Index of a tile within the active atlas. Empty tiles use
/// `TileId::EMPTY` and are skipped during rendering.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct TileId(pub u8);

impl TileId {
    /// Sentinel value meaning "no tile here, do not draw".
    pub const EMPTY: TileId = TileId(255);

    pub fn is_empty(self) -> bool {
        self.0 == Self::EMPTY.0
    }
}

/// 2D grid of `TileId`s with a uniform tile size and a world-space
/// origin pointing at the center of tile (0, 0).
#[derive(Debug, Clone)]
pub struct Tilemap {
    width: u32,
    height: u32,
    tile_size: f32,
    origin: [f32; 2],
    tiles: Vec<TileId>,
}

impl Tilemap {
    /// Allocate a tilemap with every cell empty.
    pub fn new(width: u32, height: u32, tile_size: f32, origin: [f32; 2]) -> Self {
        let tiles = vec![TileId::EMPTY; (width * height) as usize];
        Self {
            width,
            height,
            tile_size,
            origin,
            tiles,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn tile_size(&self) -> f32 {
        self.tile_size
    }
    pub fn origin(&self) -> [f32; 2] {
        self.origin
    }
    pub fn set_origin(&mut self, origin: [f32; 2]) {
        self.origin = origin;
    }

    /// Fill every cell with the same tile id.
    pub fn fill(&mut self, id: TileId) {
        self.tiles.iter_mut().for_each(|t| *t = id);
    }

    /// Set a single cell. Out-of-bounds writes are silently ignored.
    pub fn set(&mut self, x: u32, y: u32, id: TileId) {
        if let Some(i) = self.index(x, y) {
            self.tiles[i] = id;
        }
    }

    /// Read a single cell. Returns `TileId::EMPTY` for out-of-bounds.
    pub fn get(&self, x: u32, y: u32) -> TileId {
        self.index(x, y).map_or(TileId::EMPTY, |i| self.tiles[i])
    }

    fn index(&self, x: u32, y: u32) -> Option<usize> {
        if x < self.width && y < self.height {
            Some((y * self.width + x) as usize)
        } else {
            None
        }
    }

    /// Iterate all non-empty cells as renderable `SpriteInstance`s.
    /// Tile id values out of the demo-atlas range (0..4) draw the
    /// checker region as a fallback.
    pub(crate) fn build_instances(&self, out: &mut Vec<SpriteInstance>) {
        let half = self.tile_size * 0.5;
        let origin_x = self.origin[0];
        let origin_y = self.origin[1];
        let size = [self.tile_size, self.tile_size];
        for y in 0..self.height {
            for x in 0..self.width {
                let id = self.tiles[(y * self.width + x) as usize];
                if id.is_empty() {
                    continue;
                }
                let region = match id.0 {
                    0 => demo_atlas::CHECKER,
                    1 => demo_atlas::DOTS,
                    2 => demo_atlas::STRIPES,
                    3 => demo_atlas::CROSS,
                    _ => demo_atlas::CHECKER,
                };
                // World-space position is the center of the tile; y goes
                // up so row 0 sits at the top of the map.
                let world_x = origin_x + x as f32 * self.tile_size + half;
                let world_y = origin_y - y as f32 * self.tile_size - half;
                out.push(SpriteInstance::from_region(
                    [world_x, world_y],
                    size,
                    region,
                ));
            }
        }
    }
}
