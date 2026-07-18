//! W182 - faithful MC worldgen port, WAVE 2: the density-function DSL.
//!
//! MC's terrain shape is a graph of "density functions" (the JSON
//! under `data/minecraft/worldgen/density_function/` + the inline tree
//! in `noise_settings/overworld.json`'s `noise_router`). This module
//! is the faithful evaluator for that DSL.
//!
//! Design: [`parse`] turns a `serde_json::Value` into a [`Df`] AST,
//! resolving string references (another density function by name) +
//! `minecraft:noise` references through caller-supplied resolvers, so
//! this wave needs NO committed MC data - the terrain stage (wave 3)
//! wires the resolvers to the real overworld JSON. [`Df::eval`] then
//! samples the tree at a world point.
//!
//! The cache/blend wrappers (`cache_2d`, `cache_once`, `flat_cache`,
//! `blend_density`) are VALUE-equivalent to their inner argument for a
//! single-pass generator (they only add caching / chunk-edge blending),
//! so they parse to a pass-through. `blend_alpha` -> 1, `blend_offset`
//! -> 0 (MC's no-blend defaults).
//!
//! W213: `interpolated` is REAL now - MC evaluates each interpolated
//! subtree only at noise-cell corners (4 wide x 8 tall for the
//! overworld) and trilinearly interpolates between them; everything
//! outside the wrapper runs per block. That cell smoothing is where
//! MC's terrain character (and the spaghetti cave shapes) comes from.
//! [`Df::eval`] stays full-resolution (biome channels, tests);
//! [`Df::eval_cells`] consumes a prebuilt [`CellLattices`] the terrain
//! stage fills per chunk column. `weird_scaled_sampler` is exact
//! (QuantizedSpaghettiRarity mappers from the decompiled source).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::noise::{ImprovedNoise, NoiseParams, NormalNoise, SimplexNoise2, SplitMix64};

/// Resolves a density-function name (e.g. `"minecraft:overworld/depth"`)
/// to its JSON definition. The terrain stage backs this with the
/// extracted overworld data; tests pass a closure over a small map.
pub(crate) type DfResolver<'a> = dyn Fn(&str) -> Option<Value> + 'a;

/// Resolves a noise name (e.g. `"minecraft:erosion"`) to its built
/// `NormalNoise`. Backed by [`NoiseRegistry`].
pub(crate) type NoiseResolver<'a> = dyn Fn(&str) -> Option<Arc<NormalNoise>> + 'a;

/// name -> built NormalNoise, seeded once. The terrain stage builds
/// this from the `worldgen/noise/*.json` params + the world seed.
#[derive(Default)]
pub(crate) struct NoiseRegistry {
    map: HashMap<String, Arc<NormalNoise>>,
}

impl NoiseRegistry {
    pub(crate) fn insert(&mut self, name: &str, seed: u64, params: &NoiseParams) {
        self.map
            .insert(name.to_string(), Arc::new(NormalNoise::new(seed, params)));
    }
    pub(crate) fn get(&self, name: &str) -> Option<Arc<NormalNoise>> {
        self.map.get(name).cloned()
    }
}

/// One control point of a MC cubic spline: at `location` along the
/// coordinate input the spline takes `value` with slope `derivative`.
struct SplinePoint {
    location: f64,
    value: SplineValue,
    derivative: f64,
}

/// A spline point's value: a constant, or a nested spline (MC composes
/// continents -> erosion -> ridges splines this way).
enum SplineValue {
    Const(f64),
    Spline(Box<Spline>),
}

/// MC `CubicSpline`: a coordinate density function + ascending points,
/// evaluated with Hermite cubic interpolation (clamped/linear outside
/// the point range, exactly like MC's `CubicSpline.Multipoint`).
pub(crate) struct Spline {
    coordinate: Box<Df>,
    points: Vec<SplinePoint>,
}

impl Spline {
    fn eval(&self, x: f64, y: f64, z: f64) -> f64 {
        let c = self.coordinate.eval(x, y, z);
        let pts = &self.points;
        if pts.is_empty() {
            return 0.0;
        }
        // Below / above the range: MC extrapolates linearly off the end
        // point using its derivative.
        if c <= pts[0].location {
            return Self::val(&pts[0], x, y, z)
                + pts[0].derivative * (c - pts[0].location);
        }
        let last = pts.len() - 1;
        if c >= pts[last].location {
            return Self::val(&pts[last], x, y, z)
                + pts[last].derivative * (c - pts[last].location);
        }
        // Find the bracketing segment.
        let mut i = 0;
        while i < last && c >= pts[i + 1].location {
            i += 1;
        }
        let p0 = &pts[i];
        let p1 = &pts[i + 1];
        let span = p1.location - p0.location;
        let t = (c - p0.location) / span;
        let v0 = Self::val(p0, x, y, z);
        let v1 = Self::val(p1, x, y, z);
        // Hermite basis with the stored derivatives (MC's formula).
        let m0 = p0.derivative * span;
        let m1 = p1.derivative * span;
        let t2 = t * t;
        let t3 = t2 * t;
        (2.0 * t3 - 3.0 * t2 + 1.0) * v0
            + (t3 - 2.0 * t2 + t) * m0
            + (-2.0 * t3 + 3.0 * t2) * v1
            + (t3 - t2) * m1
    }

