//! A10.3: getting old game text into UTF-8.
//!
//! Everything our format stores is UTF-8. The games are older than that
//! assumption: Kyrandia's Czech translation writes its dialogue in
//! CP1250, and Black Mirror's XML declares `encoding="utf-8"` while
//! carrying the occasional Latin-1 byte in a German label.
//!
//! The tempting shortcut is `String::from_utf8_lossy`. It never fails,
//! and it silently replaces every byte it does not understand with a
//! replacement character. On a file that is genuinely CP1250 that
//! destroys every accented letter in the game. So the rule here is: try
//! UTF-8 strictly, and only when that fails fall back to a code page,
//! which maps every possible byte and therefore cannot lose anything.

/// Characters CP1250 assigns to bytes 0x80..=0xFF.
///
/// The five positions the code page leaves undefined map to the
/// replacement character rather than to a guess.
const CP1250_HIGH: [char; 128] = [
    '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{FFFD}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{FFFD}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{015A}', '\u{0164}', '\u{017D}', '\u{0179}',
    '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{FFFD}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{015B}', '\u{0165}', '\u{017E}', '\u{017A}',
    '\u{00A0}', '\u{02C7}', '\u{02D8}', '\u{0141}', '\u{00A4}', '\u{0104}', '\u{00A6}', '\u{00A7}',
    '\u{00A8}', '\u{00A9}', '\u{015E}', '\u{00AB}', '\u{00AC}', '\u{00AD}', '\u{00AE}', '\u{017B}',
    '\u{00B0}', '\u{00B1}', '\u{02DB}', '\u{0142}', '\u{00B4}', '\u{00B5}', '\u{00B6}', '\u{00B7}',
    '\u{00B8}', '\u{0105}', '\u{015F}', '\u{00BB}', '\u{013D}', '\u{02DD}', '\u{013E}', '\u{017C}',
    '\u{0154}', '\u{00C1}', '\u{00C2}', '\u{0102}', '\u{00C4}', '\u{0139}', '\u{0106}', '\u{00C7}',
    '\u{010C}', '\u{00C9}', '\u{0118}', '\u{00CB}', '\u{011A}', '\u{00CD}', '\u{00CE}', '\u{010E}',
    '\u{0110}', '\u{0143}', '\u{0147}', '\u{00D3}', '\u{00D4}', '\u{0150}', '\u{00D6}', '\u{00D7}',
    '\u{0158}', '\u{016E}', '\u{00DA}', '\u{0170}', '\u{00DC}', '\u{00DD}', '\u{0162}', '\u{00DF}',
    '\u{0155}', '\u{00E1}', '\u{00E2}', '\u{0103}', '\u{00E4}', '\u{013A}', '\u{0107}', '\u{00E7}',
    '\u{010D}', '\u{00E9}', '\u{0119}', '\u{00EB}', '\u{011B}', '\u{00ED}', '\u{00EE}', '\u{010F}',
    '\u{0111}', '\u{0144}', '\u{0148}', '\u{00F3}', '\u{00F4}', '\u{0151}', '\u{00F6}', '\u{00F7}',
    '\u{0159}', '\u{016F}', '\u{00FA}', '\u{0171}', '\u{00FC}', '\u{00FD}', '\u{0163}', '\u{02D9}',
];

/// Which code page a byte string was read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// The bytes were already valid UTF-8.
    Utf8,
    /// Central European, used by the Czech translations.
    Cp1250,
    /// Western European, used for German and French text.
    Latin1,
}

/// Decode CP1250 bytes.
///
/// Cannot fail: every byte has a mapping.
pub fn cp1250_to_utf8(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b < 0x80 {
                b as char
            } else {
                CP1250_HIGH[(b - 0x80) as usize]
            }
        })
        .collect()
}

