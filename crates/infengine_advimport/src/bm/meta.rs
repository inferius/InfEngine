//! A8.1: the hotspot table and translations that ship beside a Black
//! Mirror install.
//!
//! The scene files say *where* a hotspot is; two XML files next to them
//! say what it is. `Scripts/HotSpots.xml` gives every hotspot in the
//! game a cursor and a type, and `loca/loca_<language>.xml` resolves the
//! label in its display name into readable text:
//!
//! ```xml
//! <HotSpot label="FromBeachToPromenade"
//!          display_name="#hotspots0_00080 Weg zur Promenade"
//!          cursor="exit" type="exit"/>
//! ```
//!
//! Two things fall out of that. `type="exit"` separates doorways from
//! ordinary scenery, which the scene graph alone cannot tell apart, and
//! the `From<A>To<B>` naming convention names the room on the other
//! side - so importing this table yields the map of how screens connect,
//! not just a prettier label.
//!
//! The readers here are deliberately not general XML parsers. These
//! files are machine-generated, flat, and free of namespaces, CDATA or
//! entities beyond the basic five, so a tag scanner covers them without
//! pulling in a dependency. Anything more structured would be a reason
//! to reach for a real parser instead.

use std::collections::HashMap;

use infengine_adventure::Verb;

/// What kind of thing a hotspot is, per the table's `type` attribute.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HotSpotKind {
    /// Scenery, an object, a person.
    #[default]
    Game,
    /// A way out of the room.
    Exit,
}

/// One row of `HotSpots.xml`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotSpotMeta {
    pub label: String,
    /// Translation key from the display name, e.g. `hotspots0_00080`.
    pub text_key: String,
    /// Text that followed the key, in the game's authoring language.
    pub fallback_text: String,
    pub verb: Verb,
    pub kind: HotSpotKind,
}

impl HotSpotMeta {
    /// Best available display name: the translation when there is one,
    /// otherwise the authoring-language text, otherwise the label.
    pub fn display_name(&self, loca: Option<&Localisation>) -> String {
        if let Some(text) = loca.and_then(|l| l.get(&self.text_key))
            && !text.is_empty()
        {
            return text.to_string();
        }
        if !self.fallback_text.is_empty() {
            return self.fallback_text.clone();
        }
        self.label.clone()
    }
}

/// Map the table's cursor names onto our verbs.
///
/// An exit cursor becomes [`Verb::Walk`]: the player walks through a
/// doorway, and the exit itself carries the room change.
fn verb_of(cursor: &str) -> Verb {
    match cursor {
        "use" => Verb::Use,
        "look_at" => Verb::Look,
        "pick_up" => Verb::Take,
        "talk_to" => Verb::Talk,
        _ => Verb::Walk,
    }
}

/// Every hotspot in the game, keyed by lowercased label.
#[derive(Debug, Clone, Default)]
pub struct HotSpotTable {
    rows: HashMap<String, HotSpotMeta>,
}

impl HotSpotTable {
    pub fn get(&self, label: &str) -> Option<&HotSpotMeta> {
        self.rows.get(&label.to_ascii_lowercase())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Read the attributes of one tag body into pairs.
fn attributes(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        // Name.
        while i < bytes.len() && !bytes[i].is_ascii_alphanumeric() && bytes[i] != '_' {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == '_') {
            i += 1;
        }
        if start == i {
            break;
        }
        let name: String = bytes[start..i].iter().collect();
        // Equals and opening quote.
        while i < bytes.len() && bytes[i].is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != '=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != '"' {
            continue;
        }
        i += 1;
        let value_start = i;
        while i < bytes.len() && bytes[i] != '"' {
            i += 1;
        }
        let value: String = bytes[value_start..i].iter().collect();
        i += 1;
        out.push((name, unescape(&value)));
    }
    out
}

/// Expand the five predefined XML entities. These files use no others.
fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Iterate the bodies of every `<name ...>` tag in `xml`.
fn tags<'a>(xml: &'a str, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    let open = format!("<{name}");
    std::iter::successors(Some(0usize), move |from| Some(*from)).scan(0usize, move |pos, _| {
        let rest = xml.get(*pos..)?;
        let at = rest.find(&open)? + *pos;
        let after = at + open.len();
        // The next character must end the tag name, or `<Text` would
        // also match `<TextRoot`.
        let boundary = xml[after..].chars().next()?;
        let end = xml[after..].find('>')? + after;
        *pos = end + 1;
        if boundary.is_ascii_alphanumeric() || boundary == '_' {
            return Some("");
        }
        Some(&xml[after..end])
    })
}

