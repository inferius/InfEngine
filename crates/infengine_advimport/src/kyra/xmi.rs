//! A15.1: XMIDI, and turning it into a MIDI file anything can play.
//!
//! Kyrandia's music is XMIDI - Miles Sound System's variant, wrapped in
//! IFF:
//!
//! ```text
//!   FORM XDIR
//!     INFO      how many sequences follow
//!   CAT  XMID
//!     FORM XMID
//!       TIMB    instrument bank, not needed for a MIDI file
//!       EVNT    the events
//! ```
//!
//! Two things inside `EVNT` differ from a standard MIDI track, and both
//! will silently produce a broken file if missed.
//!
//! Delta times are **not** variable-length quantities. They are a run of
//! bytes below 0x80, each contributing its full value, added together
//! until a status byte appears. Reading them as VLQs makes every long
//! gap far too short.
//!
//! A note-on carries a **duration** after its velocity, and there is no
//! matching note-off in the stream. The converter has to hold the
//! pending note-offs and emit each one at the right time, or the music
//! plays as one endless chord.
//!
//! XMIDI runs at a fixed 120 ticks per second, so the output uses 60
//! ticks per quarter note at 120 beats per minute - one output tick per
//! input tick, with no rounding anywhere.

use std::fmt;

/// Ticks per quarter note in the file we write.
const DIVISION: u16 = 60;
/// Microseconds per quarter note, giving 120 ticks per second.
const TEMPO_US: u32 = 500_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmiError {
    /// Not an IFF file.
    NotIff,
    /// No `EVNT` chunk anywhere in the file.
    NoEvents,
    /// An event ran past the end of the chunk.
    Truncated { at: usize },
}

impl fmt::Display for XmiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotIff => write!(f, "XMI does not start with an IFF FORM"),
            Self::NoEvents => write!(f, "XMI holds no EVNT chunk"),
            Self::Truncated { at } => write!(f, "XMI event at byte {at} runs past the chunk"),
        }
    }
}

impl std::error::Error for XmiError {}

/// One event on the way out, already placed in absolute time.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Timed {
    time: u32,
    /// Note-offs generated from a note's duration must land before any
    /// note-on at the same tick, or a repeated note is cut short by its
    /// own predecessor.
    is_note_off: bool,
    bytes: Vec<u8>,
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Find every `EVNT` chunk, walking IFF containers.
///
/// Ranges rather than positions: a container's children have to be
/// scanned within its own bounds. Descending without an end offset lets
/// the inner walk run on into the container's siblings and report the
/// same chunk twice.
///
/// Results come back in file order. The walk itself is a stack, so it
/// visits containers back to front; sorting by offset at the end keeps
/// sequence 0 as the first one in the file, which is the one a person
/// looking for the main theme will reach for.
fn event_chunks(data: &[u8]) -> Vec<&[u8]> {
    let mut found: Vec<(usize, &[u8])> = Vec::new();
    let mut ranges = vec![(0usize, data.len())];
    while let Some((start, end)) = ranges.pop() {
        let mut pos = start;
        while pos + 8 <= end {
            let Some(id) = data.get(pos..pos + 4) else {
                break;
            };
            let Some(size) = be32(data, pos + 4) else {
                break;
            };
            let size = size as usize;
            let body = pos + 8;
            let body_end = (body + size).min(end);
            match id {
                // Containers carry a four-character type, then children.
                b"FORM" | b"CAT " | b"LIST" => {
                    if body + 4 <= body_end {
                        ranges.push((body + 4, body_end));
                    }
                }
                b"EVNT" => {
                    if let Some(chunk) = data.get(body..body_end) {
                        found.push((pos, chunk));
                    }
                }
                _ => {}
            }
            pos = body + size + (size & 1);
        }
    }
    found.sort_by_key(|(offset, _)| *offset);
    found.into_iter().map(|(_, chunk)| chunk).collect()
}