/// Decode Latin-1 bytes. Cannot fail: the code page is the first 256
/// Unicode code points.
pub fn latin1_to_utf8(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Decode text of unknown encoding, preferring UTF-8.
///
/// Returns the text and which encoding produced it, so a caller can log
/// what it guessed rather than the guess being invisible.
pub fn decode(bytes: &[u8], fallback: Encoding) -> (String, Encoding) {
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), Encoding::Utf8),
        Err(_) => match fallback {
            Encoding::Cp1250 => (cp1250_to_utf8(bytes), Encoding::Cp1250),
            _ => (latin1_to_utf8(bytes), Encoding::Latin1),
        },
    }
}

/// Strip a UTF-8 byte order mark, which some of these files carry and
/// which would otherwise show up as an invisible first character in
/// every id read from the file.
pub fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_utf8_is_taken_as_it_is() {
        let bytes = "Vždycky byla jako kámen.".as_bytes();
        let (text, encoding) = decode(bytes, Encoding::Cp1250);
        assert_eq!(text, "Vždycky byla jako kámen.");
        assert_eq!(encoding, Encoding::Utf8);
    }

    #[test]
    fn czech_cp1250_decodes_to_the_right_letters() {
        // "Vždycky" in CP1250: z-caron is 0x9E, y-acute 0xFD.
        let bytes = [b'V', 0x9E, b'd', b'y', b'c', b'k', b'y'];
        let (text, encoding) = decode(&bytes, Encoding::Cp1250);
        assert_eq!(text, "Vždycky");
        assert_eq!(encoding, Encoding::Cp1250);
    }

    #[test]
    fn the_whole_czech_alphabet_survives() {
        // Every accented letter Czech uses, as CP1250 bytes.
        let bytes: Vec<u8> = vec![
            0xE1, 0xE8, 0xEF, 0xE9, 0xEC, 0xED, 0xF2, 0xF3, 0xF8, 0x9A, 0x9D, 0xFA, 0xF9, 0xFD,
            0x9E,
        ];
        let text = cp1250_to_utf8(&bytes);
        assert_eq!(text, "áčďéěíňóřšťúůýž");
    }

    #[test]
    fn lossy_conversion_would_have_destroyed_that_text() {
        // The point of this module: from_utf8_lossy turns every accented
        // byte into a replacement character.
        let bytes = [b'V', 0x9E, b'd', b'y'];
        let lossy = String::from_utf8_lossy(&bytes);
        assert!(lossy.contains('\u{FFFD}'), "lossy loses the letter");
        assert!(!cp1250_to_utf8(&bytes).contains('\u{FFFD}'));
    }

    #[test]
    fn german_latin1_decodes_when_utf8_fails() {
        // "Tür" with a Latin-1 u-umlaut.
        let bytes = [b'T', 0xFC, b'r'];
        let (text, encoding) = decode(&bytes, Encoding::Latin1);
        assert_eq!(text, "Tür");
        assert_eq!(encoding, Encoding::Latin1);
    }

    #[test]
    fn ascii_is_identical_under_every_code_page() {
        let bytes = b"Hello, world! 0123";
        assert_eq!(cp1250_to_utf8(bytes), "Hello, world! 0123");
        assert_eq!(latin1_to_utf8(bytes), "Hello, world! 0123");
    }

    #[test]
    fn undefined_code_points_become_the_replacement_character() {
        // CP1250 leaves 0x81 unassigned; guessing a letter there would
        // be worse than admitting it is unknown.
        assert_eq!(cp1250_to_utf8(&[0x81]), "\u{FFFD}");
    }

    #[test]
    fn every_byte_has_a_mapping() {
        // A code page fallback is only safe because it is total.
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(cp1250_to_utf8(&all).chars().count(), 256);
        assert_eq!(latin1_to_utf8(&all).chars().count(), 256);
    }

    #[test]
    fn a_byte_order_mark_is_removed() {
        assert_eq!(strip_bom("\u{FEFF}Label"), "Label");
        assert_eq!(strip_bom("Label"), "Label");
    }

    #[test]
    fn empty_input_is_handled() {
        let (text, encoding) = decode(&[], Encoding::Cp1250);
        assert!(text.is_empty());
        assert_eq!(encoding, Encoding::Utf8);
    }
}
