//! A77: Posel smrti's `.adb` object database.
//!
//! Everything that is not art lives here: strings, dialogue, screen
//! regions, references, and the game's bytecode, all as named entries
//! in one keyed pool. The layout is public knowledge, and it also
//! matches what our own earlier digging measured byte for byte - the
//! two readings differ only in where they drew the header boundary.
//!
//! Layout, little-endian:
//!
//! ```text
//!   0x00  u32   magic 0x29A
//!   0x04  u32   zero
//!   0x08  u32   entry count
//!   0x0C  u32   entry count again
//!   0x10  u32   marker 31
//!   0x14  entries, 0x28 bytes each:
//!         0x00  u32   offset of the payload, relative to the pool
//!         0x04  32 B  key, NUL-terminated, Windows-1250
//!         0x24  u32   payload size
//!   pool  starts right after the table
//! ```
//!
//! A payload whose bytes 8..12 read `AD DE 0C 00` is bytecode; the
//! rest are raw records whose meaning hangs on the key's suffix.

use std::fmt;

const MAGIC: u32 = 0x29A;
const HEADER: usize = 0x14;
const RECORD: usize = 0x28;
const KEY: usize = 32;

/// The four bytes that mark a code payload.
const CODE_MARK: [u8; 4] = [0xAD, 0xDE, 0x0C, 0x00];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbEntry {
    pub key: String,
    /// Payload offset relative to the pool.
    pub offset: usize,
    pub size: usize,
}

impl AdbEntry {
    /// The key's suffix - `.r`, `.ts`, `.p` - which is what hints at
    /// the record's meaning.
    pub fn suffix(&self) -> &str {
        self.key
            .rsplit_once('.')
            .map(|(_, s)| s)
            .unwrap_or("")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdbError {
    TooShort,
    BadMagic { found: u32 },
    /// The two counts disagree, so the header is not where we think.
    CountMismatch { first: u32, second: u32 },
}

impl fmt::Display for AdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "not an .adb: shorter than its header"),
            Self::BadMagic { found } => {
                write!(f, "not an .adb: magic {found:#x} instead of {MAGIC:#x}")
            }
            Self::CountMismatch { first, second } => {
                write!(f, "not an .adb: counts disagree, {first} vs {second}")
            }
        }
    }
}

impl std::error::Error for AdbError {}

/// A parsed database, borrowing the file's bytes.
pub struct Adb<'a> {
    data: &'a [u8],
    pool: usize,
    entries: Vec<AdbEntry>,
}

impl fmt::Debug for Adb<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Adb")
            .field("entries", &self.entries.len())
            .finish()
    }
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

impl<'a> Adb<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, AdbError> {
        if data.len() < HEADER {
            return Err(AdbError::TooShort);
        }
        let magic = u32_at(data, 0);
        if magic != MAGIC {
            return Err(AdbError::BadMagic { found: magic });
        }
        let count = u32_at(data, 8);
        let again = u32_at(data, 12);
        if count != again {
            return Err(AdbError::CountMismatch {
                first: count,
                second: again,
            });
        }

        let pool = HEADER + RECORD * count as usize;
        let mut entries = Vec::with_capacity((count as usize).min(1 << 20));
        for index in 0..count as usize {
            let at = HEADER + index * RECORD;
            let Some(record) = data.get(at..at + RECORD) else {
                break;
            };
            let offset = u32_at(record, 0) as usize;
            let name = &record[4..4 + KEY];
            let nul = name.iter().position(|&b| b == 0).unwrap_or(KEY);
            let (key, _) =
                crate::encoding::decode(&name[..nul], crate::encoding::Encoding::Cp1250);
            let size = u32_at(record, 4 + KEY) as usize;
            // The last record is a sentinel pointing nowhere; keeping
            // it would hand out a slice that panics. Skipped the same
            // way empty globals are: present in the table, no bytes.
            if pool + offset + size > data.len() {
                continue;
            }
            entries.push(AdbEntry { key, offset, size });
        }
        Ok(Self {
            data,
            pool,
            entries,
        })
    }

    pub fn entries(&self) -> &[AdbEntry] {
        &self.entries
    }

    /// One entry's payload, as stored.
    pub fn data_of(&self, entry: &AdbEntry) -> &'a [u8] {
        &self.data[self.pool + entry.offset..self.pool + entry.offset + entry.size]
    }

    /// Whether a payload is bytecode.
    pub fn is_code(payload: &[u8]) -> bool {
        payload.len() >= 12 && payload[8..12] == CODE_MARK
    }

    /// One entry's payload with the obfuscation undone.
    ///
    /// Text-bearing records arrive XORed with the same key phrase the
    /// archives use, phase restarting per payload - measured on the
    /// retail data: `.ts` entries become the dialogue's voice file
    /// names, `.t` entries its words.
    pub fn plain_of(&self, entry: &AdbEntry) -> Vec<u8> {
        let mut out = self.data_of(entry).to_vec();
        crate::bm1::grp::dexor(&mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&31u32.to_le_bytes());

        let payload_a = b"ahoj svete";
        let mut code = vec![0u8; 8];
        code.extend_from_slice(&CODE_MARK);
        code.extend_from_slice(&[1, 2, 3]);

        let record = |offset: usize, key: &[u8], size: usize| {
            let mut r = vec![0u8; RECORD];
            r[..4].copy_from_slice(&(offset as u32).to_le_bytes());
            r[4..4 + key.len()].copy_from_slice(key);
            r[36..40].copy_from_slice(&(size as u32).to_le_bytes());
            r
        };
        out.extend(record(0, b"uvod.ts", payload_a.len()));
        out.extend(record(payload_a.len(), b"scena.s", code.len()));
        out.extend_from_slice(payload_a);
        out.extend_from_slice(&code);
        out
    }

    #[test]
    fn entries_list_and_read_back() {
        let bytes = database();
        let adb = Adb::parse(&bytes).expect("parses");
        assert_eq!(adb.entries().len(), 2);
        assert_eq!(adb.entries()[0].key, "uvod.ts");
        assert_eq!(adb.entries()[0].suffix(), "ts");
        assert_eq!(adb.data_of(&adb.entries()[0]), b"ahoj svete");
    }

    #[test]
    fn plain_of_undoes_the_payload_obfuscation() {
        let mut bytes = database();
        // Obfuscate the first payload in place, as the retail file is.
        let pool = HEADER + RECORD * 2;
        crate::bm1::grp::dexor(&mut bytes[pool..pool + 10]);
        let adb = Adb::parse(&bytes).expect("parses");
        assert_eq!(adb.plain_of(&adb.entries()[0]), b"ahoj svete");
    }

    #[test]
    fn code_is_told_from_raw_by_its_mark() {
        let bytes = database();
        let adb = Adb::parse(&bytes).expect("parses");
        assert!(!Adb::is_code(adb.data_of(&adb.entries()[0])));
        assert!(Adb::is_code(adb.data_of(&adb.entries()[1])));
    }

    #[test]
    fn a_sentinel_pointing_nowhere_is_skipped_not_fatal() {
        let mut bytes = database();
        // Rewrite the second record to point far past the end.
        let at = HEADER + RECORD;
        bytes[at..at + 4].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
        let adb = Adb::parse(&bytes).expect("parses");
        assert_eq!(adb.entries().len(), 1);
    }

    #[test]
    fn garbage_is_refused_with_a_reason() {
        assert_eq!(Adb::parse(&[1, 2]).err(), Some(AdbError::TooShort));
        let junk = vec![7u8; 64];
        assert!(matches!(Adb::parse(&junk).err(), Some(AdbError::BadMagic { .. })));
    }
}
