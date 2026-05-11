//! glTF 2.0 mesh loader.
//!
//! Reads a `.gltf` or `.glb` file and produces a list of primitives
//! ready to feed into `Renderer::create_mesh`. Each primitive carries
//! its own vertex + index buffer; if the glTF mesh contains multiple
//! primitives (e.g. different materials), each becomes a separate
//! `LoadedPrimitive` so the caller can choose how to register them.
//!
//! Materials are not yet honoured - the caller decides albedo / PBR
//! parameters when drawing. A future phase will surface the material
//! table so the editor can map glTF materials onto block / mesh slots.

use std::path::{Path, PathBuf};

use crate::MeshVertex;

#[derive(Debug, Clone)]
pub struct LoadedPrimitive {
    pub name: String,
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum GltfError {
    #[error("failed to read glTF '{path}': {source}")]
    Io { path: PathBuf, source: gltf::Error },
    #[error("glTF primitive missing positions")]
    NoPositions,
}

/// Load every primitive in every mesh in `path`. Returns a flat list -
/// mesh hierarchy / scene-graph is dropped at this layer; the caller
/// reconstructs it from `LoadedPrimitive::name` if needed.
pub fn load_gltf(path: impl AsRef<Path>) -> Result<Vec<LoadedPrimitive>, GltfError> {
    let path = path.as_ref();
    let (document, buffers, _images) = gltf::import(path).map_err(|source| GltfError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut out = Vec::new();
    for mesh in document.meshes() {
        let mesh_name = mesh.name().unwrap_or("mesh").to_string();
        for (i, prim) in mesh.primitives().enumerate() {
            let reader = prim.reader(|buffer| Some(&buffers[buffer.index()]));
            let positions: Vec<[f32; 3]> = match reader.read_positions() {
                Some(iter) => iter.collect(),
                None => return Err(GltfError::NoPositions),
            };
            let normals: Vec<[f32; 3]> = match reader.read_normals() {
                Some(iter) => iter.collect(),
                None => vec![[0.0, 1.0, 0.0]; positions.len()],
            };
            let uvs: Vec<[f32; 2]> = match reader.read_tex_coords(0) {
                Some(reader) => reader.into_f32().collect(),
                None => vec![[0.0, 0.0]; positions.len()],
            };
            let indices: Vec<u32> = match reader.read_indices() {
                Some(reader) => reader.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            let mut vertices = Vec::with_capacity(positions.len());
            for (vi, pos) in positions.iter().enumerate() {
                let normal = normals.get(vi).copied().unwrap_or([0.0, 1.0, 0.0]);
                let uv = uvs.get(vi).copied().unwrap_or([0.0, 0.0]);
                vertices.push(MeshVertex::new(*pos, normal).with_uv(uv));
            }
            out.push(LoadedPrimitive {
                name: format!("{mesh_name}#{i}"),
                vertices,
                indices,
            });
        }
    }
    Ok(out)
}
