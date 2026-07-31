//! A3.6: what an import run produced, and what it could not.
//!
//! A run over a retail install touches hundreds of files, some of which
//! will not decode - a format variant we do not handle, a UI strip that
//! is not really an image, a truncated asset. Aborting on the first one
//! would make the tool useless, and swallowing them silently would make
//! it dishonest, so every skip is recorded with its reason and printed
//! at the end.

use std::fmt;

/// One thing the importer chose not to convert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Archive entry or file path.
    pub name: String,
    pub reason: String,
}

/// Totals for one import run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Archives opened successfully.
    pub archives: usize,
    /// Images written.
    pub images: usize,
    /// Room stubs written.
    pub rooms: usize,
    /// Rooms that arrived with a walkable floor traced from a mask.
    pub masks: usize,
    /// Images that are pictures rather than rooms: title cards, cutscene
    /// stills, item close-ups.
    pub sprites: usize,
    /// Animations written as sprite sheets.
    pub animations: usize,
    /// Frames across those animations.
    pub frames: usize,
    /// Languages written as locale files.
    pub languages: usize,
    /// Lines of text across them.
    pub lines: usize,
    /// Recorded lines extracted.
    pub voice_lines: usize,
    /// Bytes of audio written.
    pub voice_bytes: u64,
    /// Recordings written but not attached to a line, because their
    /// number falls outside the room's dialogue.
    pub unlinked_voice: usize,
    /// Music sequences written as MIDI.
    pub music: usize,
    /// Scripts decompiled into our own actions.
    pub scripts: usize,
    /// Steps inside them that are still untranslated.
    pub unknown_actions: usize,
    /// Scene links found in those scripts.
    pub scene_links: usize,
    /// Calls dropped as the original engine's rendering plumbing.
    pub plumbing_dropped: usize,
    pub skipped: Vec<Skipped>,
}

impl ImportReport {
    /// Did the run produce anything at all?
    pub fn is_empty(&self) -> bool {
        self.images == 0 && self.rooms == 0
    }

    /// Merge another run's totals into this one.
    pub fn absorb(&mut self, other: ImportReport) {
        self.archives += other.archives;
        self.images += other.images;
        self.rooms += other.rooms;
        self.masks += other.masks;
        self.sprites += other.sprites;
        self.animations += other.animations;
        self.frames += other.frames;
        self.languages += other.languages;
        self.lines += other.lines;
        self.voice_lines += other.voice_lines;
        self.voice_bytes += other.voice_bytes;
        self.unlinked_voice += other.unlinked_voice;
        self.music += other.music;
        self.scripts += other.scripts;
        self.unknown_actions += other.unknown_actions;
        self.scene_links += other.scene_links;
        self.plumbing_dropped += other.plumbing_dropped;
        self.skipped.extend(other.skipped);
    }

    /// Group skip reasons by their leading phrase, so a run that skipped
    /// 300 files shows five causes rather than 300 lines.
    pub fn skip_summary(&self) -> Vec<(String, usize)> {
        let mut groups: Vec<(String, usize)> = Vec::new();
        for skip in &self.skipped {
            // Cut at the first colon or open paren: the tail is usually
            // a file-specific number.
            let key = skip
                .reason
                .split(['(', ':'])
                .next()
                .unwrap_or(&skip.reason)
                .trim()
                .to_string();
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, n)) => *n += 1,
                None => groups.push((key, 1)),
            }
        }
        groups.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        groups
    }
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} archives, {} images, {} rooms ({} with a traced floor)",
            self.archives, self.images, self.rooms, self.masks
        )?;
        if self.languages > 0 {
            writeln!(
                f,
                "{} languages, {} lines, {} voiced ({:.0} MB of audio)",
                self.languages,
                self.lines,
                self.voice_lines,
                self.voice_bytes as f64 / (1024.0 * 1024.0)
            )?;
            if self.unlinked_voice > 0 {
                writeln!(
                    f,
                    "{} recordings written but not linked to a line",
                    self.unlinked_voice
                )?;
            }
        }
        if self.music > 0 {
            writeln!(f, "{} music sequences", self.music)?;
        }
        if self.scripts > 0 {
            writeln!(
                f,
                "{} scripts, {} scene links, {} steps still untranslated, {} dropped as engine plumbing",
                self.scripts, self.scene_links, self.unknown_actions, self.plumbing_dropped
            )?;
        }
        if self.sprites > 0 || self.animations > 0 {
            writeln!(
                f,
                "{} pictures that are not rooms, {} animations ({} frames)",
                self.sprites, self.animations, self.frames
            )?;
        }
        if self.skipped.is_empty() {
            return write!(f, "nothing skipped");
        }
        writeln!(f, "{} skipped:", self.skipped.len())?;
        for (reason, count) in self.skip_summary() {
            writeln!(f, "  {count:>5}  {reason}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skip(name: &str, reason: &str) -> Skipped {
        Skipped {
            name: name.into(),
            reason: reason.into(),
        }
    }

    #[test]
    fn empty_report_reports_empty() {
        assert!(ImportReport::default().is_empty());
    }

    #[test]
    fn a_report_with_images_is_not_empty() {
        let report = ImportReport {
            images: 1,
            ..Default::default()
        };
        assert!(!report.is_empty());
    }

    #[test]
    fn absorb_sums_totals_and_concatenates_skips() {
        let mut a = ImportReport {
            archives: 1,
            images: 2,
            rooms: 2,
            masks: 1,
            skipped: vec![skip("x", "bad")],
            ..Default::default()
        };
        a.absorb(ImportReport {
            archives: 3,
            images: 4,
            rooms: 4,
            masks: 3,
            skipped: vec![skip("y", "worse")],
            ..Default::default()
        });
        assert_eq!(a.archives, 4);
        assert_eq!(a.images, 6);
        assert_eq!(a.rooms, 6);
        assert_eq!(a.masks, 4);
        assert_eq!(a.skipped.len(), 2);
    }

    #[test]
    fn skip_summary_groups_by_cause_and_sorts_by_count() {
        let report = ImportReport {
            skipped: vec![
                skip("a", "not a full screen (12 pixels)"),
                skip("b", "not a full screen (48 pixels)"),
                skip("c", "not a full screen (7 pixels)"),
                skip("d", "CPS uses LZW-14, which is not supported"),
            ],
            ..Default::default()
        };
        let summary = report.skip_summary();
        assert_eq!(summary[0], ("not a full screen".to_string(), 3));
        assert_eq!(summary[1].1, 1);
    }

    #[test]
    fn display_lists_causes_not_every_file() {
        let report = ImportReport {
            archives: 2,
            images: 5,
            rooms: 5,
            masks: 5,
            skipped: vec![
                skip("a", "not a full screen (12 pixels)"),
                skip("b", "not a full screen (48 pixels)"),
            ],
            ..Default::default()
        };
        let text = report.to_string();
        assert!(text.contains("2 archives, 5 images, 5 rooms (5 with a traced floor)"));
        assert!(text.contains("2 skipped"));
        assert!(text.contains("not a full screen"));
        // The individual entry names are not spelled out.
        assert!(!text.contains("48 pixels"));
    }

    #[test]
    fn display_says_so_when_nothing_was_skipped() {
        let report = ImportReport {
            archives: 1,
            images: 1,
            rooms: 1,
            masks: 1,
            skipped: Vec::new(),
            ..Default::default()
        };
        assert!(report.to_string().contains("nothing skipped"));
    }
}
