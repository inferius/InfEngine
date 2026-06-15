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
//! The cache/interpolate/blend wrappers (`cache_2d`, `cache_once`,
//! `flat_cache`, `interpolated`, `blend_density`) are VALUE-equivalent
//! to their inner argument for a single-pass generator (they only add
//! caching / chunk-edge blending), so they parse to a pass-through.
//! `blend_alpha` -> 1, `blend_offset` -> 0 (MC's no-blend defaults).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::noise::{NoiseParams, NormalNoise};

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
    Clamp { input: Box<Df>, min: f64, max: f64 },
    YGradient { from_y: f64, to_y: f64, from_v: f64, to_v: f64 },
    RangeChoice { input: Box<Df>, min_inc: f64, max_exc: f64, when_in: Box<Df>, when_out: Box<Df> },
    Noise { noise: Arc<NormalNoise>, xz: f64, y: f64 },
    ShiftedNoise { noise: Arc<NormalNoise>, sx: Box<Df>, sy: Box<Df>, sz: Box<Df>, xz: f64, y: f64 },
    /// MC `shift_a`/`shift_b`: a 2D coordinate-shift noise sampled at
    /// (x/4, 0, z/4) (a) or (z/4, x/4, 0) (b), scaled by 4.
    ShiftA(Arc<NormalNoise>),
    ShiftB(Arc<NormalNoise>),
    Spline(Spline),
    /// MC `old_blended_noise` - the pre-1.18 3D base terrain noise,
    /// still the 3D detail under the splines.
    BlendedNoise(Box<BlendedNoise>),
    Passthrough(Box<Df>),
}

impl Df {
    pub(crate) fn eval(&self, x: f64, y: f64, z: f64) -> f64 {
        match self {
            Df::Const(c) => *c,
            Df::YCoord => y,
            Df::Add(a, b) => a.eval(x, y, z) + b.eval(x, y, z),
            Df::Mul(a, b) => a.eval(x, y, z) * b.eval(x, y, z),
            Df::Min(a, b) => a.eval(x, y, z).min(b.eval(x, y, z)),
            Df::Max(a, b) => a.eval(x, y, z).max(b.eval(x, y, z)),
            Df::Abs(a) => a.eval(x, y, z).abs(),
            Df::Square(a) => {
                let v = a.eval(x, y, z);
                v * v
            }
            Df::Cube(a) => {
                let v = a.eval(x, y, z);
                v * v * v
            }
            Df::HalfNegative(a) => {
                let v = a.eval(x, y, z);
                if v > 0.0 { v } else { v * 0.5 }
            }
            Df::QuarterNegative(a) => {
                let v = a.eval(x, y, z);
                if v > 0.0 { v } else { v * 0.25 }
            }
            Df::Squeeze(a) => {
                // MC: c = clamp(v, -1, 1); c/2 - c*c*c/24.
                let c = a.eval(x, y, z).clamp(-1.0, 1.0);
                c / 2.0 - c * c * c / 24.0
            }
            Df::Clamp { input, min, max } => input.eval(x, y, z).clamp(*min, *max),
            Df::YGradient { from_y, to_y, from_v, to_v } => {
                let t = ((y - from_y) / (to_y - from_y)).clamp(0.0, 1.0);
                from_v + t * (to_v - from_v)
            }
            Df::RangeChoice { input, min_inc, max_exc, when_in, when_out } => {
                let v = input.eval(x, y, z);
                if v >= *min_inc && v < *max_exc {
                    when_in.eval(x, y, z)
                } else {
                    when_out.eval(x, y, z)
                }
            }
            Df::Noise { noise, xz, y: ys } => noise.value(x * xz, y * ys, z * xz),
            Df::ShiftedNoise { noise, sx, sy, sz, xz, y: ys } => {
                let ox = sx.eval(x, y, z);
                let oy = sy.eval(x, y, z);
                let oz = sz.eval(x, y, z);
                noise.value(x * xz + ox, y * ys + oy, z * xz + oz)
            }
            Df::ShiftA(n) => n.value(x * 0.25, 0.0, z * 0.25) * 4.0,
            Df::ShiftB(n) => n.value(z * 0.25, x * 0.25, 0.0) * 4.0,
            Df::Spline(s) => s.eval(x, y, z),
            Df::BlendedNoise(b) => b.eval(x, y, z),
            Df::Passthrough(a) => a.eval(x, y, z),
        }
    }
}