/// Split `#key rest of the name` into its parts.
fn split_display_name(value: &str) -> (String, String) {
    let Some(rest) = value.strip_prefix('#') else {
        return (String::new(), value.trim().to_string());
    };
    match rest.split_once(char::is_whitespace) {
        Some((key, text)) => (key.to_string(), text.trim().to_string()),
        None => (rest.to_string(), String::new()),
    }
}

/// Parse `HotSpots.xml`.
pub fn parse_hotspots(xml: &str) -> HotSpotTable {
    let mut rows = HashMap::new();
    for body in tags(xml, "HotSpot") {
        if body.is_empty() {
            continue;
        }
        let attrs = attributes(body);
        let find = |name: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
                .unwrap_or_default()
        };
        let label = find("label").to_string();
        if label.is_empty() {
            continue;
        }
        let (text_key, fallback_text) = split_display_name(find("display_name"));
        let kind = if find("type") == "exit" {
            HotSpotKind::Exit
        } else {
            HotSpotKind::Game
        };
        rows.insert(
            label.to_ascii_lowercase(),
            HotSpotMeta {
                text_key,
                fallback_text,
                verb: verb_of(find("cursor")),
                kind,
                label,
            },
        );
    }
    HotSpotTable { rows }
}

/// One translated line: what is said and who says it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocaLine {
    pub text: String,
    pub speaker: Option<String>,
}

/// Translations, keyed by label.
#[derive(Debug, Clone, Default)]
pub struct Localisation {
    entries: HashMap<String, LocaLine>,
}

impl Localisation {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(|l| l.text.as_str())
    }

    /// The full line, including its speaker.
    pub fn line(&self, key: &str) -> Option<&LocaLine> {
        self.entries.get(key)
    }

    /// Every line, for building our own locale file.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &LocaLine)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Parse `loca_<language>.xml`.
///
/// The file nests `<Text value="...">` inside `<Label value="...">`, so
/// the reader walks labels in order and takes the first text under each.
pub fn parse_localisation(xml: &str) -> Localisation {
    let mut entries = HashMap::new();
    let mut cursor = 0usize;
    while let Some(label_at) = xml[cursor..].find("<Label ") {
        let label_at = label_at + cursor;
        let Some(label_end) = xml[label_at..].find('>').map(|e| e + label_at) else {
            break;
        };
        let key = attributes(&xml[label_at + 7..label_end])
            .into_iter()
            .find(|(k, _)| k == "value")
            .map(|(_, v)| v);
        cursor = label_end + 1;

        // The text belonging to this label is the first <Text> before
        // the next <Label>, so a label with no text does not borrow the
        // next one's.
        let next_label = xml[cursor..]
            .find("<Label ")
            .map(|n| n + cursor)
            .unwrap_or(xml.len());
        let Some(text_at) = xml[cursor..next_label].find("<Text ").map(|t| t + cursor) else {
            continue;
        };
        let Some(text_end) = xml[text_at..].find('>').map(|e| e + text_at) else {
            continue;
        };
        let value = attributes(&xml[text_at + 6..text_end])
            .into_iter()
            .find(|(k, _)| k == "value")
            .map(|(_, v)| v);

        // The speaker sits in a <Speaker> tag nested under this text.
        let speaker = xml[text_end..next_label]
            .find("<Speaker ")
            .map(|s| s + text_end)
            .and_then(|at| {
                let end = xml[at..].find('>')? + at;
                attributes(&xml[at + 9..end])
                    .into_iter()
                    .find(|(k, _)| k == "value")
                    .map(|(_, v)| v)
            })
            .filter(|s| !s.is_empty());

        if let (Some(key), Some(text)) = (key, value) {
            entries.insert(key, LocaLine { text, speaker });
        }
    }
    Localisation { entries }
}

/// The room an exit leads to, read from the `From<A>To<B>` convention.
///
/// Returns the raw target name; matching it against the rooms that
/// actually exist is [`resolve_room`]'s job, because the convention is
/// not always exact.
pub fn exit_target(label: &str) -> Option<String> {
    let rest = label.strip_prefix("From")?;
    // Split on the last "To" so a source room whose name contains "To"
    // does not truncate the target.
    let at = rest.rfind("To")?;
    let target = &rest[at + 2..];
    if target.is_empty() {
        return None;
    }
    Some(target.to_string())
}