/// Read a variable-length quantity, the MIDI kind.
fn read_vlq(data: &[u8], pos: &mut usize) -> Option<u32> {
    let mut value = 0u32;
    for _ in 0..4 {
        let byte = *data.get(*pos)?;
        *pos += 1;
        value = (value << 7) | (byte & 0x7F) as u32;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    Some(value)
}

/// Write a variable-length quantity.
fn write_vlq(out: &mut Vec<u8>, mut value: u32) {
    let mut buffer = [0u8; 4];
    let mut len = 0;
    buffer[len] = (value & 0x7F) as u8;
    len += 1;
    value >>= 7;
    while value > 0 && len < 4 {
        buffer[len] = ((value & 0x7F) as u8) | 0x80;
        len += 1;
        value >>= 7;
    }
    for i in (0..len).rev() {
        out.push(buffer[i]);
    }
}

/// Convert one `EVNT` chunk into absolute-time MIDI events.
fn convert_events(chunk: &[u8]) -> Result<Vec<Timed>, XmiError> {
    let mut out: Vec<Timed> = Vec::new();
    let mut now = 0u32;
    let mut pos = 0usize;

    while pos < chunk.len() {
        let at = pos;

        // Delta: a run of bytes below 0x80, each adding its own value.
        // These are not variable-length quantities.
        while let Some(&byte) = chunk.get(pos) {
            if byte >= 0x80 {
                break;
            }
            now += byte as u32;
            pos += 1;
        }

        let Some(&status) = chunk.get(pos) else { break };
        pos += 1;

        match status {
            // Meta event.
            0xFF => {
                let Some(&kind) = chunk.get(pos) else {
                    return Err(XmiError::Truncated { at });
                };
                pos += 1;
                let Some(len) = read_vlq(chunk, &mut pos) else {
                    return Err(XmiError::Truncated { at });
                };
                let Some(body) = chunk.get(pos..pos + len as usize) else {
                    return Err(XmiError::Truncated { at });
                };
                pos += len as usize;
                // End of track is written by the caller.
                if kind == 0x2F {
                    break;
                }
                let mut bytes = vec![0xFF, kind];
                write_vlq(&mut bytes, len);
                bytes.extend_from_slice(body);
                out.push(Timed {
                    time: now,
                    is_note_off: false,
                    bytes,
                });
            }
            // System exclusive.
            0xF0 | 0xF7 => {
                let Some(len) = read_vlq(chunk, &mut pos) else {
                    return Err(XmiError::Truncated { at });
                };
                let Some(body) = chunk.get(pos..pos + len as usize) else {
                    return Err(XmiError::Truncated { at });
                };
                pos += len as usize;
                let mut bytes = vec![status];
                write_vlq(&mut bytes, len);
                bytes.extend_from_slice(body);
                out.push(Timed {
                    time: now,
                    is_note_off: false,
                    bytes,
                });
            }
            // Note on, which in XMIDI carries how long the note lasts.
            0x90..=0x9F => {
                let (Some(&note), Some(&velocity)) = (chunk.get(pos), chunk.get(pos + 1)) else {
                    return Err(XmiError::Truncated { at });
                };
                pos += 2;
                let Some(duration) = read_vlq(chunk, &mut pos) else {
                    return Err(XmiError::Truncated { at });
                };
                out.push(Timed {
                    time: now,
                    is_note_off: false,
                    bytes: vec![status, note, velocity],
                });
                out.push(Timed {
                    time: now + duration,
                    is_note_off: true,
                    bytes: vec![status & 0x8F, note, 0x40],
                });
            }
            // Channel messages with one data byte.
            0xC0..=0xDF => {
                let Some(&value) = chunk.get(pos) else {
                    return Err(XmiError::Truncated { at });
                };
                pos += 1;
                out.push(Timed {
                    time: now,
                    is_note_off: false,
                    bytes: vec![status, value],
                });
            }
            // Channel messages with two data bytes.
            0x80..=0xBF | 0xE0..=0xEF => {
                let (Some(&a), Some(&b)) = (chunk.get(pos), chunk.get(pos + 1)) else {
                    return Err(XmiError::Truncated { at });
                };
                pos += 2;
                out.push(Timed {
                    time: now,
                    is_note_off: false,
                    bytes: vec![status, a, b],
                });
            }
            _ => return Err(XmiError::Truncated { at }),
        }
    }

    // Note-offs were appended out of order; sorting puts them back, and
    // the flag keeps an off before an on at the same tick.
    out.sort_by(|a, b| a.time.cmp(&b.time).then(b.is_note_off.cmp(&a.is_note_off)));
    Ok(out)
}

/// Convert an XMI file into standard MIDI files, one per sequence.
pub fn to_midi(data: &[u8]) -> Result<Vec<Vec<u8>>, XmiError> {
    if data.len() < 12 || &data[..4] != b"FORM" {
        return Err(XmiError::NotIff);
    }
    let chunks = event_chunks(data);
    if chunks.is_empty() {
        return Err(XmiError::NoEvents);
    }

    let mut out = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let events = convert_events(chunk)?;

        let mut track = Vec::new();
        // Set the tempo so one input tick is one output tick.
        write_vlq(&mut track, 0);
        track.extend_from_slice(&[0xFF, 0x51, 0x03]);
        track.extend_from_slice(&TEMPO_US.to_be_bytes()[1..]);

        let mut last = 0u32;
        for event in &events {
            write_vlq(&mut track, event.time.saturating_sub(last));
            last = event.time;
            track.extend_from_slice(&event.bytes);
        }
        write_vlq(&mut track, 0);
        track.extend_from_slice(&[0xFF, 0x2F, 0x00]);

        let mut file = Vec::with_capacity(track.len() + 22);
        file.extend_from_slice(b"MThd");
        file.extend_from_slice(&6u32.to_be_bytes());
        file.extend_from_slice(&0u16.to_be_bytes()); // format 0
        file.extend_from_slice(&1u16.to_be_bytes()); // one track
        file.extend_from_slice(&DIVISION.to_be_bytes());
        file.extend_from_slice(b"MTrk");
        file.extend_from_slice(&(track.len() as u32).to_be_bytes());
        file.extend_from_slice(&track);
        out.push(file);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrap event bytes in the IFF nesting a real XMI uses.
    fn build(events: &[u8]) -> Vec<u8> {
        let mut evnt = b"EVNT".to_vec();
        evnt.extend_from_slice(&(events.len() as u32).to_be_bytes());
        evnt.extend_from_slice(events);
        if !events.len().is_multiple_of(2) {
            evnt.push(0);
        }

        let mut xmid = b"XMID".to_vec();
        xmid.extend_from_slice(&evnt);
        let mut form = b"FORM".to_vec();
        form.extend_from_slice(&(xmid.len() as u32).to_be_bytes());
        form.extend_from_slice(&xmid);

        let mut cat = b"CAT ".to_vec();
        let body_len = 4 + form.len();
        cat.extend_from_slice(&(body_len as u32).to_be_bytes());
        cat.extend_from_slice(b"XMID");
        cat.extend_from_slice(&form);

        let mut outer = b"FORM".to_vec();
        let xdir = b"XDIRINFO".to_vec();
        let mut inner = xdir;
        inner.extend_from_slice(&2u32.to_be_bytes());
        inner.extend_from_slice(&1u16.to_be_bytes());
        outer.extend_from_slice(&(inner.len() as u32).to_be_bytes());
        outer.extend_from_slice(&inner);
        outer.extend_from_slice(&cat);
        outer
    }

    /// Pull the track bytes out of a written MIDI file.
    fn track_of(midi: &[u8]) -> &[u8] {
        let at = midi
            .windows(4)
            .position(|w| w == b"MTrk")
            .expect("MTrk present");
        &midi[at + 8..]
    }

    #[test]
    fn a_note_becomes_an_on_and_an_off() {
        // Note on channel 0, note 60, velocity 100, lasting 30 ticks.
        let file = build(&[0x90, 60, 100, 30]);
        let midis = to_midi(&file).expect("convert");
        assert_eq!(midis.len(), 1);
        let track = track_of(&midis[0]);
        // Tempo first, then the note on, then the note off.
        assert!(track.windows(3).any(|w| w == [0x90, 60, 100]));
        assert!(
            track.windows(3).any(|w| w == [0x80, 60, 0x40]),
            "the duration has to become a note off"
        );
    }

    #[test]
    fn delta_bytes_add_up_rather_than_shifting() {
        // Three delta bytes of 100 are 300 ticks, not a VLQ.
        let file = build(&[100, 100, 100, 0x90, 60, 100, 1]);
        let midis = to_midi(&file).expect("convert");
        let track = track_of(&midis[0]);
        // Skip the tempo event: 00 FF 51 03 xx xx xx
        let after_tempo = &track[7..];
        let mut pos = 0;
        let delta = read_vlq(after_tempo, &mut pos).expect("delta");
        assert_eq!(delta, 300, "read as a VLQ this would be far smaller");
    }

    #[test]
    fn note_offs_are_ordered_before_note_ons_at_the_same_tick() {
        // Two notes, the first ending exactly when the second starts.
        // Emitting the on first would let it be cut by the off.
        let file = build(&[0x90, 60, 100, 10, 10, 0x90, 60, 100, 10]);
        let midis = to_midi(&file).expect("convert");
        let track = track_of(&midis[0]);
        let off = track
            .windows(3)
            .position(|w| w == [0x80, 60, 0x40])
            .expect("note off");
        let second_on = track
            .windows(3)
            .enumerate()
            .filter(|(_, w)| *w == [0x90, 60, 100])
            .nth(1)
            .map(|(i, _)| i)
            .expect("second note on");
        assert!(off < second_on, "the off must land first");
    }

    #[test]
    fn the_header_declares_the_timing_the_conversion_assumes() {
        let midis = to_midi(&build(&[0x90, 60, 100, 4])).expect("convert");
        let midi = &midis[0];
        assert_eq!(&midi[..4], b"MThd");
        assert_eq!(u16::from_be_bytes([midi[8], midi[9]]), 0, "format 0");
        assert_eq!(u16::from_be_bytes([midi[10], midi[11]]), 1, "one track");
        assert_eq!(u16::from_be_bytes([midi[12], midi[13]]), DIVISION);
        // Tempo meta event opens the track.
        let track = track_of(midi);
        assert_eq!(&track[..4], &[0x00, 0xFF, 0x51, 0x03]);
    }

    #[test]
    fn the_track_ends_with_an_end_of_track() {
        let midis = to_midi(&build(&[0x90, 60, 100, 4])).expect("convert");
        let track = track_of(&midis[0]);
        assert_eq!(&track[track.len() - 3..], &[0xFF, 0x2F, 0x00]);
    }

    #[test]
    fn program_changes_take_one_data_byte() {
        let file = build(&[0xC0, 42, 0x90, 60, 100, 4]);
        let midis = to_midi(&file).expect("convert");
        let track = track_of(&midis[0]);
        assert!(track.windows(2).any(|w| w == [0xC0, 42]));
        assert!(track.windows(3).any(|w| w == [0x90, 60, 100]));
    }

    #[test]
    fn controllers_take_two() {
        let file = build(&[0xB0, 7, 127, 0x90, 60, 100, 4]);
        let midis = to_midi(&file).expect("convert");
        assert!(track_of(&midis[0]).windows(3).any(|w| w == [0xB0, 7, 127]));
    }

    #[test]
    fn a_meta_event_survives_the_conversion() {
        // A text meta event: FF 01 len "hi"
        let file = build(&[0xFF, 0x01, 0x02, b'h', b'i', 0x90, 60, 100, 4]);
        let midis = to_midi(&file).expect("convert");
        let track = track_of(&midis[0]);
        assert!(track.windows(5).any(|w| w == [0xFF, 0x01, 0x02, b'h', b'i']));
    }

    #[test]
    fn end_of_track_inside_the_events_stops_the_conversion() {
        let file = build(&[0x90, 60, 100, 4, 0xFF, 0x2F, 0x00, 0x90, 72, 100, 4]);
        let midis = to_midi(&file).expect("convert");
        let track = track_of(&midis[0]);
        assert!(
            !track.windows(3).any(|w| w == [0x90, 72, 100]),
            "events after the end marker are not part of the track"
        );
    }

    #[test]
    fn variable_length_quantities_round_trip() {
        for value in [0u32, 1, 127, 128, 8192, 0x0FFF_FFFF] {
            let mut buffer = Vec::new();
            write_vlq(&mut buffer, value);
            let mut pos = 0;
            assert_eq!(read_vlq(&buffer, &mut pos), Some(value), "value {value}");
            assert_eq!(pos, buffer.len());
        }
    }

    #[test]
    fn sequences_come_back_in_file_order() {
        // A bank opens with its main theme and follows it with short
        // cues; a stack-ordered walk would number them backwards.
        let mut cat_body = b"XMID".to_vec();
        for events in [
            &[0x90u8, 60, 100, 4][..],
            &[0x90, 62, 100, 4][..],
            &[0x90, 64, 100, 4][..],
        ] {
            let mut evnt = b"EVNT".to_vec();
            evnt.extend_from_slice(&(events.len() as u32).to_be_bytes());
            evnt.extend_from_slice(events);
            let mut xmid = b"XMID".to_vec();
            xmid.extend_from_slice(&evnt);
            let mut form = b"FORM".to_vec();
            form.extend_from_slice(&(xmid.len() as u32).to_be_bytes());
            form.extend_from_slice(&xmid);
            cat_body.extend_from_slice(&form);
        }
        let mut file = b"CAT ".to_vec();
        file.extend_from_slice(&(cat_body.len() as u32).to_be_bytes());
        file.extend_from_slice(&cat_body);
        let mut outer = b"FORM".to_vec();
        outer.extend_from_slice(&4u32.to_be_bytes());
        outer.extend_from_slice(b"XDIR");
        outer.extend_from_slice(&file);

        let midis = to_midi(&outer).expect("convert");
        assert_eq!(midis.len(), 3);
        for (index, note) in [60u8, 62, 64].iter().enumerate() {
            assert!(
                track_of(&midis[index]).windows(3).any(|w| w == [0x90, *note, 100]),
                "sequence {index} should hold note {note}"
            );
        }
    }

    #[test]
    fn a_file_that_is_not_iff_is_rejected() {
        assert_eq!(to_midi(b"nope"), Err(XmiError::NotIff));
    }

    #[test]
    fn an_iff_file_with_no_events_is_rejected() {
        let mut form = b"FORM".to_vec();
        form.extend_from_slice(&4u32.to_be_bytes());
        form.extend_from_slice(b"XDIR");
        assert_eq!(to_midi(&form), Err(XmiError::NoEvents));
    }

    #[test]
    fn a_truncated_event_is_rejected() {
        let file = build(&[0x90, 60]);
        assert!(matches!(to_midi(&file), Err(XmiError::Truncated { .. })));
    }
}
