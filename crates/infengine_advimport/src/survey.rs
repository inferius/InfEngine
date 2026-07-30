//! A3.7: reconnaissance on an unknown data directory.
//!
//! Before a backend can be written for a game, someone has to work out
//! what its files are. This module automates the boring first pass:
//! group files by extension, sniff each one's header, measure how random
//! the bytes look, and look for the two structures that give an archive
//! away - a run of ascending 32-bit offsets, and a table of readable
//! names.
//!
//! It answers "is this compressed, is this a container, is it obfuscated"
//! without committing to any format.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// How many leading bytes to keep as a signature.
pub const MAGIC_LEN: usize = 16;
/// How much of a file to read for statistics. Enough to characterise a
/// header without pulling a gigabyte archive into memory.
const PROBE_BYTES: usize = 64 * 1024;
/// Shortest run of printable bytes worth reporting as a string.
const MIN_STRING_LEN: usize = 4;

/// What one file looks like from the outside.
#[derive(Debug, Clone, PartialEq)]
pub struct FileProbe {
    pub path: PathBuf,
    pub len: u64,
    /// First [`MAGIC_LEN`] bytes.
    pub magic: Vec<u8>,
    /// Shannon entropy in bits per byte, 0..=8. Above ~7.5 means
    /// compressed or encrypted; plain text sits near 4.
    pub entropy: f32,
    /// Fraction of the probed bytes that are printable ASCII.
    pub printable_ratio: f32,
    /// A run of ascending u32 values at the start, which is what a file
    /// table looks like. `None` when the head does not read that way.
    pub ascending_u32_run: Option<usize>,
    /// Readable strings near the start, useful for spotting names.
    pub strings: Vec<String>,
}

impl FileProbe {
    /// A one-line verdict, deliberately hedged: these are hints for a
    /// human, not conclusions.
    pub fn verdict(&self) -> &'static str {
        if self.entropy > 7.5 {
            "compressed or encrypted"
        } else if self.ascending_u32_run.is_some_and(|n| n >= 4) && self.printable_ratio > 0.2 {
            "likely archive with a name table"
        } else if self.ascending_u32_run.is_some_and(|n| n >= 8) {
            "likely archive with an offset table"
        } else if self.printable_ratio > 0.8 {
            "text"
        } else {
            "unclassified binary"
        }
    }
}

/// Shannon entropy of a byte slice, in bits per byte.
pub fn entropy(bytes: &[u8]) -> f32 {
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for &b in bytes {
        counts[b as usize] += 1;
    }
    let total = bytes.len() as f32;
    -counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f32 / total;
            p * p.log2()
        })
        .sum::<f32>()
}

/// Fraction of bytes that are printable ASCII or common whitespace.
pub fn printable_ratio(bytes: &[u8]) -> f32 {
    if bytes.is_empty() {
        return 0.0;
    }
    let n = bytes
        .iter()
        .filter(|&&b| (0x20..0x7F).contains(&b) || b == b'\n' || b == b'\r' || b == b'\t')
        .count();
    n as f32 / bytes.len() as f32
}

/// Length of the run of strictly ascending little-endian u32s starting
/// at byte 0, or `None` if the first pair already descends.
///
/// An archive directory almost always opens with ascending offsets, so
/// this is the cheapest possible container detector.
pub fn ascending_u32_run(bytes: &[u8]) -> Option<usize> {
    let words: Vec<u32> = bytes
        .chunks_exact(4)
        .take(64)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if words.len() < 2 || words[1] <= words[0] {
        return None;
    }
    let mut run = 1;
    while run < words.len() && words[run] > words[run - 1] {
        run += 1;
    }
    Some(run)
}

/// Printable runs of at least [`MIN_STRING_LEN`] characters.
pub fn ascii_strings(bytes: &[u8], limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for &b in bytes {
        if (0x20..0x7F).contains(&b) {
            current.push(b as char);
        } else {
            if current.len() >= MIN_STRING_LEN {
                out.push(std::mem::take(&mut current));
                if out.len() >= limit {
                    return out;
                }
            }
            current.clear();
        }
    }
    if current.len() >= MIN_STRING_LEN && out.len() < limit {
        out.push(current);
    }
    out
}

/// Probe a single file.
pub fn probe(path: &Path) -> std::io::Result<FileProbe> {
    let meta = fs::metadata(path)?;
    let all = fs::read(path)?;
    let head = &all[..all.len().min(PROBE_BYTES)];
    Ok(FileProbe {
        path: path.to_path_buf(),
        len: meta.len(),
        magic: head[..head.len().min(MAGIC_LEN)].to_vec(),
        entropy: entropy(head),
        printable_ratio: printable_ratio(head),
        ascending_u32_run: ascending_u32_run(head),
        strings: ascii_strings(head, 8),
    })
}

/// Per-extension rollup of a directory tree.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtensionGroup {
    pub count: usize,
    pub total_bytes: u64,
    /// One representative file, probed.
    pub sample: Option<FileProbe>,
}

/// What a whole directory looks like.
#[derive(Debug, Clone, Default)]
pub struct SurveyReport {
    pub root: PathBuf,
    pub groups: BTreeMap<String, ExtensionGroup>,
    pub files_seen: usize,
}

