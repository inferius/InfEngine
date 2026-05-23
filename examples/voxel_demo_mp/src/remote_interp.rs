//! W47.27 - RemoteInterp (relocated from main.rs).
#![allow(unused_imports)]
use crate::*;

/// Two-sample interpolation buffer for one remote player's pose.
/// The render path samples at `now - INTERP_DELAY` so the most
/// recent snapshot is one tick "in the future" and we always have
/// the surrounding pair to lerp between.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RemoteInterp {
    pub(crate) prev: PlayerMsg,
    pub(crate) current: PlayerMsg,
    pub(crate) prev_t: std::time::Instant,
    pub(crate) current_t: std::time::Instant,
}

/// Render delay (seconds) applied to the interpolation buffer. A
/// little less than a typical send interval - lets the lerp always
/// have both endpoints without showing a noticeable lag.
pub(crate) const INTERP_DELAY: f32 = 0.10;

impl RemoteInterp {
    pub(crate) fn new(p: PlayerMsg) -> Self {
        let now = std::time::Instant::now();
        Self {
            prev: p,
            current: p,
            prev_t: now,
            current_t: now,
        }
    }

    pub(crate) fn push(&mut self, p: PlayerMsg) {
        let now = std::time::Instant::now();
        self.prev = self.current;
        self.prev_t = self.current_t;
        self.current = p;
        self.current_t = now;
    }

    /// Pose interpolated at "now minus the render delay", clamped
    /// to the available buffer so we don't extrapolate past the
    /// last snapshot when the sender pauses.
    pub(crate) fn sample(&self, now: std::time::Instant) -> PlayerMsg {
        let render_t = now - std::time::Duration::from_secs_f32(INTERP_DELAY);
        let span = self
            .current_t
            .saturating_duration_since(self.prev_t)
            .as_secs_f32()
            .max(1e-4);
        let into = render_t
            .saturating_duration_since(self.prev_t)
            .as_secs_f32();
        let t = (into / span).clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        // Shortest-arc lerp for yaw + pitch so wrap-around (PI -> -PI)
        // doesn't whip the camera the long way around.
        let lerp_angle = |a: f32, b: f32| {
            let mut d = b - a;
            let tau = std::f32::consts::TAU;
            while d > std::f32::consts::PI {
                d -= tau;
            }
            while d < -std::f32::consts::PI {
                d += tau;
            }
            a + d * t
        };
        PlayerMsg {
            id: self.current.id,
            pos: [
                lerp(self.prev.pos[0], self.current.pos[0]),
                lerp(self.prev.pos[1], self.current.pos[1]),
                lerp(self.prev.pos[2], self.current.pos[2]),
            ],
            yaw: lerp_angle(self.prev.yaw, self.current.yaw),
            pitch: lerp_angle(self.prev.pitch, self.current.pitch),
        }
    }
}
