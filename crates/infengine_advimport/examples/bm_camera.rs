//! Does Black Mirror use one camera orientation for every scene?
//!
//! If it does, the world-to-screen map is `screen = A * world + t` with
//! A shared and only t per room, and the whole game's cameras can be
//! solved from the doorway correspondences at once. If it does not,
//! each room needs its own fit and most rooms do not offer enough pairs
//! to make one.
//!
//! This fits an affine map per scene that has at least three pairs and
//! prints the linear parts, so they can be compared by eye.

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;

/// Least squares for `screen = a*x + b*z + c` over the pairs.
fn fit_row(pairs: &[(f64, f64, f64)]) -> Option<[f64; 3]> {
    // Normal equations for three unknowns.
    let mut ata = [[0.0f64; 3]; 3];
    let mut atb = [0.0f64; 3];
    for (x, z, s) in pairs {
        let row = [*x, *z, 1.0];
        for i in 0..3 {
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * s;
        }
    }
    // Gaussian elimination with partial pivoting.
    for col in 0..3 {
        let pivot = (col..3)
            .max_by(|a, b| ata[*a][col].abs().partial_cmp(&ata[*b][col].abs()).unwrap())?;
        if ata[pivot][col].abs() < 1e-9 {
            return None;
        }
        ata.swap(col, pivot);
        atb.swap(col, pivot);
        for row in 0..3 {
            if row == col {
                continue;
            }
            let factor = ata[row][col] / ata[col][col];
            let pivot_row = ata[col];
            for (target, source) in ata[row].iter_mut().zip(pivot_row) {
                *target -= factor * source;
            }
            atb[row] -= factor * atb[col];
        }
    }
    Some([atb[0] / ata[0][0], atb[1] / ata[1][1], atb[2] / ata[2][2]])
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: bm_camera <Scenes dir>");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("dat"))
        .collect();
    paths.sort();

    println!("{:<34} {:>9} {:>9} {:>9} {:>9}  px err", "scene", "dsx/dx", "dsx/dz", "dsy/dx", "dsy/dz");
    for path in paths {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(parsed) = infengine_advimport::bm::scene::parse(&bytes) else {
            continue;
        };
        let mut arrivals: BTreeMap<String, (f64, f64)> = BTreeMap::new();
        for locator in parsed.of_kind("Locator") {
            if let Some(room) = locator.label().strip_prefix("TransitionFrom") {
                arrivals.insert(
                    room.to_ascii_lowercase(),
                    (
                        locator.number("x").unwrap_or(0.0) as f64,
                        locator.number("z").unwrap_or(0.0) as f64,
                    ),
                );
            }
        }

        let mut sx = Vec::new();
        let mut sy = Vec::new();
        for hotspot in parsed.of_kind("HotSpot") {
            if hotspot.points.is_empty() {
                continue;
            }
            let label = hotspot.label();
            let Some(rest) = label.strip_prefix("From") else { continue };
            let Some(at) = rest.rfind("To") else { continue };
            let Some((wx, wz)) = arrivals.get(&rest[at + 2..].to_ascii_lowercase()) else {
                continue;
            };
            // Feet at the doorway: the middle of the shape across, its
            // lowest point down.
            let mid = hotspot.points.iter().map(|p| p.x).sum::<f32>()
                / hotspot.points.len() as f32;
            let low = hotspot.points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
            sx.push((*wx, *wz, f64::from(mid)));
            sy.push((*wx, *wz, f64::from(low)));
        }
        if sx.len() < 3 {
            continue;
        }
        let (Some(rx), Some(ry)) = (fit_row(&sx), fit_row(&sy)) else {
            continue;
        };
        let err = sx
            .iter()
            .zip(&sy)
            .map(|((x, z, s), (_, _, t))| {
                let px = rx[0] * x + rx[1] * z + rx[2] - s;
                let py = ry[0] * x + ry[1] * z + ry[2] - t;
                (px * px + py * py).sqrt()
            })
            .sum::<f64>()
            / sx.len() as f64;
        println!(
            "{:<34} {:>9.4} {:>9.4} {:>9.4} {:>9.4}  {:>6.1} ({} pairs)",
            path.file_stem().unwrap_or_default().to_string_lossy(),
            rx[0], rx[1], ry[0], ry[1], err, sx.len()
        );
    }
}
