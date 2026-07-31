//! A26: OPL2 (Yamaha YM3812) emulation for InfEngine.
//!
//! The AdLib card's chip, in software. It exists because a DOS-era
//! soundtrack has three parallel versions - MT-32, AdLib and PC speaker -
//! and only one of them can be made audible without asking the player to
//! supply someone else's ROMs or a soundfont. This is that one.
//!
//! The interface is deliberately the hardware's: write a register number
//! and a value, ask for samples. A music driver ported from the original
//! can then be a faithful port, because the thing it is talking to
//! answers the way the silicon did.
//!
//! It is a *model*, not a gate-level reproduction. The tables, the
//! logarithmic arithmetic, the operator layout and the register map are
//! the hardware's; the envelope generator follows the published
//! rate-to-time relationship rather than reproducing the counter that
//! implemented it. In practice that is inaudible except in the first
//! milliseconds of the fastest attacks.
//!
//! ```
//! use infengine_opl::{Opl2, render};
//!
//! let mut chip = Opl2::new();
//! chip.write(0x20, 0x01); // operator 0: multiple 1
//! chip.write(0x40, 0x00); // full volume
//! chip.write(0x60, 0xF0); // fast attack
//! chip.write(0x80, 0x77);
//! chip.write(0xA0, 0x98); // frequency
//! chip.write(0xB0, 0x31); // block 4, key on
//!
//! let samples = render(&mut chip, 44_100, 4_410); // a tenth of a second
//! assert!(samples.iter().any(|s| *s != 0));
//! ```

#![forbid(unsafe_code)]

mod chip;
mod operator;
mod tables;

pub use chip::{CHANNELS, NATIVE_RATE, OPERATORS, Opl2};
pub use operator::{ENVELOPE_MAX, Phase};
pub use tables::OPERATOR_PEAK;

/// Render `count` samples at `rate`, resampling from the chip's own.
///
/// The chip runs at 49716 Hz whatever the output device wants, so
/// something has to bridge the two. This takes the nearest chip sample
/// rather than interpolating: the OPL2's output is a stepped waveform to
/// begin with, and smoothing it is a way to make it sound like something
/// it never was.
pub fn render(chip: &mut Opl2, rate: u32, count: usize) -> Vec<i16> {
    let mut out = Vec::with_capacity(count);
    let mut position = 0u64;
    let step = (u64::from(NATIVE_RATE) << 16) / u64::from(rate.max(1));
    let mut produced_at = 0u64;
    let mut sample = 0i32;

    for _ in 0..count {
        let want = position >> 16;
        while produced_at <= want {
            sample = chip.sample();
            produced_at += 1;
        }
        out.push(clamp(sample));
        position += step;
    }
    out
}

/// Render until everything has stopped sounding, up to a limit.
///
/// The limit is there because a driver can leave a note held with a
/// release rate of zero, which never decays - a real thing that happens
/// and not a reason to write an endless file.
pub fn render_until_silent(chip: &mut Opl2, rate: u32, max_seconds: f32) -> Vec<i16> {
    let max = (rate as f32 * max_seconds.max(0.0)) as usize;
    let chunk = (rate / 20).max(1) as usize;
    let mut out = Vec::new();
    while out.len() < max {
        out.extend(render(chip, rate, chunk.min(max - out.len())));
        if chip.is_silent() {
            break;
        }
    }
    out
}

/// The chip's headroom is wider than a sample; clipping beats wrapping.
fn clamp(sample: i32) -> i16 {
    sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sounding_chip() -> Opl2 {
        let mut chip = Opl2::new();
        chip.write(0x01, 0x20);
        chip.write(0x20, 0x01);
        chip.write(0x40, 0x00);
        chip.write(0x60, 0xF0);
        chip.write(0x80, 0x0F);
        chip.write(0xC0, 0x01);
        chip.write(0xA0, 0x98);
        chip.write(0xB0, 0x31);
        chip
    }

    #[test]
    fn rendering_produces_exactly_what_was_asked_for() {
        let mut chip = sounding_chip();
        assert_eq!(render(&mut chip, 44_100, 1000).len(), 1000);
        assert_eq!(render(&mut chip, 22_050, 7).len(), 7);
        assert_eq!(render(&mut chip, 44_100, 0).len(), 0);
    }

    #[test]
    fn a_silent_chip_renders_silence() {
        let mut chip = Opl2::new();
        assert!(render(&mut chip, 44_100, 2000).iter().all(|s| *s == 0));
    }

    #[test]
    fn a_lower_output_rate_covers_the_same_stretch_of_time() {
        // Half the rate for half the samples is the same music, so the
        // note has to have got just as far through its envelope.
        let mut fast = sounding_chip();
        let _ = render(&mut fast, 44_100, 4410);
        let mut slow = sounding_chip();
        let _ = render(&mut slow, 22_050, 2205);
        assert_eq!(
            fast.is_silent(),
            slow.is_silent(),
            "resampling must not change how fast time passes"
        );
    }

    #[test]
    fn rendering_until_silent_stops_when_the_note_does() {
        let mut chip = sounding_chip();
        chip.write(0x80, 0xFF); // fastest release
        let _ = render(&mut chip, 44_100, 4410);
        chip.write(0xB0, 0x11); // key off
        let tail = render_until_silent(&mut chip, 44_100, 10.0);
        assert!(chip.is_silent());
        assert!(
            tail.len() < 44_100,
            "a fast release should not take a second: {}",
            tail.len()
        );
    }

    #[test]
    fn a_note_that_never_decays_still_hits_the_limit() {
        let mut chip = sounding_chip();
        chip.write(0x80, 0x00); // release rate zero: it rings for ever
        let held = render_until_silent(&mut chip, 8_000, 0.5);
        assert_eq!(held.len(), 4_000, "the limit is what ends it");
        assert!(!chip.is_silent());
    }

    #[test]
    fn loud_output_clips_rather_than_wrapping() {
        assert_eq!(clamp(i32::MAX), i16::MAX);
        assert_eq!(clamp(i32::MIN), i16::MIN);
        assert_eq!(clamp(-5), -5);
    }
}