    fn val(p: &SplinePoint, x: f64, y: f64, z: f64) -> f64 {
        match &p.value {
            SplineValue::Const(c) => *c,
            SplineValue::Spline(s) => s.eval(x, y, z),
        }
    }

    fn walk_mut(&mut self, f: &mut impl FnMut(&mut Df)) {
        self.coordinate.walk_mut(f);
        for p in &mut self.points {
            if let SplineValue::Spline(s) = &mut p.value {
                s.walk_mut(f);
            }
        }
    }

    fn for_children<'a>(&'a self, f: &mut impl FnMut(&'a Df)) {
        f(&self.coordinate);
        for p in &self.points {
            if let SplineValue::Spline(s) = &p.value {
                s.for_children(f);
            }
        }
    }
}

/// The density-function AST. One variant per MC op the overworld uses;
/// the rest of the game's ops (end_islands, etc.) parse to `Const(0)`
/// so an unexpected ref never panics.
pub(crate) enum Df {
    Const(f64),
    /// The world Y coordinate, as a density (MC's `y` function).
    YCoord,
    Add(Box<Df>, Box<Df>),
    Mul(Box<Df>, Box<Df>),
    Min(Box<Df>, Box<Df>),
    Max(Box<Df>, Box<Df>),
    Abs(Box<Df>),
    Square(Box<Df>),
    Cube(Box<Df>),
    HalfNegative(Box<Df>),
    QuarterNegative(Box<Df>),
    Squeeze(Box<Df>),
    /// W213: 26.x `invert` mapped op (`1.0 / input`).
    Invert(Box<Df>),
    /// W213: 26.x `find_top_surface` - scan down from `upper_bound`
    /// (cell-aligned) in `cell_height` steps for the first
    /// `density > 0`, else `lower_bound`. This IS the router's
    /// `preliminary_surface_level`.
    FindTopSurface { density: Box<Df>, upper: Box<Df>, lower: i32, cell_h: i32 },
    Clamp { input: Box<Df>, min: f64, max: f64 },
    YGradient { from_y: f64, to_y: f64, from_v: f64, to_v: f64 },
    RangeChoice { input: Box<Df>, min_inc: f64, max_exc: f64, when_in: Box<Df>, when_out: Box<Df> },
    Noise { noise: Arc<NormalNoise>, xz: f64, y: f64 },
    ShiftedNoise { noise: Arc<NormalNoise>, sx: Box<Df>, sy: Box<Df>, sz: Box<Df>, xz: f64, y: f64 },
    /// MC `shift_a`/`shift_b`: a 2D coordinate-shift noise sampled at
    /// (x/4, 0, z/4) (a) or (z/4, x/4, 0) (b), scaled by 4.
    ShiftA(Arc<NormalNoise>),
    ShiftB(Arc<NormalNoise>),
    /// W264: MC `end_islands` - `(getHeightValue(x/8, z/8) - 8) / 128`
    /// over the seed-built island simplex (the End terrain shape).
    EndIslands(Arc<SimplexNoise2>),
    Spline(Spline),
    /// MC `old_blended_noise` - the pre-1.18 3D base terrain noise,
    /// still the 3D detail under the splines.
    BlendedNoise(Box<BlendedNoise>),
    Passthrough(Box<Df>),
    /// W213: MC `interpolated` - the subtree is sampled at noise-cell
    /// corners and trilinearly interpolated. `id` indexes the node's
    /// lattice in a [`CellLattices`] (assigned by
    /// [`Df::assign_interp_ids`] in walk order after parse).
    Interpolated { inner: Box<Df>, id: usize },
    /// W213: MC `weird_scaled_sampler`, exact: rarity = mapper(input),
    /// value = rarity * |noise(pos / rarity)|. Drives the spaghetti
    /// cave tube thickness.
    WeirdScaled { input: Box<Df>, noise: Arc<NormalNoise>, type2: bool },
}

/// W213: per-chunk-column corner lattices for the `interpolated` nodes
/// of one density function. Corners every 4 blocks horizontally and 8
/// vertically (the overworld's `size_horizontal=1` / `size_vertical=2`
/// cell), exactly the resolution MC's `NoiseChunk` interpolates at.
pub(crate) struct CellLattices {
    /// Block origin of the lattice (chunk column origin).
    x0: i32,
    z0: i32,
    /// W260: corner Y base (the dimension's `min_y`; was hardwired 0
    /// before the world floor moved to -64). The lattice spans
    /// `y0..=y0+(ny-1)*ch`.
    y0: i32,
    /// Corner Y count.
    ny: usize,
    /// W299: cell dims - `4 * size_horizontal` / `4 * size_vertical`
    /// from the dim's noise settings. Overworld + nether run 4x8;
    /// the End runs 8x4 (`size_horizontal: 2, size_vertical: 1`).
    cw: i32,
    ch: i32,
    /// Horizontal corner count per axis (`16 / cw + 1`, <= 5).
    nw: usize,
    /// One `[ny][5][5]` corner grid per interpolated-node id (only
    /// the first `nw` entries of the horizontal axes are live).
    grids: Vec<Vec<[[f64; 5]; 5]>>,
}

pub(crate) const CELL_W: i32 = 4;
pub(crate) const CELL_H: i32 = 8;

