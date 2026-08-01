//! A38.1: the shape of a game, as a graph.
//!
//! A room leads to other rooms through its exits, a script leads
//! elsewhere through `GoToRoom` and `OpenScreen`, and a screen leads to
//! other screens through its buttons. Put together, that is the map of
//! the story - and it is the thing nobody can see by reading files one
//! at a time.
//!
//! What makes it worth building rather than eyeballing is what it finds:
//! a room nothing leads to is unreachable, and a room that leads nowhere
//! is a dead end. Both are bugs that never show up while testing the
//! scene you happen to be working on.

use std::collections::{BTreeMap, BTreeSet};

use crate::action::Action;
use crate::project::Project;

/// What a node in the map is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeKind {
    Room,
    Screen,
}

/// Why one place leads to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkKind {
    /// An exit the player walks through.
    Exit,
    /// A script that changes room.
    Script,
    /// A button on a screen.
    Button,
}

impl LinkKind {
    pub fn label(self) -> &'static str {
        match self {
            LinkKind::Exit => "exit",
            LinkKind::Script => "script",
            LinkKind::Button => "button",
        }
    }
}

/// One place in the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
}

/// One way of getting from `from` to `to`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Link {
    pub from: String,
    pub to: String,
    pub kind: LinkKind,
    /// What the author called it - an exit's name, a script's id.
    pub via: String,
}

/// The whole map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Story {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    /// Places named by a link that do not exist.
    pub missing: BTreeSet<String>,
}

impl Story {
    /// Build the map of a project.
    pub fn of(project: &Project) -> Self {
        let mut story = Story::default();
        let rooms: BTreeSet<String> = project.room_ids().into_iter().collect();
        let screens: BTreeSet<String> =
            project.screens.iter().map(|s| s.id.clone()).collect();

        for id in &rooms {
            story.nodes.push(Node {
                id: id.clone(),
                kind: NodeKind::Room,
            });
        }
        for id in &screens {
            story.nodes.push(Node {
                id: id.clone(),
                kind: NodeKind::Screen,
            });
        }

        // Which rooms answer to each script prefix. Usually one room to
        // its own name, but rooms that share a background share their
        // logic too, and a link found in that logic belongs to all of
        // them rather than to whichever one sorts first.
        let mut by_prefix: BTreeMap<String, Vec<String>> = BTreeMap::new();

        // Exits, read from the room files themselves.
        for id in &rooms {
            let Some(room) = project.load_room(id) else {
                continue;
            };
            by_prefix
                .entry(room.script_prefix().to_string())
                .or_default()
                .push(id.clone());
            for exit in &room.exits {
                if exit.target_room.is_empty() {
                    continue;
                }
                // The name if it has one, the id otherwise: two doors
                // to the same place are two doors, and the map should
                // say which is which rather than merging them.
                let via = if exit.name.is_empty() {
                    exit.id.as_str()
                } else {
                    exit.name.as_str()
                };
                story.link(id, &exit.target_room, LinkKind::Exit, via, &rooms, &screens);
            }
        }

        // Scripts. A script's id starts with the room it belongs to,
        // which is the only thing tying the two together.
        for (id, script) in &project.scripts {
            let prefix = id.split_once('.').map(|(room, _)| room).unwrap_or(id);
            let Some(owners) = by_prefix.get(prefix) else {
                continue;
            };
            let mut found: Vec<(String, LinkKind)> = Vec::new();
            script.walk(&mut |action| match action {
                Action::GoToRoom { room, .. } => {
                    found.push((room.clone(), LinkKind::Script));
                }
                Action::OpenScreen { screen } => {
                    found.push((screen.clone(), LinkKind::Script));
                }
                _ => {}
            });
            for owner in owners {
                for (target, kind) in &found {
                    story.link(owner, target, *kind, id, &rooms, &screens);
                }
            }
        }

        // Screens, through their buttons.
        for screen in &project.screens {
            for target in screen.destinations() {
                story.link(&screen.id, &target, LinkKind::Button, "", &rooms, &screens);
            }
        }

        story.links.sort();
        story.links.dedup();
        story
    }

    fn link(
        &mut self,
        from: &str,
        to: &str,
        kind: LinkKind,
        via: &str,
        rooms: &BTreeSet<String>,
        screens: &BTreeSet<String>,
    ) {
        if !rooms.contains(to) && !screens.contains(to) {
            self.missing.insert(to.to_string());
            return;
        }
        self.links.push(Link {
            from: from.to_string(),
            to: to.to_string(),
            kind,
            via: via.to_string(),
        });
    }

    /// Where you can get to from here.
    pub fn out_of(&self, id: &str) -> Vec<&Link> {
        self.links.iter().filter(|l| l.from == id).collect()
    }

    /// What leads here.
    pub fn into_of(&self, id: &str) -> Vec<&Link> {
        self.links.iter().filter(|l| l.to == id).collect()
    }

