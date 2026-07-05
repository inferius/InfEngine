//! W181 - faithful MC worldgen port, WAVE 1: the noise foundation.
//!
//! Everything in the modern (1.18+) MC overworld - the
//! continentalness / erosion / peaks-valleys splines, the 3D terrain
//! density, the 6-channel biome climate - is built on `NormalNoise`,
//! which is octaves of improved Perlin noise. This module ports that
//! stack faithfully.
//!
//! `ImprovedNoise` is one octave of Ken-Perlin improved 3D noise with
//! MC's 16-vector gradient set. `PerlinNoise` is a stack of octaves
//! driven by a `firstOctave` and per-octave `amplitudes` (exactly the
//! shape of MC's noise JSONs under `worldgen/noise/*.json`).
//! `NormalNoise` is MC's two-PerlinNoise wrapper that pushes the output
//! toward a normal distribution; its `value()` is what callers sample.
//!
//! Seeds: matching MC *byte-for-byte* (same seed -> same world) would
//! need MC's exact Xoroshiro128++ + positional seeding; the user's
//! ask is "look the same", so we seed our own permutation with a
//! SplitMix64 stream. The terrain LOOK comes from the algorithm + the
//! MC amplitude data, not from a specific seed, so a given seed yields
//! a different-but-equally-MC-shaped world.

/// MC `SimplexNoise.GRADIENT` - 16 gradient vectors (the last four
/// repeat the first set so a 4-bit hash indexes cleanly).
const GRADIENT: [[f64; 3]; 16] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
    [1.0, 1.0, 0.0],
    [0.0, -1.0, 1.0],
    [-1.0, 1.0, 0.0],
    [0.0, -1.0, -1.0],
];

/// SplitMix64 - a tiny, well-distributed 64-bit stream used to seed
/// the noise permutation + per-octave coordinate offsets.
pub(crate) struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform double in [0, 1). `pub(crate)` since W201 - the
    /// placed-feature streams (ocean flora) roll probabilities too.
    pub(crate) fn next_f64(&mut self) -> f64 {
        // 53-bit mantissa precision.
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }
}

#[inline]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

#[inline]
fn grad_dot(hash: usize, x: f64, y: f64, z: f64) -> f64 {
    let g = GRADIENT[hash & 15];
    g[0] * x + g[1] * y + g[2] * z
}

/// One octave of improved Perlin noise. The permutation is a shuffle
/// of 0..=255 + per-octave fractional coordinate offsets, exactly like
/// MC's `ImprovedNoise` (which draws xo/yo/zo + the shuffle from its
/// `RandomSource`).
pub(crate) struct ImprovedNoise {
    perm: [u8; 256],
    xo: f64,
    yo: f64,
    zo: f64,
}