impl CellLattices {
    /// Fill the lattices for the `interpolated` nodes of `roots` (in
    /// the SAME order their ids were assigned) over the chunk column at
    /// block origin `(x0, z0)`, corners `y = y0, y0+8, .., y0+(ny-1)*8`
    /// (the overworld/nether 4x8 cell).
    pub(crate) fn build(roots: &[&Df], x0: i32, z0: i32, y0: i32, ny: usize) -> CellLattices {
        Self::build_sized(roots, x0, z0, y0, ny, CELL_W, CELL_H)
    }

    /// W299: `build` with explicit cell dims for dims whose noise
    /// settings differ from the overworld's 4x8 (the End is 8x4).
    pub(crate) fn build_sized(
        roots: &[&Df],
        x0: i32,
        z0: i32,
        y0: i32,
        ny: usize,
        cw: i32,
        ch: i32,
    ) -> CellLattices {
        let nw = (16 / cw + 1) as usize;
        let mut nodes = Vec::new();
        for root in roots {
            root.collect_interp(&mut nodes);
        }
        let grids = nodes
            .iter()
            .map(|inner| {
                let mut g = vec![[[0.0f64; 5]; 5]; ny];
                for (j, layer) in g.iter_mut().enumerate() {
                    let wy = (y0 + j as i32 * ch) as f64;
                    for (i, row) in layer.iter_mut().enumerate().take(nw) {
                        let wx = (x0 + i as i32 * cw) as f64;
                        for (k, c) in row.iter_mut().enumerate().take(nw) {
                            let wz = (z0 + k as i32 * cw) as f64;
                            *c = inner.eval(wx, wy, wz);
                        }
                    }
                }
                g
            })
            .collect();
        CellLattices { x0, z0, y0, ny, cw, ch, nw, grids }
    }

    /// Trilinear sample of node `id`'s lattice at block `(x, y, z)`.
    /// Coordinates clamp to the lattice span (blocks above the top
    /// corner only occur in the always-air sky band).
    fn sample(&self, id: usize, x: f64, y: f64, z: f64) -> f64 {
        let g = &self.grids[id];
        let span = (self.nw - 1) as f64 - 1e-9;
        let fx = ((x - self.x0 as f64) / self.cw as f64).clamp(0.0, span);
        let fz = ((z - self.z0 as f64) / self.cw as f64).clamp(0.0, span);
        let fy = ((y - self.y0 as f64) / self.ch as f64).clamp(0.0, (self.ny - 1) as f64 - 1e-9);
        let (ix, iy, iz) = (fx as usize, fy as usize, fz as usize);
        let (tx, ty, tz) = (fx - ix as f64, fy - iy as f64, fz - iz as f64);
        let l = |a: f64, b: f64, t: f64| a + (b - a) * t;
        let c00 = l(g[iy][ix][iz], g[iy][ix + 1][iz], tx);
        let c01 = l(g[iy][ix][iz + 1], g[iy][ix + 1][iz + 1], tx);
        let c10 = l(g[iy + 1][ix][iz], g[iy + 1][ix + 1][iz], tx);
        let c11 = l(g[iy + 1][ix][iz + 1], g[iy + 1][ix + 1][iz + 1], tx);
        l(l(c00, c01, tz), l(c10, c11, tz), ty)
    }
}

/// `QuantizedSpaghettiRarity.getSphaghettiRarity2D` (source-exact).
fn rarity_2d(v: f64) -> f64 {
    if v < -0.75 {
        0.5
    } else if v < -0.5 {
        0.75
    } else if v < 0.5 {
        1.0
    } else if v < 0.75 {
        2.0
    } else {
        3.0
    }
}

/// `QuantizedSpaghettiRarity.getSpaghettiRarity3D` (source-exact).
fn rarity_3d(v: f64) -> f64 {
    if v < -0.5 {
        0.75
    } else if v < 0.0 {
        1.0
    } else if v < 0.5 {
        1.5
    } else {
        2.0
    }
}

impl Df {
    pub(crate) fn eval(&self, x: f64, y: f64, z: f64) -> f64 {
        self.eval_impl(x, y, z, None)
    }

    /// W213: evaluate with the `interpolated` subtrees read from a
    /// prebuilt per-chunk-column lattice (cell-corner + trilinear,
    /// MC's `NoiseChunk` semantics) instead of full resolution.
    pub(crate) fn eval_cells(&self, x: f64, y: f64, z: f64, cells: &CellLattices) -> f64 {
        self.eval_impl(x, y, z, Some(cells))
    }

