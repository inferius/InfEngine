//! A3.2: the Kyrandia `.PAK` archive.
//!
//! The directory is a flat run of `(u32 little-endian offset, NUL
//! terminated name)` pairs, ending with a sentinel whose name is empty
//! and whose offset marks the end of the last file. Entry sizes are
//! implied: each file runs from its own offset to the next one.
//!
//! Verified against a retail install, e.g. `ALCHEMY.PAK` starts
//! `49 00 00 00 "ALCHEMY.DAT\0" C4 00 00 00 "ALCHEMY.MSC\0" ...`.

use std::fmt;

/// A malformed archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PakError {
    /// Too small to hold even a directory sentinel.
    TooSmall { len: usize },
    /// A name ran to the end of the file without a terminator, which
    /// means the directory overlapped the data.
    UnterminatedName { at: usize },
    /// An entry pointed outside the archive.
    OffsetOutOfRange { name: String, offset: usize },
    /// Entry offsets went backwards, so a size would be negative.
    NonMonotonicOffsets { name: String },
}

impl fmt::Display for PakError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall { len } => write!(f, "PAK is only {len} bytes, too small for a directory"),
            Self::UnterminatedName { at } => {
                write!(f, "PAK entry name at byte {at} has no terminator")
            }
            Self::OffsetOutOfRange { name, offset } => {
                write!(f, "PAK entry {name:?} points to byte {offset}, past the archive")
            }
            Self::NonMonotonicOffsets { name } => {
                write!(f, "PAK entry {name:?} starts before the entry preceding it")
            }
        }
    }
}

impl std::error::Error for PakError {}

/// One file inside the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakEntry {
    /// Name as stored, e.g. `ALCHEMY.CPS`. Case is whatever the archive
    /// used, which on retail data is uppercase.
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

impl PakEntry {
    /// Uppercase extension without the dot, or `""` when the name has
    /// none. Used to route an entry to the right decoder.
    pub fn extension(&self) -> String {
        self.name
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_uppercase())
            .unwrap_or_default()
    }
}

/// A parsed archive directory, borrowing the file bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Pak<'a> {
    data: &'a [u8],
    entries: Vec<PakEntry>,
}

impl<'a> Pak<'a> {
    /// Parse the directory at the start of `data`.
    pub fn parse(data: &'a [u8]) -> Result<Self, PakError> {
        if data.len() < 5 {
            return Err(PakError::TooSmall { len: data.len() });
        }

        // Collect (offset, name) pairs until the sentinel.
        let mut raw: Vec<(usize, String)> = Vec::new();
        let mut pos = 0usize;
        loop {
            if pos + 4 > data.len() {
                return Err(PakError::UnterminatedName { at: pos });
            }
            let offset =
                u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                    as usize;
            pos += 4;

            let start = pos;
            while pos < data.len() && data[pos] != 0 {
                pos += 1;
            }
            if pos >= data.len() {
                return Err(PakError::UnterminatedName { at: start });
            }
            let name = String::from_utf8_lossy(&data[start..pos]).into_owned();
            pos += 1; // the NUL

            // The sentinel closes the directory and supplies the end
            // offset of the final file.
            if name.is_empty() {
                raw.push((offset, String::new()));
                break;
            }
            raw.push((offset, name));

            // A directory that reaches its own first data offset is
            // finished even without a sentinel; some archives are
            // padded rather than terminated.
            if let Some((first, _)) = raw.first()
                && pos >= *first
            {
                break;
            }
        }

        // Turn offsets into sizes. The end of each file is the start of
        // the next one; the last real file ends at the sentinel offset,
        // or at the end of the archive when the sentinel is absent or
        // implausible.
        let mut entries = Vec::new();
        for i in 0..raw.len() {
            let (offset, ref name) = raw[i];
            if name.is_empty() {
                continue;
            }
            if offset > data.len() {
                return Err(PakError::OffsetOutOfRange {
                    name: name.clone(),
                    offset,
                });
            }
            let end = raw
                .get(i + 1)
                .map(|(next, _)| *next)
                .filter(|next| *next <= data.len())
                .unwrap_or(data.len());
            if end < offset {
                return Err(PakError::NonMonotonicOffsets { name: name.clone() });
            }
            entries.push(PakEntry {
                name: name.clone(),
                offset,
                size: end - offset,
            });
        }

        Ok(Self { data, entries })
    }

    pub fn entries(&self) -> &[PakEntry] {
        &self.entries
    }

