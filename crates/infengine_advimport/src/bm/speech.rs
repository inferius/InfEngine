//! A10.2: the Black Mirror voice archive.
//!
//! `loca/<language>/speech.dat` holds every recorded line for one
//! language, as Ogg Vorbis plus a lip sync file per line, behind a flat
//! index of readable paths:
//!
//! ```text
//!   [u32 entry count]
//!   [u32 path length][path][u32 offset][u32 size]   x count
//! ```
//!
//! Offsets are absolute. The Czech archive checks out exactly: the index
//! ends at 837540, the first entry starts four bytes later, and the last
//! entry's offset plus size lands on the file's final byte.
//!
//! What makes this worth importing rather than just noting: the file
//! name carries the same label the subtitle uses, so
//! `speech/angelina/angelina0_00295.ogg` is the recording of the line
//! `<Label value="angelina0_00295">`. Text, audio and lip sync are
//! already one keyed set - our own format only has to preserve that.
//!
//! The archives run to hundreds of megabytes, so entries are read by
//! seeking rather than by pulling the file into memory.

use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// How much of the file to read to cover the index. The Czech archive's
/// index is 818 KiB; two megabytes leaves room without loading the audio.
const INDEX_PROBE: usize = 4 * 1024 * 1024;
/// Longest path worth believing inside the index.
const MAX_PATH_LEN: u32 = 512;
/// Largest single recording worth believing.
const MAX_ENTRY_BYTES: u32 = 64 * 1024 * 1024;
/// Entry count beyond which the header is not an index.
const MAX_ENTRIES: u32 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechError {
    TooSmall,
    /// The count at the head is not plausible.
    ImplausibleCount(u32),
    /// The index ran out before it listed every entry it promised.
    TruncatedIndex { read: usize, promised: u32 },
}

impl fmt::Display for SpeechError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall => write!(f, "speech archive is too short to hold an index"),
            Self::ImplausibleCount(n) => write!(f, "speech archive claims {n} entries"),
            Self::TruncatedIndex { read, promised } => {
                write!(f, "speech index listed {read} of {promised} entries")
            }
        }
    }
}

impl std::error::Error for SpeechError {}

/// One file inside the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechEntry {
    /// Path as stored, e.g. `loca/czech/speech/angelina/angelina0_00295.ogg`.
    pub path: String,
    pub offset: u64,
    pub size: u32,
}

impl SpeechEntry {
    /// The subtitle label this file belongs to: the file name with its
    /// extension removed.
    pub fn label(&self) -> &str {
        let name = self
            .path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&self.path);
        name.rsplit_once('.').map_or(name, |(stem, _)| stem)
    }

    /// Folder the line was filed under, which is the speaker.
    pub fn speaker(&self) -> Option<&str> {
        let mut parts = self.path.rsplit(['/', '\\']);
        parts.next()?;
        parts.next()
    }

    /// Lowercase extension without the dot.
    pub fn extension(&self) -> String {
        self.path
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default()
    }

    pub fn is_audio(&self) -> bool {
        self.extension() == "ogg"
    }

    pub fn is_lipsync(&self) -> bool {
        self.extension() == "lipsync"
    }
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Parse the index at the head of an archive.
pub fn read_index(head: &[u8]) -> Result<Vec<SpeechEntry>, SpeechError> {
    let count = u32_at(head, 0).ok_or(SpeechError::TooSmall)?;
    if count == 0 || count > MAX_ENTRIES {
        return Err(SpeechError::ImplausibleCount(count));
    }

    let mut entries = Vec::with_capacity(count as usize);
    let mut pos = 4usize;
    for _ in 0..count {
        let Some(len) = u32_at(head, pos) else { break };
        if len == 0 || len > MAX_PATH_LEN {
            break;
        }
        let Some(bytes) = head.get(pos + 4..pos + 4 + len as usize) else {
            break;
        };
        let Ok(path) = std::str::from_utf8(bytes) else {
            break;
        };
        let after = pos + 4 + len as usize;
        let (Some(offset), Some(size)) = (u32_at(head, after), u32_at(head, after + 4)) else {
            break;
        };
        if size > MAX_ENTRY_BYTES {
            break;
        }
        entries.push(SpeechEntry {
            path: path.to_string(),
            offset: offset as u64,
            size,
        });
        pos = after + 8;
    }

    if entries.len() != count as usize {
        return Err(SpeechError::TruncatedIndex {
            read: entries.len(),
            promised: count,
        });
    }
    Ok(entries)
}

/// Open an archive and read its index.
pub fn open_index(path: &Path) -> Result<Vec<SpeechEntry>, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let mut head = vec![0u8; INDEX_PROBE];
    let read = file.read(&mut head)?;
    head.truncate(read);
    Ok(read_index(&head)?)
}

/// What one extraction run produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeechStats {
    pub audio: usize,
    pub lipsync: usize,
    pub bytes: u64,
    pub skipped: usize,
}