    fn eval_impl(&self, x: f64, y: f64, z: f64, cells: Option<&CellLattices>) -> f64 {
        let ev = |d: &Df| d.eval_impl(x, y, z, cells);
        match self {
            Df::Const(c) => *c,
            Df::YCoord => y,
            Df::Add(a, b) => ev(a) + ev(b),
            Df::Mul(a, b) => ev(a) * ev(b),
            Df::Min(a, b) => ev(a).min(ev(b)),
            Df::Max(a, b) => ev(a).max(ev(b)),
            Df::Abs(a) => ev(a).abs(),
            Df::Square(a) => {
                let v = ev(a);
                v * v
            }
            Df::Cube(a) => {
                let v = ev(a);
                v * v * v
            }
            Df::HalfNegative(a) => {
                let v = ev(a);
                if v > 0.0 { v } else { v * 0.5 }
            }
            Df::QuarterNegative(a) => {
                let v = ev(a);
                if v > 0.0 { v } else { v * 0.25 }
            }
            Df::Squeeze(a) => {
                // MC: c = clamp(v, -1, 1); c/2 - c*c*c/24.
                let c = ev(a).clamp(-1.0, 1.0);
                c / 2.0 - c * c * c / 24.0
            }
            Df::Invert(a) => 1.0 / ev(a),
            Df::FindTopSurface { density, upper, lower, cell_h } => {
                let top = (ev(upper) / *cell_h as f64).floor() as i32 * cell_h;
                let mut result = *lower;
                let mut by = top;
                while by >= *lower {
                    if density.eval_impl(x, by as f64, z, cells) > 0.0 {
                        result = by;
                        break;
                    }
                    by -= cell_h;
                }
                result as f64
            }
            Df::Clamp { input, min, max } => ev(input).clamp(*min, *max),
            Df::YGradient { from_y, to_y, from_v, to_v } => {
                let t = ((y - from_y) / (to_y - from_y)).clamp(0.0, 1.0);
                from_v + t * (to_v - from_v)
            }
            Df::RangeChoice { input, min_inc, max_exc, when_in, when_out } => {
                let v = ev(input);
                if v >= *min_inc && v < *max_exc {
                    ev(when_in)
                } else {
                    ev(when_out)
                }
            }
            Df::Noise { noise, xz, y: ys } => noise.value(x * xz, y * ys, z * xz),
            Df::ShiftedNoise { noise, sx, sy, sz, xz, y: ys } => {
                let ox = ev(sx);
                let oy = ev(sy);
                let oz = ev(sz);
                noise.value(x * xz + ox, y * ys + oy, z * xz + oz)
            }
            Df::ShiftA(n) => n.value(x * 0.25, 0.0, z * 0.25) * 4.0,
            Df::ShiftB(n) => n.value(z * 0.25, x * 0.25, 0.0) * 4.0,
            Df::EndIslands(n) => {
                // `compute`: (getHeightValue(blockX/8, blockZ/8) - 8)
                // / 128 - Java's truncating int division, like Rust's.
                (end_island_height(n, x.floor() as i32 / 8, z.floor() as i32 / 8) as f64
                    - 8.0)
                    / 128.0
            }
            Df::Spline(s) => s.eval(x, y, z),
            Df::BlendedNoise(b) => b.eval(x, y, z),
            Df::Passthrough(a) => ev(a),
            Df::Interpolated { inner, id } => match cells {
                Some(c) => c.sample(*id, x, y, z),
                None => ev(inner),
            },
            Df::WeirdScaled { input, noise, type2 } => {
                let rarity = if *type2 { rarity_2d(ev(input)) } else { rarity_3d(ev(input)) };
                rarity * noise.value(x / rarity, y / rarity, z / rarity).abs()
            }
        }
    }

    /// Assign lattice indices to the `interpolated` nodes in a stable
    /// depth-first walk order. Call once on a freshly parsed root.
    pub(crate) fn assign_interp_ids(&mut self, next: &mut usize) {
        self.walk_mut(&mut |d| {
            if let Df::Interpolated { id, .. } = d {
                *id = *next;
                *next += 1;
            }
        });
    }

    /// The `interpolated` nodes' INNER functions, in id order (the same
    /// walk as [`Df::assign_interp_ids`]).
    pub(crate) fn collect_interp<'a>(&'a self, out: &mut Vec<&'a Df>) {
        match self {
            Df::Interpolated { inner, .. } => {
                out.push(inner);
                inner.collect_interp(out);
            }
            _ => self.for_children(&mut |c| c.collect_interp(out)),
        }
    }

    fn walk_mut(&mut self, f: &mut impl FnMut(&mut Df)) {
        f(self);
        match self {
            Df::Add(a, b) | Df::Mul(a, b) | Df::Min(a, b) | Df::Max(a, b) => {
                a.walk_mut(f);
                b.walk_mut(f);
            }
            Df::Abs(a)
            | Df::Square(a)
            | Df::Cube(a)
            | Df::HalfNegative(a)
            | Df::QuarterNegative(a)
            | Df::Squeeze(a)
            | Df::Invert(a)
            | Df::Passthrough(a) => a.walk_mut(f),
            Df::FindTopSurface { density, upper, .. } => {
                density.walk_mut(f);
                upper.walk_mut(f);
            }
            Df::Clamp { input, .. } => input.walk_mut(f),
            Df::RangeChoice { input, when_in, when_out, .. } => {
                input.walk_mut(f);
                when_in.walk_mut(f);
                when_out.walk_mut(f);
            }
            Df::ShiftedNoise { sx, sy, sz, .. } => {
                sx.walk_mut(f);
                sy.walk_mut(f);
                sz.walk_mut(f);
            }
            Df::Interpolated { inner, .. } => inner.walk_mut(f),
            Df::WeirdScaled { input, .. } => input.walk_mut(f),
            Df::Spline(s) => s.walk_mut(f),
            Df::Const(_)
            | Df::YCoord
            | Df::YGradient { .. }
            | Df::Noise { .. }
            | Df::ShiftA(_)
            | Df::ShiftB(_)
            | Df::EndIslands(_)
            | Df::BlendedNoise(_) => {}
        }
    }

    fn for_children<'a>(&'a self, f: &mut impl FnMut(&'a Df)) {
        match self {
            Df::Add(a, b) | Df::Mul(a, b) | Df::Min(a, b) | Df::Max(a, b) => {
                f(a);
                f(b);
            }
            Df::Abs(a)
            | Df::Square(a)
            | Df::Cube(a)
            | Df::HalfNegative(a)
            | Df::QuarterNegative(a)
            | Df::Squeeze(a)
            | Df::Invert(a)
            | Df::Passthrough(a) => f(a),
            Df::FindTopSurface { density, upper, .. } => {
                f(density);
                f(upper);
            }
            Df::Clamp { input, .. } => f(input),
            Df::RangeChoice { input, when_in, when_out, .. } => {
                f(input);
                f(when_in);
                f(when_out);
            }
            Df::ShiftedNoise { sx, sy, sz, .. } => {
                f(sx);
                f(sy);
                f(sz);
            }
            Df::Interpolated { inner, .. } => f(inner),
            Df::WeirdScaled { input, .. } => f(input),
            Df::Spline(s) => s.for_children(f),
            Df::Const(_)
            | Df::YCoord
            | Df::YGradient { .. }
            | Df::Noise { .. }
            | Df::ShiftA(_)
            | Df::ShiftB(_)
            | Df::EndIslands(_)
            | Df::BlendedNoise(_) => {}
        }
    }
}

