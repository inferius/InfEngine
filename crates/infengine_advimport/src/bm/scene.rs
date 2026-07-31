//! A4.1: the Black Mirror 2/3 scene graph.
//!
//! A scene `.dat` opens with its own name, then an *atom table* naming
//! every field and node type the file uses, then the scene graph itself,
//! then embedded resources. The layout was worked out from a retail
//! install:
//!
//! ```text
//!   [u32 len][scene name]
//!   repeated: [u32 key][u32 len][name]      until key == 0
//!   ... graph ...
//! ```
//!
//! The subtlety that makes this readable at all: the key stored *before*
//! a name is that name's key. Reading it as a trailing field instead
//! shifts every lookup by one entry and yields a graph where `label`
//! holds a filename and `file` holds a number - plausible enough to
//! waste an afternoon on.
//!
//! In the graph, an attribute is `[u32 key][u32 type][value]` with type
//! 2 for `f32` and type 3 for a length-prefixed string. Node structure
//! uses other type codes whose meaning is not yet pinned down, so the
//! scanner does not pretend to parse the tree: it walks byte by byte,
//! recognises attributes and node-type atoms, and attaches attributes to
//! the node most recently opened. Byte-granular scanning matters because
//! strings are followed by a terminator and padding that breaks 4-byte
//! alignment.

use std::collections::HashMap;
use std::fmt;

use glam::Vec2;

/// Type code for a 32-bit float attribute.
const TYPE_F32: u32 = 2;
/// Type code for a length-prefixed string attribute.
const TYPE_STR: u32 = 3;
/// Longest plausible atom name; anything longer means the table ended.
const MAX_ATOM_LEN: u32 = 64;
/// Longest plausible string value.
const MAX_STRING_LEN: u32 = 256;

/// Node-type atoms that open a new element.
const NODE_KINDS: &[&str] = &[
    "Project",
    "Layer",
    "HotSpot",
    "Light",
    "Locator",
    "NavMesh",
    "NavMeshes",
    "Rain",
    "SpecialFX",
    "Objects3D",
    "HotSpots",
    "Lights",
    "Locators",
    "BackgroundElements",
    "StageElements",
    "ForegroundElements",
    "HeavenElements",
    "FrontFogElements",
];

/// Atom that opens a vertex of the current element's outline.
const POINT_KIND: &str = "Point";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneError {
    /// Too short to hold a scene name.
    TooSmall,
    /// The leading length field does not describe a readable name.
    BadSceneName,
    /// The atom table held no entries.
    EmptyAtomTable,
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall => write!(f, "scene file is too short to hold a name"),
            Self::BadSceneName => write!(f, "scene file does not start with a readable name"),
            Self::EmptyAtomTable => write!(f, "scene file has an empty atom table"),
        }
    }
}

impl std::error::Error for SceneError {}

/// An attribute value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f32),
    Text(String),
}

impl Value {
    pub fn as_number(&self) -> Option<f32> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Text(_) => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            Self::Number(_) => None,
        }
    }
}

/// One node of the scene graph.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    /// Node-type atom that opened it, e.g. `Layer` or `HotSpot`.
    pub kind: String,
    pub attrs: Vec<(String, Value)>,
    /// Outline vertices, for nodes built from `Point` children.
    pub points: Vec<Vec2>,
}

impl Element {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }

    pub fn number(&self, name: &str) -> Option<f32> {
        self.get(name)?.as_number()
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        self.get(name)?.as_text()
    }

    /// Display label, falling back to the node kind.
    pub fn label(&self) -> &str {
        self.text("label").unwrap_or(&self.kind)
    }

    fn set(&mut self, name: &str, value: Value) {
        match self.attrs.iter_mut().find(|(k, _)| k == name) {
            // A repeated key means a second node opened without a
            // node-type atom we recognise; keep the first, which belongs
            // to the element we actually identified.
            Some(_) => {}
            None => self.attrs.push((name.to_string(), value)),
        }
    }
}

/// A parsed scene.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub name: String,
    pub elements: Vec<Element>,
    /// Byte offset where attribute scanning stopped. Everything after
    /// this is the resource section, so the texture reader starts there
    /// instead of re-scanning the graph.
    pub graph_end: usize,
}

