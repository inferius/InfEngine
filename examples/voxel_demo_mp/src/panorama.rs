//! Wave 12.6 - Menu panorama backdrop.
//!
//! MC's main menu draws a slowly-rotating cubemap backdrop sampled
//! from `assets/minecraft/textures/gui/title/background/panorama_0..5.png`.
//! This module wires the discovery + rotation math; the actual
//! egui-side draw + GPU cube mesh is layered on top by the menu
//! screen renderer.
//!
//! Per W12 design decisions: panorama rotation speed is 1 RPM
//! (matches MC). Fallback when no MC assets present: keep the
//! existing dark-navy clear colour so the menu still renders on a
//! clean machine.
//!
//! Cite `J:\Temp\template-mod-template-26.1.2\sources\
//! minecraft-merged-52430b475d-26.1.2-sources\
//! net\minecraft\client\renderer\PanoramaRenderer.java` - MC's
//! panorama cube renderer + per-face mapping.

use std::path::{Path, PathBuf};

/// The 6 cube face file stems MC ships under
/// `textures/gui/title/background/`. Order matters: PanoramaRenderer
/// assigns each index to a specific cube face.
pub(crate) const PANORAMA_FACE_NAMES: [&str; 6] = [
    "panorama_0", // south / -Z
    "panorama_1", // west / -X
    "panorama_2", // north / +Z
    "panorama_3", // east / +X
    "panorama_4", // up / +Y
    "panorama_5", // down / -Y
];

/// MC main-menu panorama rotation speed. 1 full rotation per
/// minute = 360 deg / 60 s = 6 deg/sec = 0.10472 rad/sec.
pub(crate) const PANORAMA_RPM: f32 = 1.0;
pub(crate) const PANORAMA_ROT_RATE_RAD_PER_SEC: f32 =
    PANORAMA_RPM * std::f32::consts::TAU / 60.0;

/// Compute the Y-axis rotation (radians) for a given elapsed time.
/// Wraps via rem_euclid so an arbitrarily-long elapsed time stays
/// numerically stable.
pub(crate) fn rotation_y_at(elapsed_s: f32) -> f32 {
    let raw = elapsed_s * PANORAMA_ROT_RATE_RAD_PER_SEC;
    raw.rem_euclid(std::f32::consts::TAU)
}

/// Probe the user's MC install for the panorama assets. Walks
/// every "background" or "title/background" sub-directory under
/// `assets_root` and returns the 6 face paths in `PANORAMA_FACE_NAMES`
/// order if all 6 resolve.
pub(crate) fn discover_panorama_faces(assets_root: &Path) -> Option<[PathBuf; 6]> {
    let candidates = [
        assets_root
            .join("textures")
            .join("gui")
            .join("title")
            .join("background"),
        assets_root
            .join("textures")
            .join("gui")
            .join("background"),
        // Some older MC installs flatten the path; the W9 path
        // discovery already tries this layout for other GUI assets.
        assets_root
            .join("minecraft")
            .join("textures")
            .join("gui")
            .join("title")
            .join("background"),
    ];
    for dir in candidates {
        if let Some(paths) = try_discover_in(&dir) {
            return Some(paths);
        }
    }
    None
}

fn try_discover_in(dir: &Path) -> Option<[PathBuf; 6]> {
    let mut out: [Option<PathBuf>; 6] = Default::default();
    for (i, name) in PANORAMA_FACE_NAMES.iter().enumerate() {
        let p = dir.join(format!("{name}.png"));
        if p.is_file() {
            out[i] = Some(p);
        } else {
            return None;
        }
    }
    let resolved: Vec<PathBuf> = out.into_iter().flatten().collect();
    if resolved.len() != 6 {
        return None;
    }
    let arr: [PathBuf; 6] = resolved.try_into().ok()?;
    Some(arr)
}

/// Cube-face index -> direction vector (unit). Matches MC's
/// PanoramaRenderer mapping so the sampling code can pick the
/// right face for a given view direction.
pub(crate) fn face_direction(face_idx: usize) -> [f32; 3] {
    match face_idx {
        0 => [0.0, 0.0, -1.0], // south / -Z
        1 => [-1.0, 0.0, 0.0], // west  / -X
        2 => [0.0, 0.0, 1.0],  // north / +Z
        3 => [1.0, 0.0, 0.0],  // east  / +X
        4 => [0.0, 1.0, 0.0],  // up    / +Y
        5 => [0.0, -1.0, 0.0], // down  / -Y
        _ => [0.0, 0.0, 0.0],
    }
}

/// Fallback clear colour when no panorama assets are present.
/// Matches the pre-W12 menu's dark-navy backdrop.
pub(crate) const FALLBACK_CLEAR_COLOR: [f32; 4] = [0.05, 0.06, 0.10, 1.0];

/// Resolved panorama state ready for the menu renderer. `paths`
/// holds the 6 face PNG paths the texture loader pulls in;
/// `rotation_y` is the current frame's Y-axis rotation (radians)
/// derived from elapsed-since-menu-shown.
#[derive(Debug, Clone)]
pub(crate) struct PanoramaState {
    pub(crate) paths: [PathBuf; 6],
    pub(crate) rotation_y: f32,
}