/// W264: `EndIslandDensityFunction.getHeightValue`, ported verbatim -
/// the main island's 100-radius falloff plus the outer-island field
/// (chunks past 4096 sq-dist where the simplex dips under -0.9 grow
/// an island of hash-picked size 9..21).
pub(crate) fn end_island_height(noise: &SimplexNoise2, section_x: i32, section_z: i32) -> f32 {
    let chunk_x = section_x / 2;
    let chunk_z = section_z / 2;
    let sub_x = section_x % 2;
    let sub_z = section_z % 2;
    let mut h = 100.0
        - ((section_x * section_x + section_z * section_z) as f32).sqrt() * 8.0;
    h = h.clamp(-100.0, 80.0);
    for xo in -12..=12 {
        for zo in -12..=12 {
            let cx = (chunk_x + xo) as i64;
            let cz = (chunk_z + zo) as i64;
            if cx * cx + cz * cz > 4096
                && noise.get_value(cx as f64, cz as f64) < -0.9
            {
                let size = ((cx.abs() as f32) * 3439.0 + (cz.abs() as f32) * 147.0)
                    % 13.0
                    + 9.0;
                let xd = (sub_x - xo * 2) as f32;
                let zd = (sub_z - zo * 2) as f32;
                let ih = (100.0 - (xd * xd + zd * zd).sqrt() * size).clamp(-100.0, 80.0);
                h = h.max(ih);
            }
        }
    }
    h
}

/// W216: MC `BlendedNoise` (old_blended_noise), ported EXACTLY from the
/// decompiled `BlendedNoise.compute`: three legacy Perlin stacks - two
/// 16-octave limit noises and an 8-octave main noise - where octave `i`
/// samples at frequency `2^-i` (coarser octaves get LARGER weight via
/// the `/pow`), coordinates wrap at +-16.7M for double precision, every
/// octave uses the Y-SMEARED 5-arg noise (`smear_scale_multiplier`),
/// and the main noise picks the blend factor between the two limits
/// (`clampedLerp(main/10/2+0.5, min/512, max/512) / 128`). The is_max /
/// is_min shortcuts skip whole stacks exactly like MC.
pub(crate) struct BlendedNoise {
    min_limit: Vec<ImprovedNoise>,
    max_limit: Vec<ImprovedNoise>,
    main: Vec<ImprovedNoise>,
    xz_multiplier: f64,
    y_multiplier: f64,
    xz_factor: f64,
    y_factor: f64,
    smear: f64,
}

/// `PerlinNoise.wrap` - keep octave inputs within +-16.7M so the
/// double mantissa keeps sub-block precision far from spawn.
fn wrap(v: f64) -> f64 {
    v - (v / 3.3554432e7 + 0.5).floor() * 3.3554432e7
}

impl BlendedNoise {
    fn new(seed: u64, xz_scale: f64, y_scale: f64, xz_factor: f64, y_factor: f64, smear: f64) -> Self {
        let mut rng = SplitMix64::new(seed);
        let stack = |rng: &mut SplitMix64, n: usize| -> Vec<ImprovedNoise> {
            (0..n).map(|_| ImprovedNoise::new(rng)).collect()
        };
        BlendedNoise {
            min_limit: stack(&mut rng, 16),
            max_limit: stack(&mut rng, 16),
            main: stack(&mut rng, 8),
            xz_multiplier: 684.412 * xz_scale,
            y_multiplier: 684.412 * y_scale,
            xz_factor,
            y_factor,
            smear,
        }
    }