impl ImprovedNoise {
    pub(crate) fn new(rng: &mut SplitMix64) -> Self {
        let xo = rng.next_f64() * 256.0;
        let yo = rng.next_f64() * 256.0;
        let zo = rng.next_f64() * 256.0;
        // Fisher-Yates shuffle of 0..=255 (MC seeds the same table).
        let mut perm = [0u8; 256];
        for (i, p) in perm.iter_mut().enumerate() {
            *p = i as u8;
        }
        for i in (1..256).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            perm.swap(i, j);
        }
        Self { perm, xo, yo, zo }
    }

    #[inline]
    fn p(&self, i: i32) -> usize {
        self.perm[(i & 255) as usize] as usize
    }

    /// Improved-Perlin value at `(x, y, z)`, output in ~[-1, 1].
    pub(crate) fn noise(&self, x: f64, y: f64, z: f64) -> f64 {
        self.noise_smeared(x, y, z, 0.0, 0.0)
    }

    /// W216: MC's 5-arg `ImprovedNoise.noise` - the Y coordinate fed to
    /// the GRADIENTS is quantized down by `y_scale` (capped at
    /// `y_fudge`), which vertically smears the noise; the fade weight
    /// keeps the raw Y. This is what gives `old_blended_noise` its
    /// terraced terrain body (smear_scale_multiplier).
    pub(crate) fn noise_smeared(&self, x: f64, y: f64, z: f64, y_scale: f64, y_fudge: f64) -> f64 {
        let fx = x + self.xo;
        let fy = y + self.yo;
        let fz = z + self.zo;
        let xi = fx.floor() as i32;
        let yi = fy.floor() as i32;
        let zi = fz.floor() as i32;
        let xf = fx - xi as f64;
        let y_raw = fy - yi as f64;
        let zf = fz - zi as f64;
        let yf = if y_scale != 0.0 {
            let limit = if y_fudge >= 0.0 && y_fudge < y_raw { y_fudge } else { y_raw };
            y_raw - ((limit / y_scale + 1.0e-7).floor() * y_scale)
        } else {
            y_raw
        };
        let u = fade(xf);
        let v = fade(y_raw);
        let w = fade(zf);
        // Hash the 8 cube corners through the permutation.
        let a = self.p(xi) as i32 + yi;
        let aa = self.p(a) as i32 + zi;
        let ab = self.p(a + 1) as i32 + zi;
        let b = self.p(xi + 1) as i32 + yi;
        let ba = self.p(b) as i32 + zi;
        let bb = self.p(b + 1) as i32 + zi;
        // Trilinear blend of the 8 corner gradients.
        let x1 = lerp(
            u,
            grad_dot(self.p(aa), xf, yf, zf),
            grad_dot(self.p(ba), xf - 1.0, yf, zf),
        );
        let x2 = lerp(
            u,
            grad_dot(self.p(ab), xf, yf - 1.0, zf),
            grad_dot(self.p(bb), xf - 1.0, yf - 1.0, zf),
        );
        let x3 = lerp(
            u,
            grad_dot(self.p(aa + 1), xf, yf, zf - 1.0),
            grad_dot(self.p(ba + 1), xf - 1.0, yf, zf - 1.0),
        );
        let x4 = lerp(
            u,
            grad_dot(self.p(ab + 1), xf, yf - 1.0, zf - 1.0),
            grad_dot(self.p(bb + 1), xf - 1.0, yf - 1.0, zf - 1.0),
        );
        let y1 = lerp(v, x1, x2);
        let y2 = lerp(v, x3, x4);
        lerp(w, y1, y2)
    }
}

/// MC's noise-param record: `firstOctave` + per-octave `amplitudes`.
/// Mirrors `data/minecraft/worldgen/noise/<name>.json` exactly so the
/// JSON loads straight into it (serde) with no remapping.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct NoiseParams {
    #[serde(rename = "firstOctave")]
    pub(crate) first_octave: i32,
    pub(crate) amplitudes: Vec<f64>,
}

/// A stack of [`ImprovedNoise`] octaves. `getValue` sums each octave
/// at frequency `2^(firstOctave+i)` weighted by `amplitudes[i]` and a
/// lowest-frequency value factor - the exact MC `PerlinNoise.getValue`
/// shape (so the contrast between octaves matches).
pub(crate) struct PerlinNoise {
    octaves: Vec<Option<ImprovedNoise>>,
    amplitudes: Vec<f64>,
    lowest_freq_input: f64,
    lowest_freq_value: f64,
}

/// MC wraps very large coords to keep f64 precision; `3.3554432e7`
/// is MC's exact wrap period. A no-op for our coordinate ranges, kept
/// for faithfulness.
#[inline]
fn wrap(v: f64) -> f64 {
    v - (v / 3.3554432e7 + 0.5).floor() * 3.3554432e7
}

impl PerlinNoise {
    pub(crate) fn new(rng: &mut SplitMix64, params: &NoiseParams) -> Self {
        let n = params.amplitudes.len();
        let octaves = params
            .amplitudes
            .iter()
            .map(|&a| if a != 0.0 { Some(ImprovedNoise::new(rng)) } else { rng_skip(rng) })
            .collect();
        // MC: lowestFreqInputFactor = 2^firstOctave;
        //     lowestFreqValueFactor = 2^(n-1) / (2^n - 1).
        let lowest_freq_input = 2f64.powi(params.first_octave);
        let lowest_freq_value = 2f64.powi(n as i32 - 1) / (2f64.powi(n as i32) - 1.0);
        Self {
            octaves,
            amplitudes: params.amplitudes.clone(),
            lowest_freq_input,
            lowest_freq_value,
        }
    }

    pub(crate) fn get(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut value = 0.0;
        let mut input_f = self.lowest_freq_input;
        let mut value_f = self.lowest_freq_value;
        for (oct, &amp) in self.octaves.iter().zip(&self.amplitudes) {
            if let Some(n) = oct {
                value += amp
                    * value_f
                    * n.noise(wrap(x * input_f), wrap(y * input_f), wrap(z * input_f));
            }
            input_f *= 2.0;
            value_f /= 2.0;
        }
        value
    }
}

