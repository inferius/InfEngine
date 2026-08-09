//! A120: arrive on the side you came from.
//!
//! The last twelve bytes of every room's `.DAT` are its four arrival
//! points - north, east, south, west - each a word x (low bits masked)
//! and a byte y. The original places the player on the point opposite
//! the exit they walked out of; our imported exits carried no target
//! entry at all, so every arrival used the room's default and walking
//! out left could land you on the right.
//!
//! This writes the four points into each room as entries named by
//! direction, and points every directional exit at the opposite entry
//! of its target room.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_enters -- <dir of .PAK> <project>
//! ```

#![allow(clippy::print_stdout)]

use glam::Vec2;
use infengine_adventure::interact::{EntryPoint, Facing};
use infengine_advimport::kyra::pak::Pak;

/// Exit direction to the entry the player appears on, and the way they
/// then face: walking out the west side arrives on the east point of
/// the next room, facing further west.
const OPPOSITE: &[(&str, &str, Facing)] = &[
    ("north", "south", Facing::Back),
    ("south", "north", Facing::Front),
    ("east", "west", Facing::Right),
    ("west", "east", Facing::Left),
];

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(project)) = (args.next(), args.next()) else {
        println!("usage: kyra_enters <dir of .PAK files> <project>");
        return;
    };
    let rooms_dir = std::path::PathBuf::from(&project).join("rooms");

    let mut paks: Vec<_> = std::fs::read_dir(&game)
        .expect("readable game dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("pak"))
        })
        .collect();
    paks.sort();

    let (mut roomed, mut wired) = (0usize, 0usize);
    for path in &paks {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(pak) = Pak::parse(&bytes) else { continue };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let dat = pak
            .entries()
            .iter()
            .position(|e| e.name.eq_ignore_ascii_case(&format!("{stem}.dat")))
            .and_then(|index| pak.data_at(index));
        let Some(dat) = dat else { continue };
        if dat.len() < 12 {
            continue;
        }
        let tail = &dat[dat.len() - 12..];
        let point = |i: usize| {
            let x = (u16::from_le_bytes([tail[i * 3], tail[i * 3 + 1]]) & 0xFFFC) as f32;
            let y = (tail[i * 3 + 2] & 0xFE) as f32;
            Vec2::new(x, y)
        };
        let coords = [
            ("north", point(0)),
            ("east", point(1)),
            ("south", point(2)),
            ("west", point(3)),
        ];

        let room_path = rooms_dir.join(format!("{stem}.room.ron"));
        let Ok(mut room) = infengine_adventure::io::load(&room_path) else {
            continue;
        };
        for (id, at) in coords {
            // The original masks unused points to the screen edge zero;
            // a corner point is "no arrival from this side".
            if at == Vec2::ZERO {
                continue;
            }
            let facing = OPPOSITE
                .iter()
                .find(|(exit, ..)| *exit == id)
                .map(|(_, _, facing)| *facing)
                .unwrap_or(Facing::Front);
            match room.entries.iter_mut().find(|e| e.id == id) {
                Some(entry) => {
                    entry.position = at;
                    entry.facing = facing;
                }
                None => room.entries.push(EntryPoint {
                    id: id.into(),
                    position: at,
                    facing,
                }),
            }
        }
        for exit in &mut room.exits {
            if let Some((_, entry, _)) = OPPOSITE.iter().find(|(dir, ..)| *dir == exit.id) {
                exit.target_entry = (*entry).into();
                wired += 1;
            }
        }
        if infengine_adventure::io::save(&room, &room_path).is_ok() {
            roomed += 1;
        }
    }
    println!("{roomed} rooms given their four arrival points, {wired} exits wired to the opposite side");
}
