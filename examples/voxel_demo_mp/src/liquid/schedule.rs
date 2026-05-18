//! MC-style per-cell liquid tick schedule.
//!
//! Each scheduled cell carries a `due_at_tick` timestamp (in the
//! shared `liquid_clock` units, one click per net-tick frame). The
//! min-heap pops cells in earliest-due-first order; the parallel
//! `pending` set lets the world hot-paths (placement, neighbour
//! wake-up) avoid double-queueing the same cell without scanning
//! the whole heap.
//!
//! This is the structural twin of MC's `ServerLevel.scheduleTick`:
//!
//! ```text
//! ServerLevel.scheduleTick(pos, fluid, delay)
//!     -> pos becomes due at gameTime + delay
//! tick(level) -> pops every due entry, calls FlowingFluid.tick(pos)
//! ```
//!
//! Cap is gone: a giant pool just has many entries, and each frame
//! processes at most `MAX_LIQUID_PER_FRAME` of them. Cells that
//! don't fit this frame keep their due time and tick on the next.
//!
//! `cancel` is best-effort: removing a position from the pending
//! set is O(1), but the heap entry itself sticks around until it
//! pops. When `pop_due` finds an entry whose position is no longer
//! in `pending`, it skips it. This keeps the hot path lean - we
//! never linear-scan the heap.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

/// One scheduled tick. `Ord` walks `due_at_tick` first so the
/// `BinaryHeap<Reverse<_>>` behaves like a min-heap by due time.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ScheduledTick {
    due_at_tick: u64,
    pos: [i32; 3],
}

impl PartialOrd for ScheduledTick {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScheduledTick {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.due_at_tick
            .cmp(&other.due_at_tick)
            .then_with(|| self.pos.cmp(&other.pos))
    }
}

/// Heap-backed schedule of pending liquid ticks. Each entry is a
/// `(due_at_tick, pos)` pair; the parallel `pending` set guards
/// double-scheduling.
#[derive(Debug, Default)]
pub(crate) struct LiquidSchedule {
    heap: BinaryHeap<Reverse<ScheduledTick>>,
    /// Cells currently waiting in `heap`. A cell is "pending" from
    /// the moment `schedule` accepts it until `pop_due` returns it.
    /// `world_set_tracked` and the tick path both treat this as the
    /// drop-in replacement for the old `liquid_active` HashSet.
    pending: HashSet<[i32; 3]>,
}

impl LiquidSchedule {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Queue `pos` for a tick at `when`. If the cell is already
    /// pending, the earlier of the two due times wins (we just push
    /// a new heap entry - the older entry pops first and the later
    /// one pops as a tombstone since `pending` is no longer set).
    pub(crate) fn schedule(&mut self, pos: [i32; 3], when: u64) {
        // Always push: even when the cell is already pending, the
        // new entry may be earlier. The pop loop skips entries
        // whose pos has been cancelled or already consumed.
        self.heap.push(Reverse(ScheduledTick {
            due_at_tick: when,
            pos,
        }));
        self.pending.insert(pos);
    }

    /// Pop the next cell whose `due_at_tick <= now`, skipping any
    /// stale entries (cancelled positions or duplicates whose
    /// `pending` flag was already consumed). Returns `None` when no
    /// due entry remains.
    #[allow(dead_code)]
    pub(crate) fn pop_due(&mut self, now: u64) -> Option<[i32; 3]> {
        while let Some(Reverse(top)) = self.heap.peek().copied() {
            if top.due_at_tick > now {
                return None;
            }
            self.heap.pop();
            // Skip tombstones: the pos is not pending anymore.
            if self.pending.remove(&top.pos) {
                return Some(top.pos);
            }
        }
        None
    }

    /// Drop `pos` from the pending set. The matching heap entry
    /// becomes a tombstone consumed on the next `pop_due` skip.
    pub(crate) fn cancel(&mut self, pos: [i32; 3]) {
        self.pending.remove(&pos);
    }