impl fmt::Display for SurveyReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} ({} files)", self.root.display(), self.files_seen)?;
        let mut rows: Vec<_> = self.groups.iter().collect();
        rows.sort_by_key(|(_, group)| std::cmp::Reverse(group.total_bytes));
        for (ext, group) in rows {
            let mb = group.total_bytes as f64 / (1024.0 * 1024.0);
            writeln!(
                f,
                "\n  .{ext:<10} {:>5} files  {mb:>10.1} MB",
                group.count
            )?;
            let Some(sample) = &group.sample else {
                continue;
            };
            writeln!(f, "    sample   {}", sample.path.display())?;
            writeln!(
                f,
                "    entropy  {:.2} bits/byte   printable {:.0}%   -> {}",
                sample.entropy,
                sample.printable_ratio * 100.0,
                sample.verdict()
            )?;
            let hex: Vec<String> = sample.magic.iter().map(|b| format!("{b:02X}")).collect();
            writeln!(f, "    magic    {}", hex.join(" "))?;
            if let Some(run) = sample.ascending_u32_run {
                writeln!(f, "    offsets  {run} ascending u32 at the head")?;
            }
            if !sample.strings.is_empty() {
                writeln!(f, "    strings  {}", sample.strings.join(" | "))?;
            }
        }
        Ok(())
    }
}

/// Walk `root` recursively and summarise it by extension.
pub fn survey_dir(root: &Path) -> std::io::Result<SurveyReport> {
    let mut report = SurveyReport {
        root: root.to_path_buf(),
        ..Default::default()
    };
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            report.files_seen += 1;
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_else(|| "(none)".into());
            let group = report.groups.entry(ext).or_default();
            group.count += 1;
            group.total_bytes += meta.len();
            if group.sample.is_none() {
                group.sample = probe(&path).ok();
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entropy_of_a_constant_buffer_is_zero() {
        assert_eq!(entropy(&[7u8; 1000]), 0.0);
    }

    #[test]
    fn entropy_of_every_byte_once_is_eight_bits() {
        let all: Vec<u8> = (0..=255).collect();
        assert!((entropy(&all) - 8.0).abs() < 1e-3);
    }

    #[test]
    fn entropy_of_two_equally_common_bytes_is_one_bit() {
        let mut b = vec![0u8; 500];
        b.extend(vec![1u8; 500]);
        assert!((entropy(&b) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn entropy_of_nothing_is_zero() {
        assert_eq!(entropy(&[]), 0.0);
    }

    #[test]
    fn printable_ratio_separates_text_from_binary() {
        assert!((printable_ratio(b"hello world") - 1.0).abs() < 1e-6);
        assert_eq!(printable_ratio(&[0u8; 10]), 0.0);
        assert_eq!(printable_ratio(&[]), 0.0);
    }

    #[test]
    fn ascending_offsets_are_detected() {
        let mut b = Vec::new();
        for v in [10u32, 20, 30, 40, 50] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(ascending_u32_run(&b), Some(5));
    }

    #[test]
    fn a_descending_head_is_not_an_offset_table() {
        let mut b = Vec::new();
        for v in [50u32, 40, 30] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(ascending_u32_run(&b), None);
    }

    #[test]
    fn the_run_stops_where_the_ascent_stops() {
        let mut b = Vec::new();
        for v in [1u32, 2, 3, 2, 9] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(ascending_u32_run(&b), Some(3));
    }

    #[test]
    fn too_short_to_hold_two_words_is_none() {
        assert_eq!(ascending_u32_run(&[1, 2, 3, 4]), None);
        assert_eq!(ascending_u32_run(&[]), None);
    }

    #[test]
    fn ascii_strings_finds_names_between_binary() {
        let data = b"\x00\x01ALCHEMY.CPS\x00\xff\xfeROOM.DAT\x00";
        let found = ascii_strings(data, 8);
        assert_eq!(found, vec!["ALCHEMY.CPS", "ROOM.DAT"]);
    }

    #[test]
    fn ascii_strings_ignores_short_noise_and_honours_the_limit() {
        let data = b"ab\x00cd\x00LONGENOUGH\x00ALSOLONG\x00";
        assert_eq!(ascii_strings(data, 8), vec!["LONGENOUGH", "ALSOLONG"]);
        assert_eq!(ascii_strings(data, 1), vec!["LONGENOUGH"]);
    }

    #[test]
    fn ascii_strings_keeps_a_run_that_reaches_the_end() {
        assert_eq!(ascii_strings(b"\x00TRAILING", 4), vec!["TRAILING"]);
    }

    #[test]
    fn verdict_calls_high_entropy_compressed() {
        let probe = FileProbe {
            path: PathBuf::new(),
            len: 0,
            magic: Vec::new(),
            entropy: 7.9,
            printable_ratio: 0.05,
            ascending_u32_run: None,
            strings: Vec::new(),
        };
        assert_eq!(probe.verdict(), "compressed or encrypted");
    }

    #[test]
    fn verdict_calls_an_offset_table_plus_names_an_archive() {
        let probe = FileProbe {
            path: PathBuf::new(),
            len: 0,
            magic: Vec::new(),
            entropy: 4.0,
            printable_ratio: 0.5,
            ascending_u32_run: Some(6),
            strings: vec!["ALCHEMY.CPS".into()],
        };
        assert_eq!(probe.verdict(), "likely archive with a name table");
    }

    #[test]
    fn verdict_calls_mostly_printable_text() {
        let probe = FileProbe {
            path: PathBuf::new(),
            len: 0,
            magic: Vec::new(),
            entropy: 4.5,
            printable_ratio: 0.95,
            ascending_u32_run: None,
            strings: Vec::new(),
        };
        assert_eq!(probe.verdict(), "text");
    }
}
