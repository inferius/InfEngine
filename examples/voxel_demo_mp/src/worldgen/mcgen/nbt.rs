//! W245.1 - minimal Java-edition NBT reader for structure templates.
//!
//! Parses the gzip-compressed big-endian Named Binary Tag format
//! (`net/minecraft/nbt/*` writers) far enough to load
//! `data/minecraft/structure/**.nbt` templates: compounds, lists,
//! the numeric scalars and the array tags. Not a general-purpose
//! round-tripping library - read-only, allocation-happy, meant to
//! run once per template load and then be dropped.

use std::collections::HashMap;

use flate2::read::GzDecoder;
use std::io::Read;

/// One NBT payload. Numeric tags widen to their natural Rust types.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<Nbt>),
    Compound(HashMap<String, Nbt>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Nbt {
    pub(crate) fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(m) => m.get(key),
            _ => None,
        }
    }

    pub(crate) fn as_i32(&self) -> Option<i32> {
        match self {
            Nbt::Byte(v) => Some(*v as i32),
            Nbt::Short(v) => Some(*v as i32),
            Nbt::Int(v) => Some(*v),
            Nbt::Long(v) => Some(*v as i32),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Nbt::String(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn as_list(&self) -> Option<&[Nbt]> {
        match self {
            Nbt::List(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn as_compound(&self) -> Option<&HashMap<String, Nbt>> {
        match self {
            Nbt::Compound(m) => Some(m),
            _ => None,
        }
    }

    /// `[x, y, z]` from a List<Int> (the structure `size`/`pos` shape).
    pub(crate) fn as_ivec3(&self) -> Option<[i32; 3]> {
        let l = self.as_list()?;
        if l.len() != 3 {
            return None;
        }
        Some([l[0].as_i32()?, l[1].as_i32()?, l[2].as_i32()?])
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.at + n > self.data.len() {
            return Err(format!(
                "nbt: truncated at byte {} (wanted {n} more of {})",
                self.at,
                self.data.len()
            ));
        }
        let s = &self.data[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn string(&mut self) -> Result<String, String> {
        let len = self.i16()? as u16 as usize;
        let bytes = self.take(len)?;
        // Java "modified UTF-8" differs from UTF-8 only for NUL +
        // supplementary chars - none appear in vanilla ids; lossy
        // conversion keeps the reader total.
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn payload(&mut self, tag: u8, depth: u32) -> Result<Nbt, String> {
        if depth > 128 {
            return Err("nbt: nesting depth over 128".into());
        }
        Ok(match tag {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => Nbt::Short(self.i16()?),
            3 => Nbt::Int(self.i32()?),
            4 => Nbt::Long(self.i64()?),
            5 => Nbt::Float(f32::from_bits(self.i32()? as u32)),
            6 => Nbt::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.i32()?.max(0) as usize;
                Nbt::ByteArray(self.take(n)?.iter().map(|b| *b as i8).collect())
            }
            8 => Nbt::String(self.string()?),
            9 => {
                let elem = self.u8()?;
                let n = self.i32()?.max(0) as usize;
                let mut v = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    v.push(self.payload(elem, depth + 1)?);
                }
                Nbt::List(v)
            }
            10 => {
                let mut m = HashMap::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let name = self.string()?;
                    let val = self.payload(t, depth + 1)?;
                    m.insert(name, val);
                }
                Nbt::Compound(m)
            }
            11 => {
                let n = self.i32()?.max(0) as usize;
                let mut v = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    v.push(self.i32()?);
                }
                Nbt::IntArray(v)
            }
            12 => {
                let n = self.i32()?.max(0) as usize;
                let mut v = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    v.push(self.i64()?);
                }
                Nbt::LongArray(v)
            }
            t => return Err(format!("nbt: unknown tag {t} at byte {}", self.at)),
        })
    }
}

/// Parse an UNCOMPRESSED NBT stream: the root is a named compound
/// (the name is discarded - vanilla writes "").
pub(crate) fn parse(data: &[u8]) -> Result<Nbt, String> {
    let mut c = Cursor { data, at: 0 };
    let tag = c.u8()?;
    if tag != 10 {
        return Err(format!("nbt: root tag {tag}, expected compound"));
    }
    let _root_name = c.string()?;
    c.payload(10, 0)
}

/// Parse a gzip-compressed NBT file body (the on-disk `.nbt` shape).
pub(crate) fn parse_gzip(data: &[u8]) -> Result<Nbt, String> {
    let mut out = Vec::new();
    GzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| format!("nbt: gzip decode failed: {e}"))?;
    parse(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-built stream: {"answer": 42i32, "name": "well",
    /// "pos": List<Int>[1,2,3]}.
    #[test]
    fn parses_a_hand_built_compound() {
        let mut d: Vec<u8> = vec![10, 0, 0];
        d.extend([3u8, 0, 6]);
        d.extend(b"answer");
        d.extend(42i32.to_be_bytes());
        d.extend([8u8, 0, 4]);
        d.extend(b"name");
        d.extend([0u8, 4]);
        d.extend(b"well");
        d.extend([9u8, 0, 3]);
        d.extend(b"pos");
        d.push(3);
        d.extend(3i32.to_be_bytes());
        d.extend(1i32.to_be_bytes());
        d.extend(2i32.to_be_bytes());
        d.extend(3i32.to_be_bytes());
        d.push(0);
        let root = parse(&d).expect("parses");
        assert_eq!(root.get("answer").and_then(Nbt::as_i32), Some(42));
        assert_eq!(root.get("name").and_then(Nbt::as_str), Some("well"));
        assert_eq!(root.get("pos").and_then(Nbt::as_ivec3), Some([1, 2, 3]));
    }

    #[test]
    fn truncated_stream_errors_cleanly() {
        let d: Vec<u8> = vec![10, 0, 0, 3, 0, 6];
        assert!(parse(&d).is_err());
    }
}
