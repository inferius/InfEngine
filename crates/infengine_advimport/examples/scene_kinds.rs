//! What a Black Mirror scene file actually contains.
//!
//! A probe rather than a test: it needs the game installed, and what it
//! reports is a fact about the data, not about this code. Kept because
//! the next question about these files starts by running it.
//!
//! ```text
//! cargo run -p infengine_advimport --example scene_kinds -- <Scenes dir> [node kind]
//! ```
//!
//! With no node kind it counts every kind and how many carry an
//! outline. With one, it prints the first few of that kind with their
//! attributes, then tallies which attributes that kind uses.

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        println!("usage: scene_kinds <Scenes dir> [node kind]");
        return;
    };
    let want = args.next();

    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut outlined: BTreeMap<String, usize> = BTreeMap::new();
    let mut attrs: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    let mut scenes = 0;

    let Ok(entries) = std::fs::read_dir(&dir) else {
        println!("cannot read {dir}");
        return;
    };
    let mut paths: Vec<_> = entries
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
        let scene = path.file_stem().unwrap_or_default().to_string_lossy();

        for element in &parsed.elements {
            *kinds.entry(element.kind.clone()).or_default() += 1;
            if !element.points.is_empty() {
                *outlined.entry(element.kind.clone()).or_default() += 1;
            }
            if want.as_deref() != Some(element.kind.as_str()) {
                continue;
            }
            for (name, _) in &element.attrs {
                *attrs.entry(name.clone()).or_default() += 1;
            }
            if shown < 14 {
                let pairs: Vec<String> = element
                    .attrs
                    .iter()
                    .map(|(k, v)| format!("{k}={v:?}"))
                    .collect();
                println!(
                    "  {scene} | outline {} | {}",
                    element.points.len(),
                    pairs.join(" ")
                );
                shown += 1;
            }
        }
    }

    println!("{scenes} scenes");
    match want {
        Some(kind) => {
            println!("\nattributes of {kind}:");
            for (name, count) in &attrs {
                println!("  {name:24} x{count}");
            }
        }
        None => {
            for (kind, count) in &kinds {
                let pts = outlined.get(kind).copied().unwrap_or(0);
                println!("  {kind:24} x{count:6}  with an outline: {pts}");
            }
        }
    }
}