    fn eval(&self, x: f64, y: f64, z: f64) -> f64 {
        let limit_x = x * self.xz_multiplier;
        let limit_y = y * self.y_multiplier;
        let limit_z = z * self.xz_multiplier;
        let main_x = limit_x / self.xz_factor;
        let main_y = limit_y / self.y_factor;
        let main_z = limit_z / self.xz_factor;
        let limit_smear = self.y_multiplier * self.smear;
        let main_smear = limit_smear / self.y_factor;
        let mut main_v = 0.0;
        let mut pow = 1.0;
        for n in &self.main {
            main_v += n.noise_smeared(
                wrap(main_x * pow),
                wrap(main_y * pow),
                wrap(main_z * pow),
                main_smear * pow,
                main_y * pow,
            ) / pow;
            pow /= 2.0;
        }
        let factor = (main_v / 10.0 + 1.0) / 2.0;
        let is_max = factor >= 1.0;
        let is_min = factor <= 0.0;
        let mut lo = 0.0;
        let mut hi = 0.0;
        pow = 1.0;
        for i in 0..16 {
            let wx = wrap(limit_x * pow);
            let wy = wrap(limit_y * pow);
            let wz = wrap(limit_z * pow);
            let ys = limit_smear * pow;
            if !is_max {
                lo += self.min_limit[i].noise_smeared(wx, wy, wz, ys, limit_y * pow) / pow;
            }
            if !is_min {
                hi += self.max_limit[i].noise_smeared(wx, wy, wz, ys, limit_y * pow) / pow;
            }
            pow /= 2.0;
        }
        // Mth.clampedLerp(factor, min, max).
        let blended = if factor < 0.0 {
            lo / 512.0
        } else if factor > 1.0 {
            hi / 512.0
        } else {
            lo / 512.0 + factor * (hi / 512.0 - lo / 512.0)
        };
        blended / 128.0
    }
}

// ----------------------------------------------------------------
// Parser: serde_json::Value -> Df.
// ----------------------------------------------------------------

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn field<'a>(o: &'a Value, k: &str) -> Option<&'a Value> {
    o.get(k)
}

