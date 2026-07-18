//! W65.4 / W294 - crop growth plumbing for the world pipelines.
//!
//! The 2-stage approximation is gone: stages, MC growth data and the
//! harvest loot live in [`crate::block::crop_stages`] (block tree, so
//! the worldgen blockmap resolves `wheat[age=N]` template states
//! too). This module keeps the thin API the place/break pipelines +
//! random tick were already calling.

pub(crate) use crate::block::crop_stages::{
    immature_for, is_crop, is_immature_crop, next_stage,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immature_and_mature_round_trip() {
        for mature in [493u16, 494, 495, 496] {
            let imm = immature_for(mature).expect("crop has an age-0 stage");
            assert!(is_immature_crop(imm));
            // Walking the chain from age 0 ends at the mature id.
            let mut id = imm;
            while let Some(next) = next_stage(id) {
                id = next;
            }
            assert_eq!(id, mature);
            assert!(crate::block::crop_stages::is_mature_crop(id));
        }
    }

    #[test]
    fn non_crop_ids_map_to_none() {
        assert_eq!(immature_for(1), None); // stone
        assert!(!is_immature_crop(1));
        assert!(!is_crop(1));
        // A mature crop is not itself an immature stage.
        assert!(!is_immature_crop(493));
    }
}
