//! A10.1: text, voice and languages.
//!
//! Every line the game can say - a subtitle, a hotspot name, a menu
//! label - is stored once per language under a stable id. Voice and lip
//! sync hang off the same id, so a translator who records new audio does
//! not have to touch anything else, and a language that has text but no
//! recordings simply leaves the voice field empty.
//!
//! ```text
//!   loca/
//!     en.loca.ron      one file per language
//!     cs.loca.ron      adding a language is dropping a file in here
//! ```
//!
//! That layout is the whole design goal: adding or correcting a
//! language must never mean editing the rooms, the scripts, or another
//! language's file. Nothing outside this module knows what languages
//! exist.
//!
//! Two rules make gaps visible rather than silent. A line missing from
//! one language falls back to the project's fallback language instead of
//! rendering blank - an English subtitle beats no subtitle. And a line
//! missing everywhere renders as its own id, so a typo in a key shows up
//! on screen as `hall_door_locked` rather than as nothing at all.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Conventional file suffix for a language file.
pub const LOCALE_SUFFIX: &str = ".loca.ron";
/// Conventional directory holding them.
pub const LOCALE_DIR: &str = "loca";

/// One translatable line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// What the player reads.
    pub text: String,
    /// Who says it, when the line is dialogue. Drives the portrait and
    /// the subtitle colour; `None` for narration and UI text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// Recorded audio, relative to the project root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// Lip sync data that goes with the recording.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lipsync: Option<String>,
}

impl Line {
    /// A plain line of text with no speaker or audio.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }

    pub fn with_speaker(mut self, speaker: impl Into<String>) -> Self {
        self.speaker = Some(speaker.into());
        self
    }

    pub fn with_voice(mut self, voice: impl Into<String>) -> Self {
        self.voice = Some(voice.into());
        self
    }

    /// Does this line have audio to play?
    pub fn is_voiced(&self) -> bool {
        self.voice.is_some()
    }
}

/// Every line in one language.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locale {
    /// Language tag, e.g. `en` or `cs`. Used as the file stem.
    pub language: String,
    /// Name shown in the language menu, written in that language.
    #[serde(default)]
    pub display_name: String,
    /// Lines keyed by id. Ordered so a diff between two revisions of a
    /// translation stays readable.
    #[serde(default)]
    pub lines: BTreeMap<String, Line>,
}

impl Locale {
    pub fn new(language: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            display_name: display_name.into(),
            lines: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, id: impl Into<String>, line: Line) {
        self.lines.insert(id.into(), line);
    }

    pub fn get(&self, id: &str) -> Option<&Line> {
        self.lines.get(id)
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// How many lines have audio.
    pub fn voiced_count(&self) -> usize {
        self.lines.values().filter(|l| l.is_voiced()).count()
    }

    /// File name this locale is stored under.
    pub fn file_name(&self) -> String {
        format!("{}{LOCALE_SUFFIX}", self.language)
    }
}

/// All the languages a project ships.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locales {
    /// Language used when a line is missing from the one in play.
    pub fallback: String,
    locales: BTreeMap<String, Locale>,
}