/// MC `BlendedNoise` (old_blended_noise): three Perlin stacks
/// (min-limit, max-limit, main) combined into the 3D base terrain
/// detail. Ported faithfully from `BlendedNoise.compute`.
pub(crate) struct BlendedNoise {
    min_limit: Arc<NormalNoise>,
    max_limit: Arc<NormalNoise>,
    main: Arc<NormalNoise>,
    xz_scale: f64,
    y_scale: f64,
    xz_factor: f64,
    y_factor: f64,
    smear: f64,
}

impl BlendedNoise {
    fn eval(&self, x: f64, y: f64, z: f64) -> f64 {
        // MC scales the input by a fixed 684.412 * the per-axis scale.
        let xz = 684.412 * self.xz_scale;
        let ys = 684.412 * self.y_scale;
        let mx = xz / self.xz_factor;
        let my = ys / self.y_factor;
        let main = self.main.value(x * mx, y * my, z * mx);
        // Blend factor in [0,1] from the main noise (smeared).
        let t = ((main / 10.0 + 1.0) / 2.0).clamp(0.0, 1.0);
        let lo = self.min_limit.value(x * xz, y * ys, z * xz) / 512.0;
        let hi = self.max_limit.value(x * xz, y * ys, z * xz) / 512.0;
        let _ = self.smear;
        (lo + (hi - lo) * t) / 128.0
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
/// `noises` resolves named noises.
pub(crate) fn parse(v: &Value, df: &DfResolver<'_>, noises: &NoiseResolver<'_>) -> Df {
    match v {
        Value::Number(n) => Df::Const(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => match df(s) {
            Some(inner) => parse(&inner, df, noises),
            None => Df::Const(0.0),
        },
        Value::Object(_) => parse_obj(v, df, noises),
        _ => Df::Const(0.0),
    }
}

fn boxed(v: Option<&Value>, df: &DfResolver<'_>, noises: &NoiseResolver<'_>) -> Box<Df> {
    Box::new(match v {
        Some(val) => parse(val, df, noises),
        None => Df::Const(0.0),
    })
}

fn noise_of(o: &Value, key: &str, noises: &NoiseResolver<'_>) -> Arc<NormalNoise> {
    let name = o.get(key).and_then(|v| v.as_str()).unwrap_or("");
    noises(name).unwrap_or_else(|| {
        Arc::new(NormalNoise::new(0, &NoiseParams { first_octave: -7, amplitudes: vec![1.0] }))
    })
}

fn parse_obj(o: &Value, df: &DfResolver<'_>, noises: &NoiseResolver<'_>) -> Df {
    let ty = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let ty = ty.strip_prefix("minecraft:").unwrap_or(ty);
    let a1 = || boxed(o.get("argument1"), df, noises);
    let a2 = || boxed(o.get("argument2"), df, noises);
    let arg = || boxed(o.get("argument"), df, noises);
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
        "clamp" => Df::Clamp {
            input: boxed(o.get("input"), df, noises),
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
            input: boxed(o.get("input"), df, noises),
            min_inc: field(o, "min_inclusive").map(num).unwrap_or(0.0),
            max_exc: field(o, "max_exclusive").map(num).unwrap_or(0.0),
            when_in: boxed(o.get("when_in_range"), df, noises),
            when_out: boxed(o.get("when_out_of_range"), df, noises),
        },
        "noise" => Df::Noise {
            noise: noise_of(o, "noise", noises),
            xz: field(o, "xz_scale").map(num).unwrap_or(1.0),
            y: field(o, "y_scale").map(num).unwrap_or(1.0),
        },
        "shifted_noise" => Df::ShiftedNoise {
            noise: noise_of(o, "noise", noises),
            sx: boxed(o.get("shift_x"), df, noises),
            sy: boxed(o.get("shift_y"), df, noises),
            sz: boxed(o.get("shift_z"), df, noises),
            xz: field(o, "xz_scale").map(num).unwrap_or(1.0),
            y: field(o, "y_scale").map(num).unwrap_or(1.0),
        },
        "shift_a" => Df::ShiftA(noise_of(o, "argument", noises)),
        "shift_b" => Df::ShiftB(noise_of(o, "argument", noises)),
        "spline" => parse_spline(o, df, noises),
        "old_blended_noise" => Df::BlendedNoise(Box::new(BlendedNoise {
            // The 3D base noise uses three fixed-config Perlin stacks.
            // We seed them off the registry's "minecraft:terrain" entry
            // if present, else a default broadband stack.
            min_limit: noise_or_default(noises, "minecraft:terrain", 0x10),
            max_limit: noise_or_default(noises, "minecraft:terrain", 0x20),
            main: noise_or_default(noises, "minecraft:terrain", 0x30),
            xz_scale: field(o, "xz_scale").map(num).unwrap_or(1.0),
            y_scale: field(o, "y_scale").map(num).unwrap_or(1.0),
            xz_factor: field(o, "xz_factor").map(num).unwrap_or(80.0),
            y_factor: field(o, "y_factor").map(num).unwrap_or(160.0),
            smear: field(o, "smear_scale_multiplier").map(num).unwrap_or(8.0),
        })),
        // Caching / interpolation / blending: value == inner argument
        // for a single-pass generator.
        "cache_2d" | "cache_once" | "cache_all_in_cell" | "flat_cache"
        | "interpolated" | "blend_density" => Df::Passthrough(arg()),
        "blend_alpha" => Df::Const(1.0),
        "blend_offset" => Df::Const(0.0),
        // weird_scaled_sampler approximated by its noise (the rarity
        // remap is a wave-3 refinement); end_islands / unknown -> 0.
        "weird_scaled_sampler" => Df::Noise {
            noise: noise_of(o, "noise", noises),
            xz: 1.0,
            y: 1.0,
        },
        _ => Df::Const(0.0),
    }
}

fn noise_or_default(noises: &NoiseResolver<'_>, name: &str, salt: u64) -> Arc<NormalNoise> {
    match noises(name) {
        Some(n) => n,
        None => Arc::new(NormalNoise::new(
            salt,
            &NoiseParams { first_octave: -15, amplitudes: vec![1.0; 16] },
        )),
    }
}

fn parse_spline(o: &Value, df: &DfResolver<'_>, noises: &NoiseResolver<'_>) -> Df {
    // A spline node is either {type:spline, spline:{coordinate, points}}
    // or the inner spline object directly.
    let s = o.get("spline").unwrap_or(o);
    Df::Spline(parse_spline_inner(s, df, noises))
}

fn parse_spline_inner(s: &Value, df: &DfResolver<'_>, noises: &NoiseResolver<'_>) -> Spline {
    let coordinate = boxed(s.get("coordinate"), df, noises);
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
                    SplineValue::Spline(Box::new(parse_spline_inner(inner, df, noises)))
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
        parse(&v, &no_df, &no_noise)
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
        let d = parse(&v, &resolver, &no_noise);
        assert_eq!(d.eval(0., 0., 0.), 0.5);
        // Unknown ref -> Const(0), never panics.
        let u: Value = serde_json::json!("minecraft:nope");
        assert_eq!(parse(&u, &resolver, &no_noise).eval(0., 0., 0.), 0.0);
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
