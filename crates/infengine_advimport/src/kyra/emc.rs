//! A12.1: the Kyrandia `.EMC` room script.
//!
//! Every room ships one, and it holds both the room's logic and its
//! words. It is an IFF file, so big-endian sizes and four-character
//! chunk ids:
//!
//! ```text
//!   FORM <be32 size> EMC2
//!     ORDR <be32 size>   entry points into the bytecode
//!     TEXT <be32 size>   offset table, then NUL-terminated strings
//!     DATA <be32 size>   the bytecode itself
//! ```
//!
//! The `TEXT` chunk uses the old trick where the table's own length is
//! implied: the first offset points at the first string, so dividing it
//! by two gives the number of strings. No count is stored anywhere.
//!
//! This is where a translated Kyrandia differs from the original - the
//! Czech release ships the same rooms with `TEXT` rewritten in CP1250 -
//! so decoding this chunk yields the dialogue of whichever language the
//! archives came from.
//!
//! The bytecode is kept but not interpreted yet. When it is, it should
//! compile to the engine's own action list rather than be emulated, so
//! that a room's behaviour becomes editable data like everything else.

use std::fmt;

use crate::encoding::{self, Encoding};

/// Chunk ids we know.
const ID_ORDR: &[u8; 4] = b"ORDR";
const ID_TEXT: &[u8; 4] = b"TEXT";
const ID_DATA: &[u8; 4] = b"DATA";

/// Longest string worth believing, to bound a corrupt offset.
const MAX_STRING: usize = 4096;

/// One IFF chunk: its four-character id and its bytes.
type Chunk<'a> = ([u8; 4], &'a [u8]);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmcError {
    /// Not an IFF file.
    NotIff,
    /// An IFF file, but not an EMC2 form.
    WrongForm([u8; 4]),
    /// A chunk header ran past the end of the file.
    TruncatedChunk { at: usize },
}

impl fmt::Display for EmcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotIff => write!(f, "EMC does not start with an IFF FORM"),
            Self::WrongForm(id) => {
                write!(f, "EMC form type is {:?}, not EMC2", String::from_utf8_lossy(id))
            }
            Self::TruncatedChunk { at } => write!(f, "EMC chunk header at byte {at} is cut short"),
        }
    }
}

impl std::error::Error for EmcError {}

/// A decoded room script.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Script {
    /// Byte offsets into [`Self::code`] where the game enters the
    /// script. Kept for when the bytecode is compiled rather than
    /// discarded.
    pub entry_points: Vec<u16>,
    /// The room's lines, in the order the file stores them.
    pub texts: Vec<String>,
    /// The bytecode, untouched.
    pub code: Vec<u8>,
}

impl Script {
    /// Stable id for the line at `index`, scoped by room.
    ///
    /// The index is the script's own ordering, which the translated
    /// releases preserve - that is what lets the Czech text line up with
    /// the English under one key.
    pub fn line_id(room: &str, index: usize) -> String {
        format!("{room}.{index:02}")
    }

    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Split an EMC into its chunks.
fn chunks(data: &[u8]) -> Result<Vec<Chunk<'_>>, EmcError> {
    if data.len() < 12 || &data[..4] != b"FORM" {
        return Err(EmcError::NotIff);
    }
    let form: [u8; 4] = data[8..12].try_into().map_err(|_| EmcError::NotIff)?;
    if &form != b"EMC2" {
        return Err(EmcError::WrongForm(form));
    }

    let mut out = Vec::new();
    let mut pos = 12usize;
    while pos + 8 <= data.len() {
        let id: [u8; 4] = data[pos..pos + 4]
            .try_into()
            .map_err(|_| EmcError::TruncatedChunk { at: pos })?;
        let size = be32(data, pos + 4).ok_or(EmcError::TruncatedChunk { at: pos })? as usize;
        let start = pos + 8;
        let end = start.saturating_add(size).min(data.len());
        out.push((id, &data[start..end]));
        // IFF pads odd-sized chunks to an even boundary.
        pos = start + size + (size & 1);
    }
    Ok(out)
}