impl PanoramaState {
    /// Build from a discovery result + elapsed time. Returns
    /// `None` when discovery failed - caller falls back to the
    /// dark-navy clear colour.
    pub(crate) fn build(
        assets_root: &Path,
        elapsed_s: f32,
    ) -> Option<PanoramaState> {
        let paths = discover_panorama_faces(assets_root)?;
        Some(Self {
            paths,
            rotation_y: rotation_y_at(elapsed_s),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn panorama_loaded_when_mc_assets_present() {
        let tmp = std::env::temp_dir().join(format!(
            "infengine_panorama_test_{}",
            std::process::id()
        ));
        let dir = tmp
            .join("textures")
            .join("gui")
            .join("title")
            .join("background");
        fs::create_dir_all(&dir).expect("mkdir");
        for name in PANORAMA_FACE_NAMES {
            let p = dir.join(format!("{name}.png"));
            // Touch each file with a tiny placeholder PNG header.
            fs::write(&p, b"\x89PNG\r\n\x1a\n").expect("write");
        }
        let paths = discover_panorama_faces(&tmp)
            .expect("panorama should discover when all 6 faces present");
        assert_eq!(paths.len(), 6);
        for (i, p) in paths.iter().enumerate() {
            let name = PANORAMA_FACE_NAMES[i];
            assert!(
                p.file_stem().and_then(|s| s.to_str()) == Some(name),
                "face {i} resolved to wrong stem: {p:?}",
            );
        }
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn panorama_falls_back_to_dark_navy_when_absent() {
        // Empty temp dir + missing panorama files.
        let tmp = std::env::temp_dir().join(format!(
            "infengine_panorama_absent_{}",
            std::process::id()
        ));
        fs::create_dir_all(&tmp).expect("mkdir");
        assert!(discover_panorama_faces(&tmp).is_none());
        // Fallback colour is the dark-navy used pre-W12.
        let fb = FALLBACK_CLEAR_COLOR;
        assert!(fb[0] < 0.2 && fb[1] < 0.2 && fb[2] < 0.2);
        assert!((fb[3] - 1.0).abs() < 1e-3);
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn rotation_speed_is_1_rpm() {
        // 60 seconds -> one full rotation.
        let r60 = rotation_y_at(60.0);
        // Tiny float wobble; tau wrap brings the value close to 0.
        let near_zero = r60 < 0.01;
        let near_tau = r60 > std::f32::consts::TAU - 0.01;
        assert!(
            near_zero || near_tau,
            "60s rotation should be near a full turn, got {r60}",
        );
    }

    #[test]
    fn rotation_at_15s_is_quarter_turn() {
        // 15s = 1/4 minute -> tau / 4.
        let r = rotation_y_at(15.0);
        let expected = std::f32::consts::TAU / 4.0;
        assert!(
            approx_eq(r, expected, 1e-3),
            "rotation at 15s should be quarter-turn, got {r} (want {expected})",
        );
    }

    #[test]
    fn rotation_wraps_via_rem_euclid() {
        // 600 seconds = 10 full rotations -> wraps to near 0.
        let r600 = rotation_y_at(600.0);
        let near_zero = r600 < 0.01;
        let near_tau = r600 > std::f32::consts::TAU - 0.01;
        assert!(
            near_zero || near_tau,
            "10x rotation should wrap to 0, got {r600}",
        );
    }

    #[test]
    fn cube_face_directions_are_unit() {
        for i in 0..6 {
            let d = face_direction(i);
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            assert!(approx_eq(len, 1.0, 1e-3), "face {i} direction not unit");
        }
    }

    #[test]
    fn cube_face_directions_are_orthogonal_pairs() {
        // face 0 (south) and face 2 (north) should be antipodal.
        let s = face_direction(0);
        let n = face_direction(2);
        assert!(approx_eq(s[0] + n[0], 0.0, 1e-3));
        assert!(approx_eq(s[2] + n[2], 0.0, 1e-3));
        let w = face_direction(1);
        let e = face_direction(3);
        assert!(approx_eq(w[0] + e[0], 0.0, 1e-3));
        let u = face_direction(4);
        let d = face_direction(5);
        assert!(approx_eq(u[1] + d[1], 0.0, 1e-3));
    }

    #[test]
    fn panorama_state_builds_from_full_install() {
        let tmp = std::env::temp_dir().join(format!(
            "infengine_panorama_state_{}",
            std::process::id()
        ));
        let dir = tmp
            .join("textures")
            .join("gui")
            .join("title")
            .join("background");
        fs::create_dir_all(&dir).expect("mkdir");
        for name in PANORAMA_FACE_NAMES {
            fs::write(dir.join(format!("{name}.png")), b"x").expect("write");
        }
        let st = PanoramaState::build(&tmp, 30.0).expect("build");
        // At 30s = halfway through one rotation -> tau / 2.
        let expected = std::f32::consts::TAU / 2.0;
        assert!(approx_eq(st.rotation_y, expected, 1e-3));
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn panorama_state_none_on_partial_install() {
        // Only 5 of 6 faces - discovery should fail.
        let tmp = std::env::temp_dir().join(format!(
            "infengine_panorama_partial_{}",
            std::process::id()
        ));
        let dir = tmp
            .join("textures")
            .join("gui")
            .join("title")
            .join("background");
        fs::create_dir_all(&dir).expect("mkdir");
        for name in &PANORAMA_FACE_NAMES[..5] {
            fs::write(dir.join(format!("{name}.png")), b"x").expect("write");
        }
        assert!(PanoramaState::build(&tmp, 0.0).is_none());
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn rate_constant_matches_1_rpm() {
        // PANORAMA_ROT_RATE_RAD_PER_SEC * 60 = tau exactly.
        let one_minute = PANORAMA_ROT_RATE_RAD_PER_SEC * 60.0;
        assert!(approx_eq(one_minute, std::f32::consts::TAU, 1e-3));
    }
}