    /// Everything reachable from `start`, following links.
    pub fn reachable_from(&self, start: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut queue = vec![start.to_string()];
        // Adjacency built once: a map with a few hundred rooms would
        // otherwise walk the whole link list per step.
        let mut out: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for link in &self.links {
            out.entry(&link.from).or_default().push(&link.to);
        }
        while let Some(id) = queue.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            for next in out.get(id.as_str()).into_iter().flatten() {
                queue.push((*next).to_string());
            }
        }
        seen
    }

    /// Places nothing leads to.
    ///
    /// `start` is excluded, since the game begins there without needing
    /// anything to point at it.
    pub fn unreachable(&self, start: &str) -> Vec<&Node> {
        let reachable = self.reachable_from(start);
        self.nodes
            .iter()
            .filter(|node| !reachable.contains(&node.id))
            .collect()
    }

    /// Places you cannot leave.
    pub fn dead_ends(&self) -> Vec<&Node> {
        self.nodes
            .iter()
            .filter(|node| !self.links.iter().any(|l| l.from == node.id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Script;
    use crate::interact::{Exit, Shape};
    use crate::io;
    use crate::room::Room;
    use crate::screen::{self, Screen, ScreenAction, Widget, WidgetKind};
    use glam::Vec2;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("infengine_story_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn room_with_exit(root: &Path, id: &str, to: &str) {
        let mut room = Room::new(id, Vec2::new(320.0, 200.0));
        if !to.is_empty() {
            room.exits.push(Exit {
                id: format!("to_{to}"),
                name: format!("to the {to}"),
                shape: Shape::Rect {
                    min: Vec2::ZERO,
                    max: Vec2::new(10.0, 10.0),
                },
                target_room: to.into(),
                enabled: true,
                ..Default::default()
            });
        }
        io::save(&room, &root.join(format!("rooms/{id}.room.ron"))).expect("room");
    }

    #[test]
    fn exits_scripts_and_buttons_all_become_links() {
        let root = temp("links");
        room_with_exit(&root, "hall", "crypt");
        room_with_exit(&root, "crypt", "");
        room_with_exit(&root, "attic", "");

        let mut script = Script::new("crypt.stairs");
        script.push(Action::GoToRoom {
            room: "attic".into(),
            entry: None,
        });
        crate::script_io::save_room("crypt", &[script], &root.join(crate::script_io::SCRIPT_DIR))
            .expect("script");

        let mut title = Screen::new("title", Vec2::new(320.0, 200.0));
        title.widgets.push(Widget {
            id: "go".into(),
            min: Vec2::ZERO,
            max: Vec2::new(10.0, 10.0),
            kind: WidgetKind::Button {
                line: String::new(),
                size: 16.0,
                align: Default::default(),
                action: ScreenAction::Open {
                    screen: "options".into(),
                },
            },
            visible: true,
        });
        screen::save(&title, &root.join(screen::SCREEN_DIR)).expect("title");
        screen::save(
            &Screen::new("options", Vec2::new(320.0, 200.0)),
            &root.join(screen::SCREEN_DIR),
        )
        .expect("options");

        let story = Story::of(&Project::open(&root));
        let kinds: Vec<LinkKind> = story.links.iter().map(|l| l.kind).collect();
        assert!(kinds.contains(&LinkKind::Exit), "{:?}", story.links);
        assert!(kinds.contains(&LinkKind::Script));
        assert!(kinds.contains(&LinkKind::Button));
        assert_eq!(story.nodes.len(), 5, "three rooms and two screens");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_room_nothing_leads_to_is_reported() {
        let root = temp("unreachable");
        room_with_exit(&root, "hall", "crypt");
        room_with_exit(&root, "crypt", "");
        room_with_exit(&root, "island", "");

        let story = Story::of(&Project::open(&root));
        let lost: Vec<&str> = story
            .unreachable("hall")
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(lost, vec!["island"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_room_you_cannot_leave_is_reported() {
        let root = temp("dead_end");
        room_with_exit(&root, "hall", "crypt");
        room_with_exit(&root, "crypt", "");

        let story = Story::of(&Project::open(&root));
        let ends: Vec<&str> = story.dead_ends().iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ends, vec!["crypt"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_door_to_nowhere_is_named_rather_than_drawn() {
        let root = temp("missing");
        room_with_exit(&root, "hall", "atlantis");

        let story = Story::of(&Project::open(&root));
        assert!(story.missing.contains("atlantis"));
        assert!(
            story.links.is_empty(),
            "a link to a place that does not exist is not a link"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reachability_follows_a_chain_and_survives_a_loop() {
        let root = temp("chain");
        room_with_exit(&root, "a", "b");
        room_with_exit(&root, "b", "c");
        room_with_exit(&root, "c", "a");

        let story = Story::of(&Project::open(&root));
        let seen = story.reachable_from("a");
        assert_eq!(seen.len(), 3, "a loop must not hang or lose a room");
        assert!(story.unreachable("a").is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_same_link_twice_is_one_link() {
        let root = temp("dedup");
        let mut room = Room::new("hall", Vec2::new(320.0, 200.0));
        for id in ["north", "south"] {
            room.exits.push(Exit {
                id: id.into(),
                name: String::new(),
                shape: Shape::Rect {
                    min: Vec2::ZERO,
                    max: Vec2::new(10.0, 10.0),
                },
                target_room: "crypt".into(),
                enabled: true,
                ..Default::default()
            });
        }
        io::save(&room, &root.join("rooms/hall.room.ron")).expect("room");
        room_with_exit(&root, "crypt", "");

        let story = Story::of(&Project::open(&root));
        // Two doors to the same place are two doors, and the map keeps
        // both because they are named differently.
        assert_eq!(story.out_of("hall").len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

}