impl Locales {
    pub fn new(fallback: impl Into<String>) -> Self {
        Self {
            fallback: fallback.into(),
            locales: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, locale: Locale) {
        if self.fallback.is_empty() {
            self.fallback = locale.language.clone();
        }
        self.locales.insert(locale.language.clone(), locale);
    }

    pub fn get(&self, language: &str) -> Option<&Locale> {
        self.locales.get(language)
    }

    pub fn get_mut(&mut self, language: &str) -> Option<&mut Locale> {
        self.locales.get_mut(language)
    }

    /// Language tags present, sorted.
    pub fn languages(&self) -> Vec<&str> {
        self.locales.keys().map(String::as_str).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.locales.is_empty()
    }

    /// The line for `id` in `language`, falling back when it is missing.
    ///
    /// Falling back rather than returning `None` is deliberate: a
    /// half-translated game should show the original text, not a blank
    /// subtitle.
    pub fn line(&self, id: &str, language: &str) -> Option<&Line> {
        if let Some(found) = self.locales.get(language).and_then(|l| l.get(id)) {
            return Some(found);
        }
        self.locales.get(&self.fallback).and_then(|l| l.get(id))
    }

    /// Text for `id`, or the id itself when nothing matches.
    ///
    /// Rendering the id makes a missing or mistyped key obvious on
    /// screen instead of silently showing nothing.
    pub fn text<'a>(&'a self, id: &'a str, language: &str) -> &'a str {
        self.line(id, language).map_or(id, |l| l.text.as_str())
    }

    /// Ids present in the fallback language but missing from
    /// `language`, sorted - the translator's to-do list.
    pub fn missing(&self, language: &str) -> Vec<&str> {
        let Some(base) = self.locales.get(&self.fallback) else {
            return Vec::new();
        };
        let Some(target) = self.locales.get(language) else {
            return base.lines.keys().map(String::as_str).collect();
        };
        base.lines
            .keys()
            .filter(|id| !target.lines.contains_key(*id))
            .map(String::as_str)
            .collect()
    }

    /// Ids in `language` that the fallback does not have - usually a
    /// stale key left behind after the original text was renamed.
    pub fn orphaned(&self, language: &str) -> Vec<&str> {
        let (Some(base), Some(target)) = (
            self.locales.get(&self.fallback),
            self.locales.get(language),
        ) else {
            return Vec::new();
        };
        target
            .lines
            .keys()
            .filter(|id| !base.lines.contains_key(*id))
            .map(String::as_str)
            .collect()
    }

    /// Fraction of the fallback's lines that `language` covers, 0 to 1.
    pub fn coverage(&self, language: &str) -> f32 {
        let Some(base) = self.locales.get(&self.fallback) else {
            return 1.0;
        };
        if base.lines.is_empty() {
            return 1.0;
        }
        let missing = self.missing(language).len();
        1.0 - (missing as f32 / base.lines.len() as f32)
    }
}

/// Errors from reading or writing language files.
#[derive(Debug, thiserror::Error)]
pub enum LocaError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: ron::error::SpannedError,
    },
    #[error("failed to serialize locale {language}: {source}")]
    Serialize {
        language: String,
        #[source]
        source: ron::Error,
    },
}

fn pretty() -> ron::ser::PrettyConfig {
    ron::ser::PrettyConfig::new()
        .indentor("  ")
        .struct_names(true)
}

/// Write one language file into `dir`.
pub fn save_locale(locale: &Locale, dir: &Path) -> Result<PathBuf, LocaError> {
    let text =
        ron::ser::to_string_pretty(locale, pretty()).map_err(|source| LocaError::Serialize {
            language: locale.language.clone(),
            source,
        })?;
    fs::create_dir_all(dir).map_err(|source| LocaError::Write {
        path: dir.display().to_string(),
        source,
    })?;
    let path = dir.join(locale.file_name());
    fs::write(&path, text).map_err(|source| LocaError::Write {
        path: path.display().to_string(),
        source,
    })?;
    Ok(path)
}

/// Read one language file.
pub fn load_locale(path: &Path) -> Result<Locale, LocaError> {
    let text = fs::read_to_string(path).map_err(|source| LocaError::Read {
        path: path.display().to_string(),
        source,
    })?;
    ron::from_str(&text).map_err(|source| LocaError::Parse {
        path: path.display().to_string(),
        source,
    })
}

