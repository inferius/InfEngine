//! W65.4 - crop growth (immature -> mature).
//!
//! The existing crop blocks (wheat 493, carrots 494, potatoes 495,
//! beetroots 496) are the *mature* stage. W65.4 adds one immature
//! stage per crop (ids 646..=649) that mirrors the mature block's
//! render path but shows an early-stage sprite. Planting substitutes
//! the immature block for the mature crop item, and a random tick
//! advances immature -> mature once the crop has had time on soil.
//!
//! This is a 2-stage approximation of MC's 8-stage wheat / 4-stage
//! root crops - enough to make crops visibly grow over time without a
//! farmland + seed-item + per-stage cross-model pass, which is filed
//! as a follow-up.

use crate::block::{
    BEETROOTS_IMMATURE_ID, CARROTS_IMMATURE_ID, POTATOES_IMMATURE_ID,
    WHEAT_IMMATURE_ID,
};

/// Mature crop block ids (the pre-existing fully-grown crops).
const WHEAT: u16 = 493;
const CARROTS: u16 = 494;
const POTATOES: u16 = 495;
const BEETROOTS: u16 = 496;

/// The immature block a freshly planted `mature` crop should start
/// as, or `None` when `mature` isn't a crop. Used by the place
/// pipeline to swap a planted crop for its young stage.
pub(crate) fn immature_for(mature: u16) -> Option<u16> {
    match mature {
        WHEAT => Some(WHEAT_IMMATURE_ID),
        CARROTS => Some(CARROTS_IMMATURE_ID),
        POTATOES => Some(POTATOES_IMMATURE_ID),
        BEETROOTS => Some(BEETROOTS_IMMATURE_ID),
        _ => None,
    }
}

/// The mature crop an `immature` block grows into, or `None` when the
/// id isn't an immature crop stage.
pub(crate) fn mature_for(immature: u16) -> Option<u16> {
    match immature {
        WHEAT_IMMATURE_ID => Some(WHEAT),
        CARROTS_IMMATURE_ID => Some(CARROTS),
        POTATOES_IMMATURE_ID => Some(POTATOES),
        BEETROOTS_IMMATURE_ID => Some(BEETROOTS),
        _ => None,
    }
}

/// True for an immature crop stage (the random-tick growth subject
/// and the no-drop case in the break pipeline).
pub(crate) fn is_immature_crop(id: u16) -> bool {
    mature_for(id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immature_and_mature_round_trip() {
        for mature in [WHEAT, CARROTS, POTATOES, BEETROOTS] {
            let imm = immature_for(mature).expect("crop has an immature stage");
            assert_eq!(mature_for(imm), Some(mature));
            assert!(is_immature_crop(imm));
        }
    }

    #[test]
    fn non_crop_ids_map_to_none() {
        assert_eq!(immature_for(1), None); // stone
        assert_eq!(mature_for(1), None);
        assert!(!is_immature_crop(1));
        // A mature crop is not itself an immature stage.
        assert!(!is_immature_crop(WHEAT));
    }
}