    /// Bytes of an entry by index.
    pub fn data_at(&self, index: usize) -> Option<&'a [u8]> {
        let entry = self.entries.get(index)?;
        self.data.get(entry.offset..entry.offset + entry.size)
    }

    /// Bytes of an entry by name, matched case-insensitively.
    pub fn get(&self, name: &str) -> Option<&'a [u8]> {
        let index = self
            .entries
            .iter()
            .position(|e| e.name.eq_ignore_ascii_case(name))?;
        self.data_at(index)
    }

    /// Entries whose extension matches, e.g. `"CPS"` for backgrounds.
    pub fn with_extension(&self, ext: &str) -> impl Iterator<Item = (usize, &PakEntry)> {
        let ext = ext.to_ascii_uppercase();
        self.entries
            .iter()
            .enumerate()
            .filter(move |(_, e)| e.extension() == ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an archive the same way the retail files are laid out.
    fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
        // Directory size: per entry 4 bytes + name + NUL, plus the
        // 4-byte + NUL sentinel.
        let dir_len: usize = files
            .iter()
            .map(|(name, _)| 4 + name.len() + 1)
            .sum::<usize>()
            + 5;

        let mut dir = Vec::new();
        let mut body = Vec::new();
        for (name, data) in files {
            let offset = dir_len + body.len();
            dir.extend_from_slice(&(offset as u32).to_le_bytes());
            dir.extend_from_slice(name.as_bytes());
            dir.push(0);
            body.extend_from_slice(data);
        }
        dir.extend_from_slice(&((dir_len + body.len()) as u32).to_le_bytes());
        dir.push(0);

        assert_eq!(dir.len(), dir_len, "test builder disagrees with itself");
        dir.extend_from_slice(&body);
        dir
    }

    #[test]
    fn parses_a_two_file_archive() {
        let archive = build(&[("ALCHEMY.DAT", b"first"), ("ALCHEMY.CPS", b"second!!")]);
        let pak = Pak::parse(&archive).expect("parse");
        assert_eq!(pak.entries().len(), 2);
        assert_eq!(pak.entries()[0].name, "ALCHEMY.DAT");
        assert_eq!(pak.entries()[0].size, 5);
        assert_eq!(pak.entries()[1].size, 8);
    }

    #[test]
    fn entry_bytes_are_exact() {
        let archive = build(&[("A.DAT", b"first"), ("B.CPS", b"second!!")]);
        let pak = Pak::parse(&archive).expect("parse");
        assert_eq!(pak.get("A.DAT"), Some(&b"first"[..]));
        assert_eq!(pak.get("B.CPS"), Some(&b"second!!"[..]));
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let archive = build(&[("ALCHEMY.CPS", b"pixels")]);
        let pak = Pak::parse(&archive).expect("parse");
        assert_eq!(pak.get("alchemy.cps"), Some(&b"pixels"[..]));
        assert!(pak.get("missing.cps").is_none());
    }

    #[test]
    fn extension_filter_selects_backgrounds() {
        let archive = build(&[
            ("ROOM.DAT", b"d"),
            ("ROOM.CPS", b"c"),
            ("ROOM.MSC", b"m"),
            ("OTHER.CPS", b"c2"),
        ]);
        let pak = Pak::parse(&archive).expect("parse");
        let names: Vec<_> = pak
            .with_extension("cps")
            .map(|(_, e)| e.name.clone())
            .collect();
        assert_eq!(names, vec!["ROOM.CPS", "OTHER.CPS"]);
    }

    #[test]
    fn extension_of_a_name_without_a_dot_is_empty() {
        let entry = PakEntry {
            name: "NODOT".into(),
            offset: 0,
            size: 0,
        };
        assert_eq!(entry.extension(), "");
    }

    #[test]
    fn a_single_file_archive_ends_at_the_sentinel() {
        let archive = build(&[("ONLY.CPS", b"abcdefgh")]);
        let pak = Pak::parse(&archive).expect("parse");
        assert_eq!(pak.entries().len(), 1);
        assert_eq!(pak.get("ONLY.CPS"), Some(&b"abcdefgh"[..]));
    }

    #[test]
    fn tiny_input_is_rejected() {
        assert_eq!(Pak::parse(&[0, 0]), Err(PakError::TooSmall { len: 2 }));
    }

    #[test]
    fn unterminated_name_is_rejected() {
        // Offset then a name that never hits a NUL.
        let mut archive = vec![10, 0, 0, 0];
        archive.extend_from_slice(b"NOTERM");
        assert!(matches!(
            Pak::parse(&archive),
            Err(PakError::UnterminatedName { .. })
        ));
    }

    #[test]
    fn offset_past_the_archive_is_rejected() {
        let mut archive = Vec::new();
        archive.extend_from_slice(&9999u32.to_le_bytes());
        archive.extend_from_slice(b"FAR.CPS\0");
        archive.extend_from_slice(&0u32.to_le_bytes());
        archive.push(0);
        assert!(matches!(
            Pak::parse(&archive),
            Err(PakError::OffsetOutOfRange { .. })
        ));
    }

    #[test]
    fn missing_sentinel_falls_back_to_the_archive_end() {
        // Directory whose entries run straight into the data, with no
        // empty-name terminator: the loop stops once it reaches the
        // first data offset.
        let mut archive = Vec::new();
        let dir_len = 4 + b"X.CPS\0".len(); // one entry, no sentinel
        archive.extend_from_slice(&(dir_len as u32).to_le_bytes());
        archive.extend_from_slice(b"X.CPS\0");
        archive.extend_from_slice(b"payload");
        let pak = Pak::parse(&archive).expect("parse");
        assert_eq!(pak.entries().len(), 1);
        assert_eq!(pak.get("X.CPS"), Some(&b"payload"[..]));
    }
}