/// Parse a JSON value into a [`Df`]. A bare number is a constant; a
/// bare string is a density-function reference (resolved + parsed);
/// an object dispatches on `"type"`. `df` resolves named functions,
/// `noises` resolves named noises, `blended_seed` seeds
/// `old_blended_noise`'s legacy octave stacks (world-seed derived).
pub(crate) fn parse(v: &Value, df: &DfResolver<'_>, noises: &NoiseResolver<'_>, blended_seed: u64) -> Df {
    match v {
        Value::Number(n) => Df::Const(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => match df(s) {
            Some(inner) => parse(&inner, df, noises, blended_seed),
            None => Df::Const(0.0),
        },
        Value::Object(_) => parse_obj(v, df, noises, blended_seed),
        _ => Df::Const(0.0),
    }
}

fn boxed(v: Option<&Value>, df: &DfResolver<'_>, noises: &NoiseResolver<'_>, blended_seed: u64) -> Box<Df> {
    Box::new(match v {
        Some(val) => parse(val, df, noises, blended_seed),
        None => Df::Const(0.0),
    })
}

fn noise_of(o: &Value, key: &str, noises: &NoiseResolver<'_>) -> Arc<NormalNoise> {
    let name = o.get(key).and_then(|v| v.as_str()).unwrap_or("");
    noises(name).unwrap_or_else(|| {
        Arc::new(NormalNoise::new(0, &NoiseParams { first_octave: -7, amplitudes: vec![1.0] }))
    })
}

fn parse_obj(o: &Value, df: &DfResolver<'_>, noises: &NoiseResolver<'_>, blended_seed: u64) -> Df {
    let ty = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let ty = ty.strip_prefix("minecraft:").unwrap_or(ty);
    let a1 = || boxed(o.get("argument1"), df, noises, blended_seed);
    let a2 = || boxed(o.get("argument2"), df, noises, blended_seed);
    let arg = || boxed(o.get("argument"), df, noises, blended_seed);
    match ty {
        "add" => Df::Add(a1(), a2()),
        "mul" => Df::Mul(a1(), a2()),
        "min" => Df::Min(a1(), a2()),
        "max" => Df::Max(a1(), a2()),
        "abs" => Df::Abs(arg()),
        "square" => Df::Square(arg()),
        "cube" => Df::Cube(arg()),
        "half_negative" => Df::HalfNegative(arg()),
        "quarter_negative" => Df::QuarterNegative(arg()),
        "squeeze" => Df::Squeeze(arg()),
        "invert" => Df::Invert(arg()),
        "find_top_surface" => Df::FindTopSurface {
            density: boxed(o.get("density"), df, noises, blended_seed),
            upper: boxed(o.get("upper_bound"), df, noises, blended_seed),
            lower: field(o, "lower_bound").map(num).unwrap_or(-64.0) as i32,
            cell_h: (field(o, "cell_height").map(num).unwrap_or(8.0) as i32).max(1),
        },
        "clamp" => Df::Clamp {
            input: boxed(o.get("input"), df, noises, blended_seed),
            min: field(o, "min").map(num).unwrap_or(0.0),
            max: field(o, "max").map(num).unwrap_or(0.0),
        },
        "y_clamped_gradient" => Df::YGradient {
            from_y: field(o, "from_y").map(num).unwrap_or(0.0),
            to_y: field(o, "to_y").map(num).unwrap_or(0.0),
            from_v: field(o, "from_value").map(num).unwrap_or(0.0),
            to_v: field(o, "to_value").map(num).unwrap_or(0.0),
        },
        "range_choice" => Df::RangeChoice {
            input: boxed(o.get("input"), df, noises, blended_seed),
            min_inc: field(o, "min_inclusive").map(num).unwrap_or(0.0),
            max_exc: field(o, "max_exclusive").map(num).unwrap_or(0.0),
            when_in: boxed(o.get("when_in_range"), df, noises, blended_seed),
            when_out: boxed(o.get("when_out_of_range"), df, noises, blended_seed),
        },
        "noise" => Df::Noise {
            noise: noise_of(o, "noise", noises),
            xz: field(o, "xz_scale").map(num).unwrap_or(1.0),
            y: field(o, "y_scale").map(num).unwrap_or(1.0),
        },
        "shifted_noise" => Df::ShiftedNoise {
            noise: noise_of(o, "noise", noises),
            sx: boxed(o.get("shift_x"), df, noises, blended_seed),
            sy: boxed(o.get("shift_y"), df, noises, blended_seed),
            sz: boxed(o.get("shift_z"), df, noises, blended_seed),
            xz: field(o, "xz_scale").map(num).unwrap_or(1.0),
            y: field(o, "y_scale").map(num).unwrap_or(1.0),
        },
        "shift_a" => Df::ShiftA(noise_of(o, "argument", noises)),
        "shift_b" => Df::ShiftB(noise_of(o, "argument", noises)),
        // W264: the End island field, seeded from the same stream the
        // blended noise uses (own streams, MC's algorithm).
        "end_islands" => Df::EndIslands(Arc::new(SimplexNoise2::new(
            blended_seed ^ 0x00E4_D151_A4D5,
        ))),
        "spline" => parse_spline(o, df, noises, blended_seed),
        // W216: the exact legacy blended noise, seeded from the world
        // seed (the scales come from base_3d_noise.json).
        "old_blended_noise" => Df::BlendedNoise(Box::new(BlendedNoise::new(
            blended_seed,
            field(o, "xz_scale").map(num).unwrap_or(0.25),
            field(o, "y_scale").map(num).unwrap_or(0.125),
            field(o, "xz_factor").map(num).unwrap_or(80.0),
            field(o, "y_factor").map(num).unwrap_or(160.0),
            field(o, "smear_scale_multiplier").map(num).unwrap_or(8.0),
        ))),
        // Caching / blending: value == inner argument for a
        // single-pass generator.
        "cache_2d" | "cache_once" | "cache_all_in_cell" | "flat_cache"
        | "blend_density" => Df::Passthrough(arg()),
        // W213: real cell interpolation (id assigned post-parse by
        // `assign_interp_ids`; full-res `eval` ignores it).
        "interpolated" => Df::Interpolated { inner: arg(), id: usize::MAX },
        "blend_alpha" => Df::Const(1.0),
        "blend_offset" => Df::Const(0.0),
        // W213: exact WeirdScaledSampler (rarity-quantized |noise|).
        "weird_scaled_sampler" => Df::WeirdScaled {
            input: boxed(o.get("input"), df, noises, blended_seed),
            noise: noise_of(o, "noise", noises),
            type2: o.get("rarity_value_mapper").and_then(|v| v.as_str()) == Some("type_2"),
        },
        _ => Df::Const(0.0),
    }
}

fn parse_spline(o: &Value, df: &DfResolver<'_>, noises: &NoiseResolver<'_>, blended_seed: u64) -> Df {
    // A spline node is either {type:spline, spline:{coordinate, points}}
    // or the inner spline object directly.
    let s = o.get("spline").unwrap_or(o);
    Df::Spline(parse_spline_inner(s, df, noises, blended_seed))
}

fn parse_spline_inner(
    s: &Value,
    df: &DfResolver<'_>,
    noises: &NoiseResolver<'_>,
    blended_seed: u64,
) -> Spline {
    let coordinate = boxed(s.get("coordinate"), df, noises, blended_seed);
    let mut points = Vec::new();
    if let Some(arr) = s.get("points").and_then(|v| v.as_array()) {
        for p in arr {
            let location = p.get("location").map(num).unwrap_or(0.0);
            let derivative = p.get("derivative").map(num).unwrap_or(0.0);
            let value = match p.get("value") {
                Some(Value::Number(n)) => SplineValue::Const(n.as_f64().unwrap_or(0.0)),
                Some(v @ Value::Object(_)) => {
                    // Nested spline (its own coordinate + points).
                    let inner = v.get("spline").unwrap_or(v);
                    SplineValue::Spline(Box::new(parse_spline_inner(inner, df, noises, blended_seed)))
                }
                _ => SplineValue::Const(0.0),
            };
            points.push(SplinePoint { location, value, derivative });
        }
    }
    points.sort_by(|a, b| a.location.partial_cmp(&b.location).unwrap_or(std::cmp::Ordering::Equal));
    Spline { coordinate, points }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_df(_: &str) -> Option<Value> {
        None
    }
    fn no_noise(_: &str) -> Option<Arc<NormalNoise>> {
        None
    }

    fn parse_str(j: &str) -> Df {
        let v: Value = serde_json::from_str(j).unwrap();
        parse(&v, &no_df, &no_noise, 0)
    }

    #[test]
    fn const_and_arithmetic() {
        assert_eq!(parse_str("3.5").eval(0.0, 0.0, 0.0), 3.5);
        let d = parse_str(
            r#"{"type":"minecraft:add","argument1":2.0,"argument2":{"type":"minecraft:mul","argument1":3.0,"argument2":4.0}}"#,
        );
        assert_eq!(d.eval(0.0, 0.0, 0.0), 14.0); // 2 + 3*4
    }

    #[test]
    fn min_max_clamp() {
        assert_eq!(parse_str(r#"{"type":"minecraft:min","argument1":2.0,"argument2":5.0}"#).eval(0.,0.,0.), 2.0);
        assert_eq!(parse_str(r#"{"type":"minecraft:max","argument1":2.0,"argument2":5.0}"#).eval(0.,0.,0.), 5.0);
        let c = parse_str(r#"{"type":"minecraft:clamp","input":10.0,"min":-1.0,"max":1.0}"#);
        assert_eq!(c.eval(0.,0.,0.), 1.0);
    }

    #[test]
    fn squeeze_matches_mc_formula() {
        // squeeze(1) = 1/2 - 1/24 = 0.4583...
        let d = parse_str(r#"{"type":"minecraft:squeeze","argument":1.0}"#);
        assert!((d.eval(0.,0.,0.) - (0.5 - 1.0 / 24.0)).abs() < 1e-9);
        // clamps the input: squeeze(100) == squeeze(1).
        let big = parse_str(r#"{"type":"minecraft:squeeze","argument":100.0}"#);
        assert!((big.eval(0.,0.,0.) - (0.5 - 1.0 / 24.0)).abs() < 1e-9);
    }

    #[test]
    fn y_gradient_and_y_coord() {
        // 0 at y=64, 1 at y=128, clamped outside.
        let d = parse_str(
            r#"{"type":"minecraft:y_clamped_gradient","from_y":64,"to_y":128,"from_value":0.0,"to_value":1.0}"#,
        );
        assert_eq!(d.eval(0., 64., 0.), 0.0);
        assert_eq!(d.eval(0., 128., 0.), 1.0);
        assert!((d.eval(0., 96., 0.) - 0.5).abs() < 1e-9);
        assert_eq!(d.eval(0., 0., 0.), 0.0); // clamped below
        assert_eq!(d.eval(0., 200., 0.), 1.0); // clamped above
    }

    #[test]
    fn range_choice_picks_branch() {
        let d = parse_str(
            r#"{"type":"minecraft:range_choice","input":0.5,"min_inclusive":0.0,"max_exclusive":1.0,"when_in_range":7.0,"when_out_of_range":-7.0}"#,
        );
        assert_eq!(d.eval(0., 0., 0.), 7.0); // 0.5 in [0,1)
    }

    #[test]
    fn pass_through_and_blend_defaults() {
        // cache/interpolated/blend_density == inner; blend_alpha=1.
        let d = parse_str(r#"{"type":"minecraft:cache_2d","argument":42.0}"#);
        assert_eq!(d.eval(0., 0., 0.), 42.0);
        let i = parse_str(r#"{"type":"minecraft:interpolated","argument":{"type":"minecraft:blend_density","argument":9.0}}"#);
        assert_eq!(i.eval(0., 0., 0.), 9.0);
        assert_eq!(parse_str(r#"{"type":"minecraft:blend_alpha"}"#).eval(0.,0.,0.), 1.0);
        assert_eq!(parse_str(r#"{"type":"minecraft:blend_offset"}"#).eval(0.,0.,0.), 0.0);
    }

    #[test]
    fn reference_resolves_through_df() {
        // A string ref resolves through the df resolver + parses.
        fn resolver(name: &str) -> Option<Value> {
            if name == "minecraft:overworld/depth" {
                Some(serde_json::json!(0.5))
            } else {
                None
            }
        }
        let v: Value = serde_json::json!("minecraft:overworld/depth");
        let d = parse(&v, &resolver, &no_noise, 0);
        assert_eq!(d.eval(0., 0., 0.), 0.5);
        // Unknown ref -> Const(0), never panics.
        let u: Value = serde_json::json!("minecraft:nope");
        assert_eq!(parse(&u, &resolver, &no_noise, 0).eval(0., 0., 0.), 0.0);
    }

    #[test]
    fn spline_hermite_interpolates() {
        // Coordinate = y; points (0->0) (10->10), linear-ish in between.
        let j = r#"{"type":"minecraft:spline","spline":{"coordinate":{"type":"minecraft:y_clamped_gradient","from_y":0,"to_y":10,"from_value":0.0,"to_value":10.0},"points":[{"location":0.0,"value":0.0,"derivative":1.0},{"location":10.0,"value":10.0,"derivative":1.0}]}}"#;
        let d = parse_str(j);
        // At y=0 -> coord 0 -> value 0; at y=10 -> coord 10 -> value 10.
        assert!((d.eval(0., 0., 0.) - 0.0).abs() < 1e-6);
        assert!((d.eval(0., 10., 0.) - 10.0).abs() < 1e-6);
        // Midpoint with derivative 1 on both ends = straight line = 5.
        assert!((d.eval(0., 5., 0.) - 5.0).abs() < 1e-6);
    }
}