impl Scene {
    pub fn of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Element> {
        self.elements.iter().filter(move |e| e.kind == kind)
    }

    pub fn first_of_kind<'a>(&'a self, kind: &'a str) -> Option<&'a Element> {
        self.of_kind(kind).next()
    }
}

fn read_u32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_string(data: &[u8], at: usize, len: usize) -> Option<&str> {
    let bytes = data.get(at..at + len)?;
    if !bytes.iter().all(|&b| (0x20..0x7F).contains(&b)) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

/// The atom table, plus where the graph starts.
#[derive(Debug, Clone)]
struct Atoms {
    names: HashMap<u32, String>,
    graph_start: usize,
}

fn parse_atoms(data: &[u8]) -> Result<(String, Atoms), SceneError> {
    let len = read_u32(data, 0).ok_or(SceneError::TooSmall)? as usize;
    if len == 0 || len > MAX_ATOM_LEN as usize {
        return Err(SceneError::BadSceneName);
    }
    let scene_name = read_string(data, 4, len)
        .ok_or(SceneError::BadSceneName)?
        .to_string();

    let mut names = HashMap::new();
    let mut pos = 4 + len;
    while let Some(key) = read_u32(data, pos) {
        if key == 0 {
            pos += 4;
            break;
        }
        let Some(name_len) = read_u32(data, pos + 4) else {
            break;
        };
        if name_len == 0 || name_len > MAX_ATOM_LEN {
            break;
        }
        let Some(name) = read_string(data, pos + 8, name_len as usize) else {
            break;
        };
        names.insert(key, name.to_string());
        pos += 8 + name_len as usize;
    }

    if names.is_empty() {
        return Err(SceneError::EmptyAtomTable);
    }
    Ok((
        scene_name,
        Atoms {
            names,
            graph_start: pos,
        },
    ))
}

/// Parse a scene `.dat`.
pub fn parse(data: &[u8]) -> Result<Scene, SceneError> {
    let (name, atoms) = parse_atoms(data)?;
    let mut scene = Scene {
        name,
        elements: Vec::new(),
        graph_end: atoms.graph_start,
    };

    // Vertex under construction: `Point` opens one, and the `x`/`y` that
    // follow belong to it rather than to the element itself.
    let mut pending: Option<(Option<f32>, Option<f32>)> = None;
    let mut last_attribute: Option<usize> = None;
    let mut i = atoms.graph_start;

    while i + 12 <= data.len() {
        let Some(key) = read_u32(data, i) else { break };
        let Some(atom) = atoms.names.get(&key) else {
            i += 1;
            continue;
        };
        let Some(type_code) = read_u32(data, i + 4) else {
            break;
        };

        // `Point` is encoded like an attribute - key, type 2, a float
        // payload - rather than as a node, so it has to be recognised
        // before the type dispatch or it is swallowed as a number and
        // every outline collapses to its last vertex.
        if atom == POINT_KIND {
            flush_point(&mut scene, &mut pending);
            pending = Some((None, None));
            i += if type_code == TYPE_F32 { 12 } else { 8 };
            continue;
        }

        if type_code == TYPE_F32
            && let Some(raw) = read_u32(data, i + 8)
        {
            let value = f32::from_bits(raw);
            store(&mut scene, &mut pending, atom, Value::Number(value));
            i += 12;
            last_attribute = Some(i);
            continue;
        }

        if type_code == TYPE_STR
            && let Some(len) = read_u32(data, i + 8)
            && len > 0
            && len <= MAX_STRING_LEN
            && let Some(text) = read_string(data, i + 12, len as usize)
        {
            store(
                &mut scene,
                &mut pending,
                atom,
                Value::Text(text.to_string()),
            );
            i += 12 + len as usize;
            last_attribute = Some(i);
            continue;
        }

        if NODE_KINDS.contains(&atom.as_str()) {
            flush_point(&mut scene, &mut pending);
            scene.elements.push(Element {
                kind: atom.clone(),
                ..Default::default()
            });
            i += 8;
            continue;
        }

        i += 1;
    }

    flush_point(&mut scene, &mut pending);
    scene.graph_end = last_attribute.unwrap_or(atoms.graph_start);
    Ok(scene)
}

fn store(
    scene: &mut Scene,
    pending: &mut Option<(Option<f32>, Option<f32>)>,
    atom: &str,
    value: Value,
) {
    if let Some(point) = pending.as_mut()
        && let Value::Number(n) = value
    {
        match atom {
            "x" => {
                point.0 = Some(n);
                return;
            }
            "y" => {
                point.1 = Some(n);
                return;
            }
            _ => {}
        }
        // Any other attribute means the point list ended.
        flush_point(scene, pending);
        return store(scene, pending, atom, Value::Number(n));
    }
    if let Some(element) = scene.elements.last_mut() {
        element.set(atom, value);
    }
}

fn flush_point(scene: &mut Scene, pending: &mut Option<(Option<f32>, Option<f32>)>) {
    let Some((x, y)) = pending.take() else { return };
    let (Some(x), Some(y)) = (x, y) else { return };
    if let Some(element) = scene.elements.last_mut() {
        element.points.push(Vec2::new(x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a scene file the way the retail ones are laid out.
    struct Builder {
        data: Vec<u8>,
        keys: HashMap<String, u32>,
        next_key: u32,
    }

    impl Builder {
        fn new(scene_name: &str) -> Self {
            let mut data = Vec::new();
            data.extend_from_slice(&(scene_name.len() as u32).to_le_bytes());
            data.extend_from_slice(scene_name.as_bytes());
            Self {
                data,
                keys: HashMap::new(),
                next_key: 0x1000,
            }
        }

        fn atom(mut self, name: &str) -> Self {
            let key = self.next_key;
            self.next_key += 0x20;
            self.keys.insert(name.to_string(), key);
            self.data.extend_from_slice(&key.to_le_bytes());
            self.data.extend_from_slice(&(name.len() as u32).to_le_bytes());
            self.data.extend_from_slice(name.as_bytes());
            self
        }

        fn end_atoms(mut self) -> Self {
            self.data.extend_from_slice(&0u32.to_le_bytes());
            self
        }

        fn key(&self, name: &str) -> u32 {
            *self.keys.get(name).expect("atom was declared")
        }

        /// `Point` is written the way retail files write it: an
        /// attribute-shaped record, not a node.
        fn point(mut self, x: f32, y: f32) -> Self {
            self.data.extend_from_slice(&self.key("Point").to_le_bytes());
            self.data.extend_from_slice(&TYPE_F32.to_le_bytes());
            self.data.extend_from_slice(&0f32.to_bits().to_le_bytes());
            self.number("x", x).number("y", y)
        }

        fn node(mut self, kind: &str) -> Self {
            self.data.extend_from_slice(&self.key(kind).to_le_bytes());
            self.data.extend_from_slice(&9u32.to_le_bytes());
            self
        }

        fn number(mut self, name: &str, value: f32) -> Self {
            self.data.extend_from_slice(&self.key(name).to_le_bytes());
            self.data.extend_from_slice(&TYPE_F32.to_le_bytes());
            self.data.extend_from_slice(&value.to_bits().to_le_bytes());
            self
        }

        fn text(mut self, name: &str, value: &str) -> Self {
            self.data.extend_from_slice(&self.key(name).to_le_bytes());
            self.data.extend_from_slice(&TYPE_STR.to_le_bytes());
            self.data
                .extend_from_slice(&(value.len() as u32).to_le_bytes());
            self.data.extend_from_slice(value.as_bytes());
            // Retail files put a terminator and padding here, which is
            // exactly what breaks 4-byte-aligned scanning.
            self.data.extend_from_slice(&[0, 0x79, 0]);
            self
        }

        fn pad(mut self, n: usize) -> Self {
            self.data.extend_from_slice(&vec![0u8; n]);
            self
        }

        fn build(self) -> Vec<u8> {
            let mut data = self.data;
            // Room for the 12-byte lookahead at the tail.
            data.extend_from_slice(&[0u8; 16]);
            data
        }
    }

    fn sample() -> Vec<u8> {
        Builder::new("LighthouseDay")
            .atom("label")
            .atom("file")
            .atom("x")
            .atom("y")
            .atom("w")
            .atom("h")
            .atom("ysort")
            .atom("Project")
            .atom("Layer")
            .atom("HotSpot")
            .atom("Point")
            .end_atoms()
            .node("Project")
            .text("label", "LighthouseDay")
            .number("w", 1280.0)
            .number("h", 800.0)
            .node("Layer")
            .number("x", 0.0)
            .number("y", 0.0)
            .number("w", 1280.0)
            .number("h", 800.0)
            .text("label", "bg")
            .text("file", "s097_LighthouseDay.tga")
            .node("Layer")
            .number("ysort", 644.0)
            .text("label", "grass1")
            .node("HotSpot")
            .text("label", "LighthouseNote")
            .point(873.0, 458.0)
            .point(900.0, 500.0)
            .pad(3)
            .build()
    }

    #[test]
    fn scene_name_and_atom_table_are_read() {
        let scene = parse(&sample()).expect("parse");
        assert_eq!(scene.name, "LighthouseDay");
    }

    #[test]
    fn nodes_are_recognised_in_order() {
        let scene = parse(&sample()).expect("parse");
        let kinds: Vec<_> = scene.elements.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, vec!["Project", "Layer", "Layer", "HotSpot"]);
    }

    #[test]
    fn project_carries_the_room_size() {
        let scene = parse(&sample()).expect("parse");
        let project = scene.first_of_kind("Project").expect("Project node");
        assert_eq!(project.number("w"), Some(1280.0));
        assert_eq!(project.number("h"), Some(800.0));
    }

    #[test]
    fn a_layer_keeps_its_label_and_file() {
        let scene = parse(&sample()).expect("parse");
        let layer = scene.first_of_kind("Layer").expect("Layer node");
        assert_eq!(layer.text("label"), Some("bg"));
        assert_eq!(layer.text("file"), Some("s097_LighthouseDay.tga"));
        assert_eq!(layer.number("w"), Some(1280.0));
    }

    #[test]
    fn scanning_survives_the_padding_after_a_string() {
        // The second Layer only parses if the scanner resynchronised
        // past the terminator and padding the first one's strings left.
        let scene = parse(&sample()).expect("parse");
        let layers: Vec<_> = scene.of_kind("Layer").collect();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[1].text("label"), Some("grass1"));
        assert_eq!(layers[1].number("ysort"), Some(644.0));
    }

    #[test]
    fn hotspot_points_become_an_outline_not_attributes() {
        let scene = parse(&sample()).expect("parse");
        let hotspot = scene.first_of_kind("HotSpot").expect("HotSpot node");
        assert_eq!(hotspot.text("label"), Some("LighthouseNote"));
        assert_eq!(
            hotspot.points,
            vec![Vec2::new(873.0, 458.0), Vec2::new(900.0, 500.0)]
        );
        // The coordinates belong to the outline, not to the hotspot.
        assert_eq!(hotspot.number("x"), None);
    }

    #[test]
    fn label_falls_back_to_the_node_kind() {
        let mut element = Element {
            kind: "Layer".into(),
            ..Default::default()
        };
        assert_eq!(element.label(), "Layer");
        element.set("label", Value::Text("bg".into()));
        assert_eq!(element.label(), "bg");
    }

    #[test]
    fn a_repeated_attribute_keeps_the_first_value() {
        let mut element = Element::default();
        element.set("x", Value::Number(1.0));
        element.set("x", Value::Number(2.0));
        assert_eq!(element.number("x"), Some(1.0));
    }

    #[test]
    fn value_accessors_do_not_coerce() {
        assert_eq!(Value::Number(1.5).as_number(), Some(1.5));
        assert_eq!(Value::Number(1.5).as_text(), None);
        assert_eq!(Value::Text("a".into()).as_text(), Some("a"));
        assert_eq!(Value::Text("a".into()).as_number(), None);
    }

    #[test]
    fn short_or_nonsense_input_is_rejected() {
        assert_eq!(parse(&[]), Err(SceneError::TooSmall));
        assert_eq!(parse(&[0, 0, 0, 0]), Err(SceneError::BadSceneName));
        // A name that is not printable.
        let mut data = 4u32.to_le_bytes().to_vec();
        data.extend_from_slice(&[0xFF, 0xFE, 0xFD, 0xFC, 0, 0, 0, 0]);
        assert_eq!(parse(&data), Err(SceneError::BadSceneName));
    }

    #[test]
    fn an_empty_atom_table_is_rejected() {
        let data = Builder::new("Scene").end_atoms().build();
        assert_eq!(parse(&data), Err(SceneError::EmptyAtomTable));
    }
}
