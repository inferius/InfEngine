//! Day/night ramp tests for `compute_day_factor`.

use super::compute_day_factor;

const DAY: f32 = 600.0;

#[test]
fn noon_returns_one() {
    // phase = 0.25 (a quarter into the day) - solidly inside the
    // full-daylight band.
    let f = compute_day_factor(DAY * 0.25, DAY);
    assert!((f - 1.0).abs() < 1e-5, "got {}", f);
}

#[test]
fn early_day_returns_one() {
    // phase = 0.0 (start of the day).
    let f = compute_day_factor(0.0, DAY);
    assert!((f - 1.0).abs() < 1e-5, "got {}", f);
}

#[test]
fn midnight_returns_night_floor() {
    // phase = 0.75 - deep in the night band.
    let f = compute_day_factor(DAY * 0.75, DAY);
    assert!((f - 0.04).abs() < 1e-5, "got {}", f);
}

#[test]
fn full_period_wraps_to_one() {
    // phase = 1.0 wraps to 0.0 -> full daylight.
    let f = compute_day_factor(DAY, DAY);
    assert!((f - 1.0).abs() < 1e-5, "got {}", f);
}

#[test]
fn dawn_is_intermediate() {
    // phase = 0.975 - halfway through the dawn ramp.
    let f = compute_day_factor(DAY * 0.975, DAY);
    assert!(
        f > 0.04 && f < 1.0,
        "expected intermediate value, got {}",
        f
    );
}

#[test]
fn dusk_is_intermediate() {
    // phase = 0.525 - halfway through the dusk ramp.
    let f = compute_day_factor(DAY * 0.525, DAY);
    assert!(
        f > 0.04 && f < 1.0,
        "expected intermediate value, got {}",
        f
    );
}

#[test]
fn dusk_endpoint_below_landing_in_night_band() {
    // phase = 0.55 marks the END of the dusk ramp and the START
    // of the night band -> exactly the night floor.
    let f = compute_day_factor(DAY * 0.55, DAY);
    assert!(
        (f - 0.04).abs() < 1e-5,
        "expected night floor at end of dusk, got {}",
        f
    );
}

#[test]
fn dawn_endpoint_lands_at_full_day() {
    // phase = 1.0 wraps to 0.0 -> full day. The smoothstep at
    // the END of dawn (phase -> 1.0) should approach 1.0.
    // Sample at phase = 0.9999.
    let f = compute_day_factor(DAY * 0.9999, DAY);
    assert!(
        (f - 1.0).abs() < 0.005,
        "expected ~1.0 at end of dawn, got {}",
        f
    );
}

#[test]
fn negative_day_time_still_wraps_via_rem_euclid() {
    // Negative inputs (e.g. paused clock that ran backwards a
    // tick) must not panic and must give the same value as the
    // wrapped positive phase.
    let f_neg = compute_day_factor(-DAY * 0.25, DAY);
    let f_pos = compute_day_factor(DAY * 0.75, DAY);
    assert!((f_neg - f_pos).abs() < 1e-5, "neg={} pos={}", f_neg, f_pos);
}

#[test]
fn zero_day_length_does_not_panic() {
    // A misconfigured `day_length_s = 0.0` must not divide by
    // zero. The function falls back to a 1.0-second period so it
    // returns SOMETHING in [0.04, 1.0].
    let f = compute_day_factor(0.5, 0.0);
    assert!((0.04..=1.0).contains(&f), "got {}", f);
}

#[test]
fn ramp_is_monotonic_through_dusk() {
    // Sample 5 points through the dusk window. day_factor must
    // be monotonically decreasing - if it ever jumps up the user
    // would see a flicker.
    let mut prev = 1.0_f32;
    for i in 0..=5 {
        let phase = 0.5 + (i as f32) * 0.01;
        let f = compute_day_factor(DAY * phase, DAY);
        assert!(
            f <= prev + 1e-5,
            "dusk not monotone at phase {}: {} > {}",
            phase,
            f,
            prev
        );
        prev = f;
    }
}

#[test]
fn ramp_is_monotonic_through_dawn() {
    // Same as dusk but checking dawn is monotonically increasing.
    let mut prev = 0.04_f32;
    for i in 0..=5 {
        let phase = 0.95 + (i as f32) * 0.01;
        let f = compute_day_factor(DAY * phase, DAY);
        assert!(
            f >= prev - 1e-5,
            "dawn not monotone at phase {}: {} < {}",
            phase,
            f,
            prev
        );
        prev = f;
    }
}
