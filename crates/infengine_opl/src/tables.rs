//! A26.1: the chip's lookup tables, built the way the silicon did.
//!
//! The OPL2 does all its arithmetic in the logarithmic domain: a sine is
//! read as an attenuation, attenuations add instead of multiplying, and
//! only at the very end does an exponential table turn the result back
//! into a sample. Reproducing that exactly matters - doing the same sums
//! in linear floating point gives a waveform that is *close*, and close
//! is audible on a chip whose whole character is its rounding.
//!
//! The two big tables are computed from their defining formulas rather
//! than transcribed. A 256-entry table copied by hand is a table with a
//! typo in it somewhere, and a typo here is a quiet wrong note nobody
//! can find later.

use std::sync::LazyLock;

/// Quarter-period sine as attenuation: `-log2(sin(x)) * 256`.
///
/// Index is a quarter phase; the other three quarters are mirrored,
/// which is what the hardware does and why only 256 entries exist.
static LOG_SIN: LazyLock<[u16; 256]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let angle = (i as f64 + 0.5) * std::f64::consts::PI / 512.0;
        (-angle.sin().log2() * 256.0).round() as u16
    })
});

/// Exponential table: `(2^((255-i)/256) - 1) * 1024`, descending.
static EXP: LazyLock<[u16; 256]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let value = (2.0f64.powf((255 - i) as f64 / 256.0) - 1.0) * 1024.0;
        value.round() as u16
    })
});

pub fn log_sin(index: usize) -> u16 {
    LOG_SIN[index & 0xFF]
}

pub fn exp(index: usize) -> u16 {
    EXP[index & 0xFF]
}

/// Turn a total attenuation into a signed sample.
///
/// `attenuation` is in the chip's units, 256 per halving. The low byte
/// indexes the table and the rest is a shift, which is how the hardware
/// covers a huge dynamic range with 256 entries.
pub fn attenuation_to_sample(attenuation: u32, negative: bool) -> i32 {
    let attenuation = attenuation.min(0x1FFF);
    let value = (i32::from(exp((attenuation & 0xFF) as usize)) + 1024) >> (attenuation >> 8);
    if negative { -value } else { value }
}

/// Loudest a single operator can be, which is what a mixer needs to know
/// to size its headroom.
pub const OPERATOR_PEAK: i32 = 2042;

/// Key-scale attenuation per block, in 1.5 dB steps, indexed by the top
/// four bits of the frequency number.
pub static KEY_SCALE_LEVEL: [u8; 16] =
    [0, 32, 40, 45, 48, 51, 53, 55, 56, 58, 59, 60, 61, 62, 63, 64];

/// Vibrato depth in 1/64 semitone steps, indexed by the LFO position.
pub static VIBRATO: [[i8; 8]; 2] = [
    // Shallow: what the chip does with the depth bit clear.
    [0, 0, 1, 0, 0, 0, -1, 0],
    // Deep.
    [0, 1, 2, 1, 0, -1, -2, -1],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_sin_table_matches_the_hardware_at_both_ends() {
        // Published values, which is the check that the formula is the
        // right formula and not merely a plausible one.
        assert_eq!(LOG_SIN[0], 2137);
        assert_eq!(LOG_SIN[1], 1731);
        assert_eq!(LOG_SIN[2], 1543);
        assert_eq!(LOG_SIN[255], 0);
    }

    #[test]
    fn the_log_sin_table_descends_all_the_way() {
        // A quarter sine goes from zero amplitude - maximum attenuation
        // - to full, so the table has to fall monotonically.
        for index in 1..256 {
            assert!(
                LOG_SIN[index - 1] >= LOG_SIN[index],
                "rises at {index}: {} -> {}",
                LOG_SIN[index - 1],
                LOG_SIN[index]
            );
        }
    }

    #[test]
    fn the_exponential_table_matches_the_hardware_at_both_ends() {
        assert_eq!(EXP[0], 1018);
        assert_eq!(EXP[1], 1013);
        assert_eq!(EXP[255], 0);
        for index in 1..256 {
            assert!(EXP[index - 1] >= EXP[index]);
        }
    }

    #[test]
    fn full_attenuation_is_silence() {
        assert_eq!(attenuation_to_sample(0x1800, false), 0);
        assert_eq!(attenuation_to_sample(0xFFFF, true), 0);
    }

    #[test]
    fn no_attenuation_is_the_loudest_sample() {
        assert_eq!(attenuation_to_sample(0, false), OPERATOR_PEAK);
        assert_eq!(attenuation_to_sample(0, true), -OPERATOR_PEAK);
        assert!(attenuation_to_sample(0x200, false) < OPERATOR_PEAK);
    }

    #[test]
    fn every_step_of_256_halves_the_amplitude() {
        // The whole point of the logarithmic domain: adding attenuation
        // is multiplying amplitude.
        let mut previous = attenuation_to_sample(0, false);
        for step in 1..8 {
            let value = attenuation_to_sample(step * 0x100, false);
            assert!(
                (previous / 2 - value).abs() <= 1,
                "step {step}: {previous} -> {value}"
            );
            previous = value;
        }
    }

    #[test]
    fn a_quarter_sine_reconstructs_the_real_thing() {
        // Round-trip the tables against the sine they encode. Within a
        // percent is what the chip's own quantisation allows.
        for index in (8..256).step_by(9) {
            let expected = ((index as f64 + 0.5) * std::f64::consts::PI / 512.0).sin();
            let got = f64::from(attenuation_to_sample(u32::from(LOG_SIN[index]), false))
                / f64::from(OPERATOR_PEAK);
            assert!(
                (got - expected).abs() < 0.01,
                "index {index}: {got} vs {expected}"
            );
        }
    }

    #[test]
    fn key_scale_levels_rise_with_the_block() {
        for pair in KEY_SCALE_LEVEL.windows(2) {
            assert!(pair[0] <= pair[1]);
        }
        assert_eq!(KEY_SCALE_LEVEL[0], 0, "the lowest octave is not scaled");
    }

    #[test]
    fn vibrato_is_balanced_over_a_cycle() {
        for depth in VIBRATO {
            let sum: i32 = depth.iter().map(|v| i32::from(*v)).sum();
            assert_eq!(sum, 0, "vibrato that does not average out detunes a note");
        }
    }
}