/// Load every language file in `dir`.
///
/// A directory that does not exist yields an empty set rather than an
/// error: a project without translations is a normal state, not a
/// broken one.
pub fn load_dir(dir: &Path, fallback: &str) -> Result<Locales, LocaError> {
    let mut locales = Locales::new(fallback);
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(locales);
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(LOCALE_SUFFIX))
        })
        .collect();
    paths.sort();
    for path in paths {
        locales.insert(load_locale(&path)?);
    }
    Ok(locales)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Locales {
        let mut english = Locale::new("en", "English");
        english.insert("greet", Line::text("Hello there.").with_speaker("Darren"));
        english.insert("door", Line::text("The door is locked."));
        english.insert("urn", Line::text("A cracked urn."));

        let mut czech = Locale::new("cs", "Čeština");
        czech.insert(
            "greet",
            Line::text("Dobrý den.")
                .with_speaker("Darren")
                .with_voice("voice/cs/greet.ogg"),
        );
        czech.insert("door", Line::text("Dveře jsou zamčené."));

        let mut all = Locales::new("en");
        all.insert(english);
        all.insert(czech);
        all
    }

    #[test]
    fn a_translated_line_comes_from_its_own_language() {
        let all = sample();
        assert_eq!(all.text("greet", "cs"), "Dobrý den.");
        assert_eq!(all.text("greet", "en"), "Hello there.");
    }

    #[test]
    fn a_missing_line_falls_back_rather_than_going_blank() {
        // "urn" is only in English. A half-translated game should show
        // the original, not an empty subtitle.
        let all = sample();
        assert_eq!(all.text("urn", "cs"), "A cracked urn.");
    }

    #[test]
    fn an_unknown_id_renders_as_itself() {
        // A typo must be visible on screen, not silently invisible.
        let all = sample();
        assert_eq!(all.text("hall_door_locked", "cs"), "hall_door_locked");
    }

    #[test]
    fn an_unknown_language_still_shows_the_fallback() {
        let all = sample();
        assert_eq!(all.text("door", "de"), "The door is locked.");
    }

    #[test]
    fn voice_hangs_off_the_same_id_as_the_text() {
        let all = sample();
        let line = all.line("greet", "cs").expect("line");
        assert!(line.is_voiced());
        assert_eq!(line.voice.as_deref(), Some("voice/cs/greet.ogg"));
        // English has the text but no recording, which is legal.
        assert!(!all.line("greet", "en").expect("line").is_voiced());
    }

    #[test]
    fn missing_lists_the_translators_work() {
        let all = sample();
        assert_eq!(all.missing("cs"), vec!["urn"]);
        assert!(all.missing("en").is_empty(), "the fallback lacks nothing");
    }

    #[test]
    fn a_language_that_does_not_exist_is_entirely_missing() {
        let all = sample();
        assert_eq!(all.missing("de").len(), 3);
        assert!((all.coverage("de") - 0.0).abs() < 1e-6);
    }

    #[test]
    fn coverage_reports_how_far_along_a_translation_is() {
        let all = sample();
        assert!((all.coverage("cs") - 2.0 / 3.0).abs() < 1e-3);
        assert!((all.coverage("en") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn orphaned_finds_keys_left_behind_by_a_rename() {
        let mut all = sample();
        all.get_mut("cs")
            .expect("cs")
            .insert("old_key", Line::text("zapomenutý"));
        assert_eq!(all.orphaned("cs"), vec!["old_key"]);
        assert!(all.orphaned("en").is_empty());
    }

    #[test]
    fn the_first_language_added_becomes_the_fallback() {
        let mut all = Locales::default();
        assert!(all.fallback.is_empty());
        all.insert(Locale::new("cs", "Čeština"));
        assert_eq!(all.fallback, "cs");
        all.insert(Locale::new("en", "English"));
        assert_eq!(all.fallback, "cs", "adding a second must not steal it");
    }

    #[test]
    fn languages_are_listed_in_a_stable_order() {
        let all = sample();
        assert_eq!(all.languages(), vec!["cs", "en"]);
        assert!(!all.is_empty());
    }

    #[test]
    fn voiced_count_reports_recording_progress() {
        let all = sample();
        assert_eq!(all.get("cs").expect("cs").voiced_count(), 1);
        assert_eq!(all.get("en").expect("en").voiced_count(), 0);
    }

    #[test]
    fn a_locale_round_trips_through_a_file() {
        let dir = std::env::temp_dir().join("infengine_loca_test");
        let _ = fs::remove_dir_all(&dir);

        let czech = sample().get("cs").expect("cs").clone();
        let path = save_locale(&czech, &dir).expect("save");
        assert!(path.ends_with("cs.loca.ron"));
        let back = load_locale(&path).expect("load");
        assert_eq!(czech, back);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn adding_a_language_is_dropping_a_file_in_the_directory() {
        // The point of the whole layout: nothing else changes.
        let dir = std::env::temp_dir().join("infengine_loca_dir_test");
        let _ = fs::remove_dir_all(&dir);

        let all = sample();
        for language in all.languages() {
            save_locale(all.get(language).expect("locale"), &dir).expect("save");
        }
        let loaded = load_dir(&dir, "en").expect("load");
        assert_eq!(loaded.languages(), vec!["cs", "en"]);

        // A new translator drops in one more file.
        let mut german = Locale::new("de", "Deutsch");
        german.insert("door", Line::text("Die Tür ist verschlossen."));
        save_locale(&german, &dir).expect("save");

        let loaded = load_dir(&dir, "en").expect("load");
        assert_eq!(loaded.languages(), vec!["cs", "de", "en"]);
        assert_eq!(loaded.text("door", "de"), "Die Tür ist verschlossen.");
        assert_eq!(loaded.text("urn", "de"), "A cracked urn.", "falls back");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_translations_loads_empty_instead_of_failing() {
        let missing = Path::new("definitely/not/here");
        let loaded = load_dir(missing, "en").expect("no error");
        assert!(loaded.is_empty());
    }

    #[test]
    fn a_broken_language_file_names_itself_in_the_error() {
        let dir = std::env::temp_dir().join("infengine_loca_bad_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join("xx.loca.ron"), "Locale(language: ").expect("write");

        let err = load_dir(&dir, "en").expect_err("should fail");
        assert!(err.to_string().contains("xx.loca.ron"));

        let _ = fs::remove_dir_all(&dir);
    }
}