/// Even for a zero amplitude MC still consumes the RNG for that octave
/// (to keep the later octaves' seeds aligned). Mirror that so adding /
/// removing zero amplitudes doesn't shift the noise.
fn rng_skip(rng: &mut SplitMix64) -> Option<ImprovedNoise> {
    let _ = ImprovedNoise::new(rng);
    None
}

/// MC `NormalNoise`: two PerlinNoise stacks combined toward a normal
/// distribution. `value()` is what the density functions sample.
pub(crate) struct NormalNoise {
    first: PerlinNoise,
    second: PerlinNoise,
    value_factor: f64,
}

/// MC's fixed second-noise input scale (`1.0181268882175227`).
const NORMAL_INPUT_FACTOR: f64 = 1.0181268882175227;

impl NormalNoise {
    pub(crate) fn new(seed: u64, params: &NoiseParams) -> Self {
        let mut rng_a = SplitMix64::new(seed);
        let mut rng_b = SplitMix64::new(seed ^ 0x6A09_E667_F3BC_C909);
        let first = PerlinNoise::new(&mut rng_a, params);
        let second = PerlinNoise::new(&mut rng_b, params);
        // MC: valueFactor = (1/6) / (0.1 * (1 + 1/(n+1))) where n is
        // the amplitude count - normalises the two-stack sum to ~unit.
        let n = params.amplitudes.len() as f64;
        let value_factor = (1.0 / 6.0) / (0.1 * (1.0 + 1.0 / (n + 1.0)));
        Self { first, second, value_factor }
    }

    pub(crate) fn value(&self, x: f64, y: f64, z: f64) -> f64 {
        let a = self.first.get(x, y, z);
        let b = self.second.get(
            x * NORMAL_INPUT_FACTOR,
            y * NORMAL_INPUT_FACTOR,
            z * NORMAL_INPUT_FACTOR,
        );
        (a + b) * self.value_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> NoiseParams {
        // MC's real continentalness params.
        NoiseParams {
            first_octave: -9,
            amplitudes: vec![1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0],
        }
    }

    #[test]
    fn json_loads_into_noise_params() {
        let j = r#"{ "firstOctave": -9, "amplitudes": [1.0, 1.0, 2.0] }"#;
        let p: NoiseParams = serde_json::from_str(j).unwrap();
        assert_eq!(p.first_octave, -9);
        assert_eq!(p.amplitudes, vec![1.0, 1.0, 2.0]);
    }

    #[test]
    fn noise_is_deterministic_for_a_seed() {
        let n1 = NormalNoise::new(42, &params());
        let n2 = NormalNoise::new(42, &params());
        for &(x, y, z) in &[(0.0, 0.0, 0.0), (12.3, -4.0, 99.9), (-50.0, 7.0, 5.0)] {
            assert_eq!(n1.value(x, y, z), n2.value(x, y, z));
        }
    }

    #[test]
    fn different_seeds_differ() {
        let a = NormalNoise::new(1, &params());
        let b = NormalNoise::new(2, &params());
        // Vanishingly unlikely to match across several samples.
        let same = [(0.0, 0.0, 0.0), (10.0, 0.0, 10.0), (5.0, 5.0, 5.0)]
            .iter()
            .all(|&(x, y, z)| a.value(x, y, z) == b.value(x, y, z));
        assert!(!same, "distinct seeds must produce distinct fields");
    }

    #[test]
    fn output_in_reasonable_range() {
        // NormalNoise is normalised to ~unit; sample a grid and assert
        // nothing explodes (the density functions clamp on top anyway).
        let n = NormalNoise::new(7, &params());
        let mut max = 0.0_f64;
        for xi in 0..40 {
            for zi in 0..40 {
                let v = n.value(xi as f64 * 4.0, 0.0, zi as f64 * 4.0);
                max = max.max(v.abs());
            }
        }
        assert!(max > 0.05, "noise should vary, got max {max}");
        assert!(max < 4.0, "noise should stay bounded, got max {max}");
    }

    #[test]
    fn noise_is_continuous() {
        // Adjacent samples must be close (no discontinuities) - the
        // hallmark of smooth Perlin terrain.
        let n = NormalNoise::new(3, &params());
        let a = n.value(100.0, 0.0, 100.0);
        let b = n.value(100.01, 0.0, 100.0);
        assert!((a - b).abs() < 0.05, "noise jumped: {a} -> {b}");
    }
}
