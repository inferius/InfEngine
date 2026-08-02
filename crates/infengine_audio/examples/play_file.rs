//! Play one audio file and wait for it, to tell a silent engine from a
//! silent audio system.
//!
//! ```text
//! cargo run -p infengine_audio --example play_file -- <file> [seconds]
//! ```

#![allow(clippy::print_stdout)]

use infengine_audio::AudioSystem;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        println!("usage: play_file <file> [seconds]");
        return;
    };
    let seconds: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5.0);

    let mut audio = match AudioSystem::new() {
        Ok(audio) => audio,
        Err(err) => {
            println!("no audio device: {err}");
            return;
        }
    };
    let sound = match audio.load_sound(std::path::Path::new(&path)) {
        Ok(sound) => sound,
        Err(err) => {
            println!("could not load {path}: {err}");
            return;
        }
    };
    if let Err(err) = audio.play_sound(&sound) {
        println!("could not play: {err}");
        return;
    }
    println!("playing {path} for {seconds}s");
    std::thread::sleep(std::time::Duration::from_secs_f32(seconds));
    println!("done");
}
