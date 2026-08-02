//! A51.1: language tags, written the way the rest of the world writes
//! them.
//!
//! A locale is identified by a BCP 47 tag - `en`, `cs`, `pt-BR`,
//! `zh-Hant` - and the format is worth keeping to rather than inventing
//! a scheme of our own. It is what a browser sends, what an operating
//! system reports, and what every translation tool round-trips, so a
//! project that uses it can hand its text to any of them without a
//! translation table in between.
//!
//! The part that trips people up is case. `EN-us`, `en-US` and `En-Us`
//! are the same tag, and the standard says so - case carries no meaning
//! and is only a convention. But a `BTreeMap` keyed by the string does
//! not know that, so two spellings of one language become two languages,
//! each holding half the lines. Everything is canonicalised on the way
//! in for that reason and no other.
//!
//! The conventional spelling is: language lower case, script with an
//! initial capital, region upper case, and anything after that lower
//! case.

/// A tag written the conventional way.
///
/// Not a validator. A tag this does not recognise the shape of comes
/// back with its case fixed and nothing else changed, because refusing
/// a language nobody anticipated would be worse than carrying it.
pub fn canonical(tag: &str) -> String {
    let trimmed = tag.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Underscores are what a file name uses and what several older
    // tools emit; the standard wants hyphens.
    let mut out = String::with_capacity(trimmed.len());
    for (index, part) in trimmed.split(['-', '_']).enumerate() {
        if part.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('-');
        }
        match (index, part.len()) {
            // The language itself.
            (0, _) => out.push_str(&part.to_ascii_lowercase()),
            // A script is four letters: Hant, Cyrl, Latn.
            (_, 4) if part.chars().all(|c| c.is_ascii_alphabetic()) => {
                let mut chars = part.chars();
                if let Some(first) = chars.next() {
                    out.push(first.to_ascii_uppercase());
                    out.extend(chars.map(|c| c.to_ascii_lowercase()));
                }
            }
            // A region is two letters or three digits: US, CZ, 419.
            (_, 2 | 3) if part.chars().all(|c| c.is_ascii_alphanumeric()) => {
                out.push_str(&part.to_ascii_uppercase());
            }
            _ => out.push_str(&part.to_ascii_lowercase()),
        }
    }
    out
}

/// Do two tags name the same language, however they are spelled?
pub fn same(a: &str, b: &str) -> bool {
    canonical(a) == canonical(b)
}

/// The language part of a tag: `pt-BR` is Portuguese.
pub fn language(tag: &str) -> String {
    canonical(tag)
        .split('-')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// What a language calls itself, for the languages this engine's
/// imports have produced so far.
///
/// Endonyms rather than English names: a language menu is read by
/// someone who wants their own language, and "Deutsch" is what they are
/// looking for, not "German". A tag not in the table gets the tag back,
/// which is honest and is what the menu showed before this existed.
const ENDONYMS: &[(&str, &str)] = &[
    ("cs", "Čeština"),
    ("da", "Dansk"),
    ("de", "Deutsch"),
    ("el", "Ελληνικά"),
    ("en", "English"),
    ("es", "Español"),
    ("fi", "Suomi"),
    ("fr", "Français"),
    ("hu", "Magyar"),
    ("it", "Italiano"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("nl", "Nederlands"),
    ("no", "Norsk"),
    ("pl", "Polski"),
    ("pt", "Português"),
    ("ru", "Русский"),
    ("sk", "Slovenčina"),
    ("sv", "Svenska"),
    ("tr", "Türkçe"),
    ("zh", "中文"),
];

/// The name to show in a language menu.
///
/// A region is kept when there is one, because Brazilian and European
/// Portuguese are two translations and a menu that calls both
/// "Português" makes the choice meaningless.
pub fn display_name(tag: &str) -> String {
    let tag = canonical(tag);
    if tag.is_empty() {
        return String::new();
    }
    let language = language(&tag);
    let Some((_, name)) = ENDONYMS.iter().find(|(code, _)| *code == language) else {
        return tag;
    };
    match tag.split_once('-') {
        Some((_, rest)) => format!("{name} ({rest})"),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_is_fixed_rather_than_trusted() {
        // Two spellings of one tag become two languages in any map
        // keyed by the string, each holding half the lines.
        assert_eq!(canonical("EN-us"), "en-US");
        assert_eq!(canonical("en_us"), "en-US");
        assert_eq!(canonical("ZH-hant-cn"), "zh-Hant-CN");
        assert_eq!(canonical("  cs  "), "cs");
        assert!(same("PT_br", "pt-BR"));
    }

    #[test]
    fn a_numeric_region_survives() {
        // 419 is Latin America, and it is a region even though it is
        // three digits rather than two letters.
        assert_eq!(canonical("es-419"), "es-419");
    }

    #[test]
    fn an_unrecognised_tag_is_kept_rather_than_refused() {
        // Refusing a language nobody anticipated would be worse than
        // carrying it through with its case tidied.
        assert_eq!(canonical("qya"), "qya");
        assert_eq!(canonical("x-PIRATE"), "x-pirate");
        assert_eq!(display_name("qya"), "qya");
    }

    #[test]
    fn nothing_in_gives_nothing_out() {
        assert_eq!(canonical(""), "");
        assert_eq!(canonical("   "), "");
        assert_eq!(display_name(""), "");
        assert_eq!(language(""), "");
    }

    #[test]
    fn a_language_calls_itself_what_its_speakers_call_it() {
        // A menu is read by someone looking for their own language.
        assert_eq!(display_name("cs"), "Čeština");
        assert_eq!(display_name("de"), "Deutsch");
        assert_eq!(display_name("EN"), "English");
    }

    #[test]
    fn two_translations_of_one_language_stay_apart_in_the_menu() {
        assert_eq!(display_name("pt"), "Português");
        assert_eq!(display_name("pt-BR"), "Português (BR)");
        assert_ne!(display_name("pt"), display_name("pt-BR"));
    }

    #[test]
    fn the_language_of_a_tag_ignores_its_region() {
        assert_eq!(language("pt-BR"), "pt");
        assert_eq!(language("zh-Hant-CN"), "zh");
        assert_eq!(language("cs"), "cs");
    }

    #[test]
    fn every_endonym_is_listed_under_a_canonical_tag() {
        // A table keyed by a tag that needs fixing would never match.
        for (tag, name) in ENDONYMS {
            assert_eq!(canonical(tag), *tag, "{tag} is not canonical");
            assert!(!name.is_empty());
        }
        // And sorted, so the next one added has an obvious home.
        let mut sorted: Vec<&str> = ENDONYMS.iter().map(|(tag, _)| *tag).collect();
        let original = sorted.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, original, "the table is out of order");
    }
}
