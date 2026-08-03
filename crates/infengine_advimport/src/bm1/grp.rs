//! A76: Posel smrti's `.grp` archives.
//!
//! The AGDS engine keeps its art in plain uncompressed archives: a
//! header, a table of named entries, and the file bytes at the offsets
//! the table gives. The only twist is a light obfuscation - the header
//! magic and the names may arrive XORed with a fixed key and inverted -
//! which the format's public documentation spells out, key and all.
//! The reading here is our own.
//!
//! Layout, little-endian throughout:
//!
//! ```text
//!   0x00  16 B   "AGDS group file\x1A", possibly obfuscated
//!   0x10  u32    header size (0x2C)
//!   0x14  4 B    magic E6 C9 03 1A
//!   0x18  u32    version (2)
//!   0x1C  u32    entry count
//!   0x2C  entries, 0x31 bytes each:
//!         0x00  33 B  name, NUL-terminated, Windows-1250; the text
//!               is obfuscated with the key phase restarting at each
//!               name, the terminator and padding stay plain zeros
//!         0x21  u32   offset of the file's bytes
//!         0x25  u32   size
//!         0x29  8 B   unused here
//! ```

use std::fmt;

/// The obfuscation key the engine uses, verbatim - the developers'
/// own warning against doing exactly this.
const KEY: &[u8] = b"Vyvojovy tym AGDS varuje: Hackerovani skodi obchodu!";

/// The plain header magic.
const MAGIC: &[u8; 16] = b"AGDS group file\x1a";

const HEADER: usize = 0x2C;
const ENTRY: usize = 0x31;
const NAME: usize = 33;

/// Undo the obfuscation in place: XOR with the cycling key, inverted.
pub fn dexor(bytes: &mut [u8]) {
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte ^= KEY[index % KEY.len()] ^ 0xFF;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrpEntry {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrpError {
    TooShort,
    BadMagic,
    /// The table promises bytes past the end of the file.
    Truncated { entry: String },
}

impl fmt::Display for GrpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "not a .grp: shorter than its header"),
            Self::BadMagic => write!(f, "not a .grp: magic missing, plain or obfuscated"),
            Self::Truncated { entry } => {
                write!(f, "{entry} points past the end of the archive")
            }
        }
    }
}

impl std::error::Error for GrpError {}

/// A parsed archive, borrowing the file's bytes.
pub struct Grp<'a> {
    data: &'a [u8],
    entries: Vec<GrpEntry>,
}

impl fmt::Debug for Grp<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grp")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl<'a> Grp<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, GrpError> {
        if data.len() < HEADER {
            return Err(GrpError::TooShort);
        }
        // The magic tells us whether this copy is obfuscated: read it
        // both ways rather than trusting a flag the format lacks.
        let mut head = [0u8; 16];
        head.copy_from_slice(&data[..16]);
        let obfuscated = if head == *MAGIC {
            false
        } else {
            dexor(&mut head);
            if head != *MAGIC {
                return Err(GrpError::BadMagic);
            }
            true
        };

        let count = u32::from_le_bytes([data[0x1C], data[0x1D], data[0x1E], data[0x1F]]) as usize;
        let mut entries = Vec::with_capacity(count.min(1 << 16));
        for index in 0..count {
            let at = HEADER + index * ENTRY;
            let Some(record) = data.get(at..at + ENTRY) else {
                break;
            };
            let mut name = [0u8; NAME];
            name.copy_from_slice(&record[..NAME]);
            // The terminator and padding stay plain zeros; only the
            // text is obfuscated, and its key phase restarts per name.
            let nul = name.iter().position(|&b| b == 0).unwrap_or(NAME);
            if obfuscated {
                dexor(&mut name[..nul]);
            }
            let (text, _) = crate::encoding::decode(&name[..nul], crate::encoding::Encoding::Cp1250);
            let offset =
                u32::from_le_bytes([record[0x21], record[0x22], record[0x23], record[0x24]])
                    as usize;
            let size =
                u32::from_le_bytes([record[0x25], record[0x26], record[0x27], record[0x28]])
                    as usize;
            if offset + size > data.len() {
                return Err(GrpError::Truncated { entry: text });
            }
            entries.push(GrpEntry {
                name: text,
                offset,
                size,
            });
        }
        Ok(Self { data, entries })
    }

    pub fn entries(&self) -> &[GrpEntry] {
        &self.entries
    }

    /// One entry's bytes, as stored.
    pub fn data_of(&self, entry: &GrpEntry) -> &'a [u8] {
        &self.data[entry.offset..entry.offset + entry.size]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(obfuscate: bool) -> Vec<u8> {
        let mut out = vec![0u8; HEADER];
        out[..16].copy_from_slice(MAGIC);
        out[0x10..0x14].copy_from_slice(&(HEADER as u32).to_le_bytes());
        out[0x14..0x18].copy_from_slice(&[0xE6, 0xC9, 0x03, 0x1A]);
        out[0x18..0x1C].copy_from_slice(&2u32.to_le_bytes());
        out[0x1C..0x20].copy_from_slice(&1u32.to_le_bytes());

        let payload = b"pixels";
        let data_at = HEADER + ENTRY;
        let mut entry = vec![0u8; ENTRY];
        entry[..7].copy_from_slice(b"a.tga\0\0");
        entry[0x21..0x25].copy_from_slice(&(data_at as u32).to_le_bytes());
        entry[0x25..0x29].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&entry);
        out.extend_from_slice(payload);

        if obfuscate {
            // The engine's own scheme: the magic, and each name's text
            // with the key starting over.
            dexor(&mut out[..16]);
            dexor(&mut out[HEADER..HEADER + 5]);
        }
        out
    }

    #[test]
    fn a_plain_archive_lists_and_reads() {
        let bytes = archive(false);
        let grp = Grp::parse(&bytes).expect("parses");
        assert_eq!(grp.entries().len(), 1);
        assert_eq!(grp.entries()[0].name, "a.tga");
        assert_eq!(grp.data_of(&grp.entries()[0]), b"pixels");
    }

    #[test]
    fn an_obfuscated_archive_reads_the_same() {
        let bytes = archive(true);
        let grp = Grp::parse(&bytes).expect("parses");
        assert_eq!(grp.entries()[0].name, "a.tga");
        assert_eq!(grp.data_of(&grp.entries()[0]), b"pixels");
    }

    #[test]
    fn the_obfuscation_is_its_own_inverse() {
        let mut bytes = b"Posel smrti".to_vec();
        dexor(&mut bytes);
        assert_ne!(bytes, b"Posel smrti");
        dexor(&mut bytes);
        assert_eq!(bytes, b"Posel smrti");
    }

    #[test]
    fn garbage_is_refused_with_a_reason() {
        assert_eq!(Grp::parse(&[1, 2, 3]).err(), Some(GrpError::TooShort));
        let junk = vec![7u8; 64];
        assert_eq!(Grp::parse(&junk).err(), Some(GrpError::BadMagic));
    }
}
