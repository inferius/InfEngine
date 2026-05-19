//! `.mcmeta` animation metadata parser.
//!
//! MC ships per-texture animation data in a sibling `.mcmeta` file:
//!
//! ```json
//! {
//!   "animation": {
//!     "frametime": 2,
//!     "interpolate": false,
//!     "frames": [0, 1, 2, 3, 4, 5, 6, 7]
//!   }
//! }
//! ```
//!
//! `frametime` is in game-ticks (50 ms each); `frames` may be either
//! a list of frame indices into the vertically-stacked source PNG
//! OR a list of `{ "index": N, "time": M }` overrides. We support
//! the simple index-list form for W9.2 + treat the override form as
//! a degenerate equal-frametime walk.
//!
//! When the file is missing OR the JSON cannot be parsed, callers
//! treat the texture as static (frames = 1, frametime_ms = 0).

use std::path::Path;

/// Parsed animation descriptor. Defaults to a static texture.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct McMetaAnim {
    pub(crate) frames: Vec<u32>,
    pub(crate) frametime_ms: u32,
    pub(crate) interpolate: bool,
}

impl Default for McMetaAnim {
    fn default() -> Self {
        Self {
            frames: vec![0],
            frametime_ms: 0,
            interpolate: false,
        }
    }
}

impl McMetaAnim {
    pub(crate) fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }
}

/// Read `<png>.mcmeta` next to the given PNG path. Returns
/// `Default` (static texture) when the sidecar is missing or
/// malformed. The `total_frames_in_png` hint is the number of
/// 16x16 tiles stacked vertically inside the PNG; when the JSON
/// omits the `frames` array we synthesize `[0, 1, ..., n-1]`.
pub(crate) fn load_for(png_path: &Path, total_frames_in_png: u32) -> McMetaAnim {
    let meta_path = {
        let mut p = png_path.to_path_buf().into_os_string();
        p.push(".mcmeta");
        std::path::PathBuf::from(p)
    };
    let text = match std::fs::read_to_string(&meta_path) {
        Ok(t) => t,
        Err(_) => return McMetaAnim::default(),
    };
    parse(&text, total_frames_in_png)
}

/// Parse `.mcmeta` JSON text. Exposed for tests; production code
/// uses `load_for`.
pub(crate) fn parse(text: &str, total_frames_in_png: u32) -> McMetaAnim {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return McMetaAnim::default();
    };
    let Some(anim) = v.get("animation") else {
        return McMetaAnim::default();
    };
    let frametime_ticks = anim
        .get("frametime")
        .and_then(|t| t.as_u64())
        .unwrap_or(1)
        .max(1);
    let frametime_ms = (frametime_ticks * 50) as u32;
    let interpolate = anim
        .get("interpolate")
        .and_then(|i| i.as_bool())
        .unwrap_or(false);
    // `frames` is optional. When omitted, MC walks 0..=total-1.
    let frames: Vec<u32> = match anim.get("frames").and_then(|f| f.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|f| {
                if let Some(n) = f.as_u64() {
                    Some(n as u32)
                } else if let Some(obj) = f.as_object() {
                    obj.get("index").and_then(|i| i.as_u64()).map(|n| n as u32)
                } else {
                    None
                }
            })
            .collect(),
        None => (0..total_frames_in_png.max(1)).collect(),
    };
    let frames = if frames.is_empty() {
        vec![0]
    } else {
        frames
    };
    McMetaAnim {
        frames,
        frametime_ms,
        interpolate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_static_default() {
        let m = parse("", 8);
        assert_eq!(m, McMetaAnim::default());
    }

    #[test]
    fn missing_animation_key_is_static() {
        let m = parse(r#"{"unrelated":1}"#, 8);
        assert_eq!(m, McMetaAnim::default());
    }

    #[test]
    fn parses_basic_frame_list() {
        let m = parse(
            r#"{"animation":{"frametime":2,"interpolate":true,"frames":[0,1,2,3]}}"#,
            4,
        );
        assert_eq!(m.frames, vec![0, 1, 2, 3]);
        assert_eq!(m.frametime_ms, 100);
        assert!(m.interpolate);
        assert!(m.is_animated());
    }

    #[test]
    fn frametime_ticks_convert_to_ms() {
        let m = parse(r#"{"animation":{"frametime":4}}"#, 1);
        assert_eq!(m.frametime_ms, 200);
    }

    #[test]
    fn missing_frames_uses_full_strip() {
        let m = parse(r#"{"animation":{"frametime":1}}"#, 5);
        assert_eq!(m.frames, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn frametime_floor_is_at_least_one_tick() {
        // Some packs ship `frametime: 0` by mistake; clamp to 1.
        let m = parse(r#"{"animation":{"frametime":0,"frames":[0,1]}}"#, 2);
        assert_eq!(m.frametime_ms, 50);
    }

    #[test]
    fn object_form_frames_extracts_index() {
        let m = parse(
            r#"{"animation":{"frametime":1,"frames":[{"index":3,"time":2},{"index":7}]}}"#,
            8,
        );
        assert_eq!(m.frames, vec![3, 7]);
    }
}