/// Extract every entry into `out_dir`, laid out as
/// `<prefix>/<speaker>/<label>.<ext>`.
///
/// Keeping the speaker folder is deliberate: a few thousand loose files
/// named after line ids are unusable, and the speaker is the one grouping
/// the archive already knows.
pub fn extract(
    archive: &Path,
    entries: &[SpeechEntry],
    out_dir: &Path,
    prefix: &str,
) -> Result<SpeechStats, std::io::Error> {
    let mut file = File::open(archive)?;
    let mut stats = SpeechStats::default();
    let mut buffer = Vec::new();

    for entry in entries {
        let speaker = entry.speaker().unwrap_or("other");
        let name = entry
            .path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&entry.path);
        let relative = format!("{prefix}/{speaker}/{name}");
        let target = out_dir.join(&relative);

        if file.seek(SeekFrom::Start(entry.offset)).is_err() {
            stats.skipped += 1;
            continue;
        }
        buffer.clear();
        buffer.resize(entry.size as usize, 0);
        if file.read_exact(&mut buffer).is_err() {
            stats.skipped += 1;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, &buffer)?;

        stats.bytes += entry.size as u64;
        if entry.is_audio() {
            stats.audio += 1;
        } else if entry.is_lipsync() {
            stats.lipsync += 1;
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an archive the way the retail ones are laid out.
    fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
        let index_len: usize = 4 + files
            .iter()
            .map(|(path, _)| 4 + path.len() + 8)
            .sum::<usize>();

        let mut index = Vec::new();
        let mut body = Vec::new();
        index.extend_from_slice(&(files.len() as u32).to_le_bytes());
        for (path, data) in files {
            let offset = index_len + body.len();
            index.extend_from_slice(&(path.len() as u32).to_le_bytes());
            index.extend_from_slice(path.as_bytes());
            index.extend_from_slice(&(offset as u32).to_le_bytes());
            index.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
        }
        assert_eq!(index.len(), index_len, "test builder disagrees with itself");
        index.extend_from_slice(&body);
        index
    }

    fn sample() -> Vec<u8> {
        build(&[
            (
                "loca/czech/speech/angelina/angelina0_00295.lipsync",
                b"lip-data",
            ),
            (
                "loca/czech/speech/angelina/angelina0_00295.ogg",
                b"OggS-audio-bytes",
            ),
            ("loca/czech/speech/darren/darren0_00010.ogg", b"OggS-more"),
        ])
    }

    #[test]
    fn the_index_lists_every_entry() {
        let entries = read_index(&sample()).expect("index");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].size, 16);
    }

    #[test]
    fn offsets_point_at_the_right_bytes() {
        let archive = sample();
        let entries = read_index(&archive).expect("index");
        let entry = &entries[1];
        let start = entry.offset as usize;
        assert_eq!(
            &archive[start..start + entry.size as usize],
            b"OggS-audio-bytes"
        );
    }

    #[test]
    fn the_label_matches_the_subtitle_key() {
        // This is the whole reason the archive is worth importing.
        let entries = read_index(&sample()).expect("index");
        assert_eq!(entries[0].label(), "angelina0_00295");
        assert_eq!(entries[1].label(), "angelina0_00295");
        assert_eq!(entries[2].label(), "darren0_00010");
    }

    #[test]
    fn the_speaker_comes_from_the_folder() {
        let entries = read_index(&sample()).expect("index");
        assert_eq!(entries[0].speaker(), Some("angelina"));
        assert_eq!(entries[2].speaker(), Some("darren"));
    }

    #[test]
    fn audio_and_lipsync_are_told_apart() {
        let entries = read_index(&sample()).expect("index");
        assert!(entries[0].is_lipsync() && !entries[0].is_audio());
        assert!(entries[1].is_audio() && !entries[1].is_lipsync());
    }

    #[test]
    fn a_path_without_a_folder_has_no_speaker() {
        let entry = SpeechEntry {
            path: "loose.ogg".into(),
            offset: 0,
            size: 0,
        };
        assert_eq!(entry.speaker(), None);
        assert_eq!(entry.label(), "loose");
    }

    #[test]
    fn a_name_without_an_extension_is_its_own_label() {
        let entry = SpeechEntry {
            path: "speech/bob/plain".into(),
            offset: 0,
            size: 0,
        };
        assert_eq!(entry.label(), "plain");
        assert_eq!(entry.extension(), "");
    }

    #[test]
    fn an_empty_or_absurd_count_is_rejected() {
        assert!(matches!(read_index(&[]), Err(SpeechError::TooSmall)));
        assert_eq!(
            read_index(&0u32.to_le_bytes()),
            Err(SpeechError::ImplausibleCount(0))
        );
        assert_eq!(
            read_index(&u32::MAX.to_le_bytes()),
            Err(SpeechError::ImplausibleCount(u32::MAX))
        );
    }

    #[test]
    fn an_index_that_stops_early_is_reported_not_used_partially() {
        // Half an index would extract half the voice track and look like
        // a success.
        let mut archive = sample();
        archive.truncate(40);
        assert!(matches!(
            read_index(&archive),
            Err(SpeechError::TruncatedIndex { .. })
        ));
    }

    #[test]
    fn extraction_writes_files_grouped_by_speaker() {
        let dir = std::env::temp_dir().join("infengine_speech_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        let archive_path = dir.join("speech.dat");
        fs::write(&archive_path, sample()).expect("write archive");

        let entries = read_index(&sample()).expect("index");
        let stats = extract(&archive_path, &entries, &dir, "voice/cs").expect("extract");
        assert_eq!(stats.audio, 2);
        assert_eq!(stats.lipsync, 1);
        assert_eq!(stats.skipped, 0);

        let ogg = dir.join("voice/cs/angelina/angelina0_00295.ogg");
        assert_eq!(fs::read(&ogg).expect("read"), b"OggS-audio-bytes");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entry_pointing_past_the_file_is_skipped_not_fatal() {
        let dir = std::env::temp_dir().join("infengine_speech_bad_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        let archive_path = dir.join("speech.dat");
        fs::write(&archive_path, sample()).expect("write archive");

        let mut entries = read_index(&sample()).expect("index");
        entries.push(SpeechEntry {
            path: "speech/ghost/missing.ogg".into(),
            offset: 9_999_999,
            size: 128,
        });
        let stats = extract(&archive_path, &entries, &dir, "voice/cs").expect("extract");
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.audio, 2, "the good entries still came out");

        let _ = fs::remove_dir_all(&dir);
    }
}