/// Read the strings out of a `TEXT` chunk.
///
/// The count is implied by the first offset, so a chunk whose first
/// offset is odd or out of range yields nothing rather than a table of
/// garbage read from the middle of a string.
fn read_texts(chunk: &[u8], fallback: Encoding) -> Vec<String> {
    let Some(first) = be16(chunk, 0) else {
        return Vec::new();
    };
    let first = first as usize;
    if first < 2 || !first.is_multiple_of(2) || first > chunk.len() {
        return Vec::new();
    }

    let count = first / 2;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let Some(offset) = be16(chunk, i * 2) else {
            break;
        };
        let offset = offset as usize;
        if offset >= chunk.len() {
            continue;
        }
        let end = chunk[offset..]
            .iter()
            .position(|b| *b == 0)
            .map_or(chunk.len(), |p| offset + p)
            .min(offset + MAX_STRING);
        let (text, _) = encoding::decode(&chunk[offset..end], fallback);
        out.push(text);
    }
    out
}

/// Decode a room script.
///
/// `fallback` is the encoding to assume when the text is not valid
/// UTF-8: CP1250 for the Czech release, Latin-1 otherwise.
pub fn decode(data: &[u8], fallback: Encoding) -> Result<Script, EmcError> {
    let mut script = Script::default();
    for (id, chunk) in chunks(data)? {
        match &id {
            ID_ORDR => {
                script.entry_points = (0..chunk.len() / 2)
                    .filter_map(|i| be16(chunk, i * 2))
                    .collect();
            }
            ID_TEXT => script.texts = read_texts(chunk, fallback),
            ID_DATA => script.code = chunk.to_vec(),
            _ => {}
        }
    }
    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an EMC the way the retail files are laid out.
    fn build(entry_points: &[u16], texts: &[&[u8]], code: &[u8]) -> Vec<u8> {
        let mut text_chunk = Vec::new();
        let table_len = texts.len() * 2;
        let mut offset = table_len;
        for text in texts {
            text_chunk.extend_from_slice(&(offset as u16).to_be_bytes());
            offset += text.len() + 1;
        }
        for text in texts {
            text_chunk.extend_from_slice(text);
            text_chunk.push(0);
        }

        let mut ordr = Vec::new();
        for point in entry_points {
            ordr.extend_from_slice(&point.to_be_bytes());
        }

        let mut body = Vec::new();
        for (id, data) in [
            (b"ORDR", &ordr),
            (b"TEXT", &text_chunk),
            (b"DATA", &code.to_vec()),
        ] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(data.len() as u32).to_be_bytes());
            body.extend_from_slice(data);
            if !data.len().is_multiple_of(2) {
                body.push(0);
            }
        }

        let mut file = b"FORM".to_vec();
        file.extend_from_slice(&((body.len() + 4) as u32).to_be_bytes());
        file.extend_from_slice(b"EMC2");
        file.extend_from_slice(&body);
        file
    }

    #[test]
    fn a_room_script_yields_its_lines_in_order() {
        let file = build(
            &[74, 87, 209],
            &[
                b"Brynn... I always took her for granite.",
                b"She was always so kind to me.",
            ],
            &[0x45, 0x01, 0x47],
        );
        let script = decode(&file, Encoding::Latin1).expect("decode");
        assert_eq!(script.texts.len(), 2);
        assert_eq!(script.texts[0], "Brynn... I always took her for granite.");
        assert_eq!(script.texts[1], "She was always so kind to me.");
    }

    #[test]
    fn entry_points_and_bytecode_are_kept() {
        let file = build(&[74, 87, 209], &[b"line"], &[0x45, 0x01, 0x47, 0x02]);
        let script = decode(&file, Encoding::Latin1).expect("decode");
        assert_eq!(script.entry_points, vec![74, 87, 209]);
        assert_eq!(script.code, vec![0x45, 0x01, 0x47, 0x02]);
        assert!(!script.is_empty());
    }

    #[test]
    fn czech_text_comes_out_as_utf8() {
        // The translated release rewrites TEXT in CP1250; z-caron is
        // 0x9E and a-acute 0xE1.
        let line = [b'V', 0x9E, b'd', b'y', b'c', b'k', b'y', b' ', 0x9E, b'e', b'n', b'a'];
        let file = build(&[], &[&line], &[]);
        let script = decode(&file, Encoding::Cp1250).expect("decode");
        assert_eq!(script.texts[0], "Vždycky žena");
    }

    #[test]
    fn english_text_is_unaffected_by_the_fallback_encoding() {
        let file = build(&[], &[b"Plain ASCII line."], &[]);
        let script = decode(&file, Encoding::Cp1250).expect("decode");
        assert_eq!(script.texts[0], "Plain ASCII line.");
    }

    #[test]
    fn the_string_count_is_derived_from_the_first_offset() {
        // No count is stored; three strings means a six-byte table.
        let file = build(&[], &[b"a", b"bb", b"ccc"], &[]);
        let script = decode(&file, Encoding::Latin1).expect("decode");
        assert_eq!(script.texts, vec!["a", "bb", "ccc"]);
    }

    #[test]
    fn a_script_with_no_text_chunk_decodes_to_nothing() {
        let file = build(&[1, 2], &[], &[0xFF]);
        let script = decode(&file, Encoding::Latin1).expect("decode");
        assert!(script.is_empty());
        assert_eq!(script.entry_points, vec![1, 2]);
    }

    #[test]
    fn a_nonsense_offset_table_yields_no_strings_rather_than_garbage() {
        // An odd first offset cannot be a table of u16s; reading one
        // anyway would slice strings in half.
        let mut file = build(&[], &[b"line"], &[]);
        let at = file
            .windows(4)
            .position(|w| w == b"TEXT")
            .expect("TEXT chunk")
            + 8;
        file[at..at + 2].copy_from_slice(&3u16.to_be_bytes());
        let script = decode(&file, Encoding::Latin1).expect("decode");
        assert!(script.is_empty());
    }

    #[test]
    fn an_offset_past_the_chunk_is_skipped() {
        let mut file = build(&[], &[b"first", b"second"], &[]);
        let at = file
            .windows(4)
            .position(|w| w == b"TEXT")
            .expect("TEXT chunk")
            + 8;
        // Point the second string somewhere impossible.
        file[at + 2..at + 4].copy_from_slice(&9000u16.to_be_bytes());
        let script = decode(&file, Encoding::Latin1).expect("decode");
        assert_eq!(script.texts, vec!["first"]);
    }

    #[test]
    fn odd_sized_chunks_are_padded_so_later_chunks_still_parse() {
        // "DATA" comes last, so an odd TEXT chunk that is not padded
        // would misalign it.
        let file = build(&[], &[b"odd"], &[0xAA, 0xBB]);
        let script = decode(&file, Encoding::Latin1).expect("decode");
        assert_eq!(script.texts, vec!["odd"]);
        assert_eq!(script.code, vec![0xAA, 0xBB]);
    }

    #[test]
    fn a_non_iff_file_is_rejected() {
        assert_eq!(decode(b"nope", Encoding::Latin1), Err(EmcError::NotIff));
        assert_eq!(decode(&[], Encoding::Latin1), Err(EmcError::NotIff));
    }

    #[test]
    fn another_iff_form_is_named_in_the_error() {
        let mut file = build(&[], &[b"x"], &[]);
        file[8..12].copy_from_slice(b"ILBM");
        assert_eq!(
            decode(&file, Encoding::Latin1),
            Err(EmcError::WrongForm(*b"ILBM"))
        );
    }

    #[test]
    fn line_ids_are_stable_and_sort_in_order() {
        assert_eq!(Script::line_id("brins", 0), "brins.00");
        assert_eq!(Script::line_id("brins", 12), "brins.12");
        let mut ids = vec![
            Script::line_id("brins", 10),
            Script::line_id("brins", 2),
        ];
        ids.sort();
        assert_eq!(ids, vec!["brins.02", "brins.10"], "zero padding keeps order");
    }
}