/// Match a target name against the rooms that were imported.
///
/// Exact match first, then a unique room whose id starts with the
/// target - `Promenade` legitimately means `PromenadeDay` when that is
/// the only candidate. An ambiguous prefix resolves to nothing rather
/// than picking one at random and wiring the map up wrong.
pub fn resolve_room(target: &str, known: &[String]) -> Option<String> {
    let lower = target.to_ascii_lowercase();
    if known.contains(&lower) {
        return Some(lower);
    }
    let mut matches = known.iter().filter(|r| r.starts_with(&lower));
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<HotSpots>
  <!-- a comment -->
  <HotSpot label="Sphere" display_name="#hotspots0_00065 Sphere" cursor="use" type="game"/>
  <HotSpot label="FromBeachToPromenade" display_name="#hotspots0_00080 Weg zur Promenade" cursor="exit" type="exit"/>
  <HotSpot label="Note" display_name="#hotspots0_00090 Zettel" cursor="pick_up" type="game" yoffset="12"/>
</HotSpots>"##;

    #[test]
    fn hotspot_rows_are_parsed() {
        let table = parse_hotspots(SAMPLE);
        assert_eq!(table.len(), 3);
        let sphere = table.get("Sphere").expect("row");
        assert_eq!(sphere.verb, Verb::Use);
        assert_eq!(sphere.kind, HotSpotKind::Game);
        assert_eq!(sphere.text_key, "hotspots0_00065");
        assert_eq!(sphere.fallback_text, "Sphere");
    }

    #[test]
    fn lookup_ignores_case() {
        let table = parse_hotspots(SAMPLE);
        assert!(table.get("sphere").is_some());
        assert!(table.get("SPHERE").is_some());
        assert!(table.get("nothing").is_none());
        assert!(!table.is_empty());
    }

    #[test]
    fn exits_are_distinguished_from_scenery() {
        let table = parse_hotspots(SAMPLE);
        assert_eq!(
            table.get("FromBeachToPromenade").expect("row").kind,
            HotSpotKind::Exit
        );
        assert_eq!(table.get("Note").expect("row").kind, HotSpotKind::Game);
    }

    #[test]
    fn every_cursor_maps_to_a_verb() {
        assert_eq!(verb_of("use"), Verb::Use);
        assert_eq!(verb_of("look_at"), Verb::Look);
        assert_eq!(verb_of("pick_up"), Verb::Take);
        assert_eq!(verb_of("talk_to"), Verb::Talk);
        assert_eq!(verb_of("exit"), Verb::Walk);
        assert_eq!(verb_of("nonsense"), Verb::Walk, "unknown falls back to walk");
    }

    #[test]
    fn extra_attributes_do_not_break_the_row() {
        let table = parse_hotspots(SAMPLE);
        let note = table.get("Note").expect("row");
        assert_eq!(note.verb, Verb::Take);
    }

    #[test]
    fn a_display_name_without_a_key_is_used_verbatim() {
        let (key, text) = split_display_name("Plain Name");
        assert!(key.is_empty());
        assert_eq!(text, "Plain Name");
        let (key, text) = split_display_name("#onlykey");
        assert_eq!(key, "onlykey");
        assert!(text.is_empty());
    }

    #[test]
    fn a_speaker_is_kept_with_its_line() {
        let xml = r#"<TextRoot><Label value="a"><Text value="Ahoj"><Speaker value="Darren"/></Text></Label>
          <Label value="b"><Text value="Vypravec"/></Label></TextRoot>"#;
        let loca = parse_localisation(xml);
        assert_eq!(loca.line("a").expect("line").speaker.as_deref(), Some("Darren"));
        assert_eq!(loca.line("b").expect("line").speaker, None, "narration has none");
        assert_eq!(loca.iter().count(), 2);
    }

    #[test]
    fn localisation_pairs_labels_with_their_text() {
        let xml = r#"<TextRoot><Language value="czech">
          <Label value="a0_001"><Text value="Angelino?"><Speaker value="Darren"/></Text></Label>
          <Label value="a0_002"><Text value="Co je?"><Speaker value="Angelina"/></Text></Label>
        </Language></TextRoot>"#;
        let loca = parse_localisation(xml);
        assert_eq!(loca.len(), 2);
        assert_eq!(loca.get("a0_001"), Some("Angelino?"));
        assert_eq!(loca.get("a0_002"), Some("Co je?"));
        assert_eq!(loca.get("missing"), None);
    }

    #[test]
    fn a_label_without_text_does_not_borrow_the_next_ones() {
        let xml = r#"<TextRoot>
          <Label value="empty"></Label>
          <Label value="full"><Text value="here"/></Label>
        </TextRoot>"#;
        let loca = parse_localisation(xml);
        assert_eq!(loca.get("empty"), None);
        assert_eq!(loca.get("full"), Some("here"));
    }

    #[test]
    fn entities_are_expanded() {
        let xml = r#"<TextRoot><Label value="k"><Text value="A &amp; B &quot;C&quot;"/></Label></TextRoot>"#;
        let loca = parse_localisation(xml);
        assert_eq!(loca.get("k"), Some(r#"A & B "C""#));
    }

    #[test]
    fn display_name_prefers_the_translation() {
        let table = parse_hotspots(SAMPLE);
        let note = table.get("Note").expect("row");
        assert_eq!(note.display_name(None), "Zettel", "no translation loaded");

        let loca = parse_localisation(
            r##"<TextRoot><Label value="hotspots0_00090"><Text value="lístek"/></Label></TextRoot>"##,
        );
        assert_eq!(note.display_name(Some(&loca)), "lístek");
    }

    #[test]
    fn display_name_falls_back_to_the_label_when_nothing_else_exists() {
        let meta = HotSpotMeta {
            label: "Bare".into(),
            ..Default::default()
        };
        assert_eq!(meta.display_name(None), "Bare");
    }

    #[test]
    fn exit_targets_come_out_of_the_naming_convention() {
        assert_eq!(
            exit_target("FromBeachToPromenade").as_deref(),
            Some("Promenade")
        );
        assert_eq!(
            exit_target("FromLighthouseToWillowCreek").as_deref(),
            Some("WillowCreek")
        );
    }

    #[test]
    fn a_source_name_containing_to_does_not_truncate_the_target() {
        // Splitting on the first "To" would give "msRealm" here.
        assert_eq!(
            exit_target("FromTomsRealmToBeach").as_deref(),
            Some("Beach")
        );
    }

    #[test]
    fn labels_that_are_not_exits_have_no_target() {
        assert_eq!(exit_target("Sphere"), None);
        assert_eq!(exit_target("FromNowhere"), None);
        assert_eq!(exit_target("FromXTo"), None);
    }

    #[test]
    fn a_target_resolves_to_an_exact_room_first() {
        let known = vec!["beachbright".to_string(), "beach".to_string()];
        assert_eq!(resolve_room("Beach", &known).as_deref(), Some("beach"));
    }

    #[test]
    fn a_unique_prefix_resolves() {
        let known = vec!["promenadeday".to_string(), "beach".to_string()];
        assert_eq!(
            resolve_room("Promenade", &known).as_deref(),
            Some("promenadeday")
        );
    }

    #[test]
    fn an_ambiguous_prefix_resolves_to_nothing() {
        // Wiring the map to whichever one sorted first would be worse
        // than leaving the exit unresolved.
        let known = vec!["beachbright".to_string(), "beachdark".to_string()];
        assert_eq!(resolve_room("Beach", &known), None);
    }

    #[test]
    fn an_unknown_target_resolves_to_nothing() {
        assert_eq!(resolve_room("Nowhere", &["beach".to_string()]), None);
        assert_eq!(resolve_room("Beach", &[]), None);
    }

    #[test]
    fn the_tag_scanner_does_not_match_a_longer_tag_name() {
        // <TextRoot> must not be read as <Text>.
        let xml = r#"<TextRoot><Label value="k"><Text value="v"/></Label></TextRoot>"#;
        assert_eq!(parse_localisation(xml).get("k"), Some("v"));
    }

    #[test]
    fn an_empty_or_malformed_file_yields_an_empty_table() {
        assert!(parse_hotspots("").is_empty());
        assert!(parse_hotspots("<HotSpots></HotSpots>").is_empty());
        assert!(parse_localisation("").is_empty());
        // A tag that never closes must not hang the scanner.
        assert!(parse_hotspots("<HotSpot label=\"x\"").is_empty());
    }
}
