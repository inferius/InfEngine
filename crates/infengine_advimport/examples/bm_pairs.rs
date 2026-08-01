//! How many world-to-screen correspondences a Black Mirror scene offers.
//!
//! An exit is a screen polygon called `From<This>To<X>`; the locator
//! called `TransitionFrom<X>` is where the player stands on arriving
//! from X. The two describe the same doorway, one in screen space and
//! one in world space, so each pair constrains the camera.

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: bm_pairs <Scenes dir>");
    let mut histogram: BTreeMap<usize, usize> = BTreeMap::new();
    let mut total = 0;
    let mut scenes = 0;

    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("dat"))
        .collect();
    paths.sort();

    for path in paths {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(parsed) = infengine_advimport::bm::scene::parse(&bytes) else {
            continue;
        };
        scenes += 1;

        let mut arrivals: BTreeMap<String, (f32, f32)> = BTreeMap::new();
        for locator in parsed.of_kind("Locator") {
            let label = locator.label();
            if let Some(room) = label.strip_prefix("TransitionFrom") {
                let x = locator.number("x").unwrap_or(0.0);
                let z = locator.number("z").unwrap_or(0.0);
                arrivals.insert(room.to_ascii_lowercase(), (x, z));
            }
        }

        let mut pairs = 0;
        for hotspot in parsed.of_kind("HotSpot") {
            let label = hotspot.label();
            if hotspot.points.is_empty() {
                continue;
            }
            let Some(rest) = label.strip_prefix("From") else { continue };
            let Some(at) = rest.rfind("To") else { continue };
            let target = rest[at + 2..].to_ascii_lowercase();
            if arrivals.contains_key(&target) {
                pairs += 1;
            }
        }
        *histogram.entry(pairs).or_default() += 1;
        total += pairs;
    }

    println!("{scenes} scenes, {total} pairs");
    for (pairs, scenes) in &histogram {
        println!("  {pairs} pairs: {scenes} scenes");
    }
}