    /// Number of cells currently pending. Slightly larger than the
    /// number of distinct positions because the heap may carry
    /// stale entries that haven't been popped yet, but the pending
    /// set is the authoritative count.
    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        self.pending.len()
    }

    /// True if `pos` is currently queued for a future tick.
    #[allow(dead_code)]
    pub(crate) fn contains(&self, pos: [i32; 3]) -> bool {
        self.pending.contains(&pos)
    }

    /// Iterate every position currently waiting in the schedule.
    /// Order is HashSet iteration order - tests rely on the set
    /// membership, not the sequence.
    #[allow(dead_code)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = [i32; 3]> + '_ {
        self.pending.iter().copied()
    }

    /// Drop every pending entry. Used by `reset_world` so a fresh
    /// world doesn't inherit stale schedule entries from the old.
    pub(crate) fn clear(&mut self) {
        self.heap.clear();
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_schedule_pops_none() {
        let mut s = LiquidSchedule::new();
        assert_eq!(s.pop_due(0), None);
        assert_eq!(s.pop_due(1000), None);
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn schedule_then_pop_due_at_time() {
        let mut s = LiquidSchedule::new();
        s.schedule([1, 2, 3], 10);
        // Too early: nothing pops.
        assert_eq!(s.pop_due(9), None);
        // Right at the due tick: pops.
        assert_eq!(s.pop_due(10), Some([1, 2, 3]));
        // Drained: nothing left.
        assert_eq!(s.pop_due(10), None);
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn pop_order_is_earliest_due_first() {
        let mut s = LiquidSchedule::new();
        s.schedule([0, 0, 0], 100);
        s.schedule([1, 1, 1], 50);
        s.schedule([2, 2, 2], 75);
        // Pop everything at time 200.
        let mut order = Vec::new();
        while let Some(p) = s.pop_due(200) {
            order.push(p);
        }
        assert_eq!(order, vec![[1, 1, 1], [2, 2, 2], [0, 0, 0]]);
    }

    #[test]
    fn rescheduling_pos_takes_earlier_of_two() {
        let mut s = LiquidSchedule::new();
        s.schedule([5, 5, 5], 100);
        // Re-schedule earlier - the older entry pops first and
        // consumes the pending flag, the later entry becomes a
        // tombstone.
        s.schedule([5, 5, 5], 20);
        // At time 30 we want the cell to fire (because its
        // EARLIEST schedule was 20).
        assert_eq!(s.pop_due(30), Some([5, 5, 5]));
        // And it doesn't fire twice.
        assert_eq!(s.pop_due(200), None);
    }

    #[test]
    fn cancel_drops_pending_pos() {
        let mut s = LiquidSchedule::new();
        s.schedule([1, 0, 0], 10);
        s.schedule([2, 0, 0], 10);
        s.cancel([1, 0, 0]);
        assert!(!s.contains([1, 0, 0]));
        assert!(s.contains([2, 0, 0]));
        // Pop only yields the non-cancelled one.
        let mut got = Vec::new();
        while let Some(p) = s.pop_due(20) {
            got.push(p);
        }
        assert_eq!(got, vec![[2, 0, 0]]);
    }

    #[test]
    fn clear_drops_every_entry() {
        let mut s = LiquidSchedule::new();
        for i in 0..10 {
            s.schedule([i, 0, 0], (i as u64) * 5);
        }
        assert_eq!(s.len(), 10);
        s.clear();
        assert_eq!(s.len(), 0);
        assert_eq!(s.pop_due(1000), None);
    }

    #[test]
    fn len_matches_distinct_pending() {
        let mut s = LiquidSchedule::new();
        s.schedule([0, 0, 0], 10);
        s.schedule([0, 0, 0], 20);
        s.schedule([0, 0, 0], 30);
        // Same pos scheduled thrice - len counts the distinct
        // pending positions, not the heap depth.
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn large_workload_pops_in_order() {
        // Mirror the per-frame budget pattern: schedule many
        // cells with varying due times, then drain a few at a
        // time and assert ordering holds.
        let mut s = LiquidSchedule::new();
        let n = 1000;
        for i in 0..n {
            // Two ticks staggered so pops happen across two
            // "frames".
            let due = if i % 2 == 0 { 5 } else { 10 };
            s.schedule([i as i32, 0, 0], due);
        }
        let mut at_5 = 0;
        while s.pop_due(5).is_some() {
            at_5 += 1;
        }
        let mut at_10 = 0;
        while s.pop_due(10).is_some() {
            at_10 += 1;
        }
        assert_eq!(at_5 + at_10, n);
        assert_eq!(at_5, n / 2);
        assert_eq!(s.len(), 0);
    }
}
