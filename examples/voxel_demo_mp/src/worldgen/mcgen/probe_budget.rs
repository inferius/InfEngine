//! W383 - cold-build budget for MAIN-THREAD structure probes.
//!
//! The mob-spawn structure overrides (`pos_in_outpost` /
//! `pos_in_monument` / `pos_in_mansion`, W378-W380) solve and cache
//! structure layouts on demand. The first spawn wave after entering
//! a world probed a cold cache, so every candidate start cell paid
//! its full gate cost (dozens of climate-noise biome samples, the
//! mansion's 6x6 `mc_floor_at` surface solve, the outpost's village
//! exclusion) SYNCHRONOUSLY inside the mob tick - the R21 log's
//! one-off `W230 slow world-tick phase ms=2571 phase="mobs"` hitch.
//!
//! Fix: on the frame thread a cache MISS may run at most ONE layout
//! build per 50 ms window; further cold cells report "not in a
//! structure" for that call WITHOUT caching, so they retry next
//! window and the cache warms over a few ticks (~10 cells around a
//! player = warm in ~0.5 s) with no perceptible frame cost. Worker
//! threads (chunk gen carving + chest loot need layouts NOW and run
//! off-frame) are exempt - `allow_build` is always true there.

use std::cell::Cell;
use std::time::Instant;

thread_local! {
    static IS_FRAME_THREAD: Cell<bool> = const { Cell::new(false) };
    static WINDOW: Cell<Option<(Instant, u32)>> = const { Cell::new(None) };
}

const WINDOW_MS: u128 = 50;
const BUILDS_PER_WINDOW: u32 = 1;

/// Mark the calling thread as the frame thread. Called from the
/// spawn tick (the only frame-thread probe caller); idempotent.
pub(crate) fn mark_frame_thread() {
    IS_FRAME_THREAD.with(|f| f.set(true));
}

/// May this thread run a cold layout build right now? Non-frame
/// threads: always. Frame thread: `BUILDS_PER_WINDOW` per
/// `WINDOW_MS`, counted here.
pub(crate) fn allow_build() -> bool {
    if !IS_FRAME_THREAD.with(|f| f.get()) {
        return true;
    }
    WINDOW.with(|w| {
        let now = Instant::now();
        match w.get() {
            Some((t0, used))
                if now.duration_since(t0).as_millis() < WINDOW_MS =>
            {
                if used < BUILDS_PER_WINDOW {
                    w.set(Some((t0, used + 1)));
                    true
                } else {
                    false
                }
            }
            _ => {
                w.set(Some((now, 1)));
                true
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_threads_are_never_throttled() {
        // Fresh spawned thread = fresh thread_locals, unmarked.
        std::thread::spawn(|| {
            for _ in 0..16 {
                assert!(allow_build());
            }
        })
        .join()
        .unwrap();
    }

    #[test]
    fn frame_thread_gets_one_build_per_window() {
        std::thread::spawn(|| {
            mark_frame_thread();
            assert!(allow_build());
            // Immediately after (same 50 ms window): throttled.
            assert!(!allow_build());
            assert!(!allow_build());
        })
        .join()
        .unwrap();
    }
}
