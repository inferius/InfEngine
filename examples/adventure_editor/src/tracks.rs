//! A86: the scene's steps sorted onto tracks.
//!
//! The script stays what it is - a sequence with waits - and the
//! timeline stays a view of it. What changes is the reading: instead
//! of one row per step in script order, each step lands on the track
//! of the thing it drives. Camera moves on the camera's row, music on
//! the music's, each animation on its own. One glance answers "who
//! does what when", which a stack of rows in script order never could.
//!
//! Nothing here edits anything. A track is a list of indices into the
//! schedule's slots; the drawing and the dragging stay in
//! [`crate::timeline`], addressed by the same slot paths as before.

use infengine_adventure::action::Action;
use infengine_adventure::schedule::Timeline;

/// What a track is about, for its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Camera,
    Picture,
    Animation,
    Speech,
    Sound,
    Music,
    /// Everything that steers rather than shows: waits, flags,
    /// branches, room changes, the steps not translated yet.
    Flow,
}

impl TrackKind {
    /// The clip colour, muted so the selection can be the bright one.
    pub fn fill(self) -> egui::Color32 {
        match self {
            TrackKind::Camera => egui::Color32::from_rgb(70, 100, 130),
            TrackKind::Picture => egui::Color32::from_rgb(80, 110, 90),
            TrackKind::Animation => egui::Color32::from_rgb(105, 85, 130),
            TrackKind::Speech => egui::Color32::from_rgb(130, 110, 70),
            TrackKind::Sound => egui::Color32::from_rgb(70, 115, 115),
            TrackKind::Music => egui::Color32::from_rgb(95, 95, 140),
            TrackKind::Flow => egui::Color32::from_rgb(75, 80, 95),
        }
    }
}

/// One row of the timeline: a name, and the slots that belong to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub name: String,
    pub kind: TrackKind,
    /// Indices into the schedule's `slots`, in time order.
    pub slots: Vec<usize>,
}

/// Which track a step belongs on.
fn key(action: &Action) -> (TrackKind, String) {
    match action {
        Action::MoveCamera { .. } => (TrackKind::Camera, "Camera".into()),
        Action::ShowImage { .. } => (TrackKind::Picture, "Pictures".into()),
        Action::PlayAnim { animation, .. } => {
            (TrackKind::Animation, animation.clone())
        }
        Action::Say { speaker, .. } => (
            TrackKind::Speech,
            speaker.clone().unwrap_or_else(|| "Speech".into()),
        ),
        Action::PlaySound { .. } => (TrackKind::Sound, "Sounds".into()),
        Action::PlayMusic { .. } => (TrackKind::Music, "Music".into()),
        _ => (TrackKind::Flow, "Steps".into()),
    }
}

/// Sort a laid-out scene onto tracks.
///
/// Track order is what a film crew would write: the shot first, then
/// what is seen, then who moves and speaks, then what is heard, and
/// the steering last. Within a kind, tracks appear in the order the
/// scene first uses them. Only tracks with something on them exist.
pub fn assign(line: &Timeline, actions: &[Action]) -> Vec<Track> {
    let mut tracks: Vec<Track> = Vec::new();
    for (index, slot) in line.slots.iter().enumerate() {
        let (kind, name) = crate::timeline::action_at(actions, &slot.path)
            .map(key)
            // A slot whose action vanished mid-edit still needs a home.
            .unwrap_or((TrackKind::Flow, "Steps".into()));
        match tracks.iter_mut().find(|t| t.kind == kind && t.name == name) {
            Some(track) => track.slots.push(index),
            None => tracks.push(Track {
                name,
                kind,
                slots: vec![index],
            }),
        }
    }
    let order = |kind: TrackKind| match kind {
        TrackKind::Camera => 0,
        TrackKind::Picture => 1,
        TrackKind::Animation => 2,
        TrackKind::Speech => 3,
        TrackKind::Sound => 4,
        TrackKind::Music => 5,
        TrackKind::Flow => 6,
    };
    tracks.sort_by_key(|track| order(track.kind));
    tracks
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;
    use infengine_adventure::action::Script;
    use infengine_adventure::schedule::{self, Timings};

    fn laid_out(actions: Vec<Action>) -> (Timeline, Vec<Action>) {
        let mut script = Script::new("t.entry".to_string());
        script.actions = actions;
        let line = schedule::timeline(
            &script,
            &Timings {
                animation: &|_| Some(2.0),
                line: &|_| Some("hello there".into()),
            },
        );
        (line, script.actions)
    }

    #[test]
    fn steps_land_on_the_track_of_what_they_drive() {
        let (line, actions) = laid_out(vec![
            Action::PlayMusic { music: Some("m".into()) },
            Action::Wait { seconds: 1.0 },
            Action::PlayAnim {
                animation: "walk".into(),
                wait: true,
                at: None,
            },
            Action::MoveCamera {
                to: Vec2::ZERO,
                seconds: 1.0,
            },
        ]);
        let tracks = assign(&line, &actions);
        let names: Vec<&str> = tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["Camera", "walk", "Music", "Steps"]);
        // Each slot appears exactly once across all tracks.
        let total: usize = tracks.iter().map(|t| t.slots.len()).sum();
        assert_eq!(total, line.slots.len());
    }

    #[test]
    fn each_animation_gets_its_own_row() {
        let anim = |id: &str| Action::PlayAnim {
            animation: id.into(),
            wait: true,
            at: None,
        };
        let (line, actions) = laid_out(vec![anim("a"), anim("b"), anim("a")]);
        let tracks = assign(&line, &actions);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].name, "a");
        assert_eq!(tracks[0].slots.len(), 2, "both plays of a share its row");
        assert_eq!(tracks[1].name, "b");
    }

    #[test]
    fn branch_children_land_on_their_own_tracks() {
        use infengine_adventure::action::Condition;
        let (line, actions) = laid_out(vec![Action::Branch {
            on: Condition::Flag("f".into()),
            then: vec![Action::PlaySound { sound: "s".into() }],
            otherwise: Vec::new(),
        }]);
        let tracks = assign(&line, &actions);
        // The branch itself steers; the sound inside it is a sound.
        assert!(tracks.iter().any(|t| t.kind == TrackKind::Flow));
        assert!(tracks.iter().any(|t| t.kind == TrackKind::Sound));
    }

    #[test]
    fn speakers_split_the_speech_rows() {
        let say = |who: Option<&str>| Action::Say {
            line: "l".into(),
            speaker: who.map(str::to_string),
            wait: true,
            };
        let (line, actions) = laid_out(vec![say(Some("kallak")), say(Some("malcolm")), say(None)]);
        let tracks = assign(&line, &actions);
        let names: Vec<&str> = tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["kallak", "malcolm", "Speech"]);
    }
}
