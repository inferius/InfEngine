//! W198 - last per-phase perf snapshot, shared so the F3 debug overlay
//! can show the ms breakdown on screen.
//!
//! The perf diag in [`crate::app_on_update`] already computes the
//! per-phase costs (render / sim / stream / liquids / drawq / mobs) every
//! ~2.5s, but only logs them via `tracing`. The user iterates by
//! screenshotting the HUD rather than reading the console, so we also
//! stash the latest values here (ms x100, packed into an `AtomicU32`)
//! for the overlay to read. Diagnostic only - never on the hot path.

use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) static RENDER_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static SIM_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static STREAM_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static LIQUIDS_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static DRAWQ_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static MOBS_MS: AtomicU32 = AtomicU32::new(0);
/// W198.2: per-frame liquid mesh rebuild (`maybe_rebuild_liquid_mesh`),
/// which is NOT inside any named sim phase - the prime suspect for the
/// untimed `sim_ms` gap while flying over ocean.
pub(crate) static LIQMESH_MS: AtomicU32 = AtomicU32::new(0);
/// W198.4: cumulative ms from frame start at two checkpoints, to localize
/// the still-untimed `sim_ms` gap (standing still: sim ~25ms but every
/// named phase ~0). CHK_MID = before the entity/particle render-prep
/// block; CHK_RPREP = after it. Instantaneous (last frame), so on a stable
/// scene: head+update+stream ~= CHK_MID, render-prep = CHK_RPREP-CHK_MID,
/// drawq+tail = sim-CHK_RPREP.
pub(crate) static CHK_MID_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static CHK_RPREP_MS: AtomicU32 = AtomicU32::new(0);
/// W198.9: split [frame_start -> CHK_MID] further. CHK_HEAD = cumulative
/// up to the start of update_play (head: plugin tick, query drain, input);
/// CHK_WORLD = cumulative after the per-frame tick block (mobs/liquids/
/// drops/fire/ambient). So head=CHK_HEAD, world-tick=CHK_WORLD-CHK_HEAD,
/// pre-render = CHK_MID-CHK_WORLD. Pins the still-untimed ~20ms gap.
pub(crate) static CHK_HEAD_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static CHK_WORLD_MS: AtomicU32 = AtomicU32::new(0);
/// W198.5: the egui UI build (`render_ui`), the biggest discrete untimed
/// call before CHK_MID - prime suspect for the constant ~24ms sim gap.
pub(crate) static UI_MS: AtomicU32 = AtomicU32::new(0);
/// W198.14: lighting dispatch (`tick_lighting`) cost. Runs at ~10 Hz and
/// synchronously clones the affected chunks + face-neighbour halo into the
/// worker snapshot ON THE MAIN THREAD, so during travel (dirty-set always
/// full) it is a periodic spike - previously invisible, lumped into `sim`.
pub(crate) static LIGHT_MS: AtomicU32 = AtomicU32::new(0);
/// W198.14: PEAK (worst single frame) over the diag window for the spiky
/// phases. Averages HIDE stutter - a 25 ms spike 10x/s averages to <0.5 ms
/// yet is felt as judder, which is exactly why the turning/travel stutter
/// kept eluding the averaged counters. These expose the worst frame so a
/// periodic spike is attributable to its phase. Stored raw (not /frames).
pub(crate) static SIM_PEAK_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static STREAM_PEAK_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static LIGHT_PEAK_MS: AtomicU32 = AtomicU32::new(0);

/// Store a ms value (kept to 0.01 ms resolution).
pub(crate) fn store(field: &AtomicU32, ms: f32) {
    field.store((ms.max(0.0) * 100.0) as u32, Ordering::Relaxed);
}

/// Read a ms value back.
pub(crate) fn load(field: &AtomicU32) -> f32 {
    field.load(Ordering::Relaxed) as f32 / 100.0
}
