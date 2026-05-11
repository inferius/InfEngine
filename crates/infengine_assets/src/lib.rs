//! Asset loading, caching, and typed handles for InfEngine.
//!
//! Phase 3.1 introduces:
//! - typed `AssetHandle<T>`s identifying a cache entry by id;
//! - `AssetServer` rooted at a content directory, with image loading
//!   via the `image` crate;
//! - `ImageAsset` (decoded RGBA8 bytes + dimensions + source path).
//!
//! Hot reload, audio, and a richer asset taxonomy land in later
//! Phase-3 steps.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info, warn};

/// Numeric identifier of an entry inside the `AssetServer`.
///
/// Used internally for cache lookup. Combine with a marker type via
/// [`AssetHandle<T>`] for type-safe public access.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct AssetId(u32);

/// Typed handle to an asset of kind `T`. Cheap to copy.
#[derive(Debug)]
pub struct AssetHandle<T: ?Sized> {
    id: AssetId,
    _marker: PhantomData<fn() -> T>,
}

impl<T: ?Sized> Copy for AssetHandle<T> {}
impl<T: ?Sized> Clone for AssetHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ?Sized> PartialEq for AssetHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl<T: ?Sized> Eq for AssetHandle<T> {}
impl<T: ?Sized> std::hash::Hash for AssetHandle<T> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.id.hash(h);
    }
}

impl<T: ?Sized> AssetHandle<T> {
    pub fn id(self) -> AssetId {
        self.id
    }
}

/// A decoded RGBA8 image asset.
#[derive(Debug, Clone)]
pub struct ImageAsset {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub source: PathBuf,
}

/// Errors raised by asset loading.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("asset path {0} is outside the asset root")]
    OutsideRoot(PathBuf),
    #[error("asset {path} not found: {source}")]
    NotFound {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to decode image {path}: {source}")]
    ImageDecode {
        path: PathBuf,
        source: image::ImageError,
    },
    #[error("failed to set up filesystem watcher: {0}")]
    Watch(#[from] notify::Error),
}

/// Filesystem change event emitted by the hot-reload watcher. Paths are
/// reported relative to the asset root whenever possible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetEvent {
    Modified(PathBuf),
    Removed(PathBuf),
}

/// Live filesystem watcher tied to the asset root.
///
/// The struct holds the underlying `notify` watcher alive; drop it to
/// stop receiving events. Poll `receiver.try_recv()` from the main loop.
pub struct AssetWatcher {
    _watcher: notify::RecommendedWatcher,
    pub receiver: std::sync::mpsc::Receiver<AssetEvent>,
}

impl std::fmt::Debug for AssetWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetWatcher").finish()
    }
}

/// Cache + loader for engine assets, rooted at a single content directory.
///
/// Loads are content-addressed by their **canonical** path relative to
/// the root. Loading the same image twice returns the same handle and
/// does not re-read the file.
#[derive(Debug)]
pub struct AssetServer {
    root: PathBuf,
    inner: Arc<RwLock<Inner>>,
}

#[derive(Debug)]
struct Inner {
    next_id: u32,
    by_path: HashMap<PathBuf, AssetId>,
    images: HashMap<AssetId, Arc<ImageAsset>>,
}

impl AssetServer {
    /// Create a server rooted at `root`. The directory does not need to
    /// exist yet; load failures are surfaced lazily.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        info!(root = %root.display(), "asset server initialized");
        Self {
            root,
            inner: Arc::new(RwLock::new(Inner {
                next_id: 1,
                by_path: HashMap::new(),
                images: HashMap::new(),
            })),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a relative asset path to an absolute path, rejecting
    /// attempts to escape the root via `..`.
    fn resolve(&self, path: &Path) -> Result<PathBuf, AssetError> {
        let normalized = normalize(path);
        if normalized
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(AssetError::OutsideRoot(path.to_path_buf()));
        }
        Ok(self.root.join(normalized))
    }

    /// Load (or fetch from cache) an RGBA8 image asset.
    pub fn load_image(
        &self,
        relative: impl AsRef<Path>,
    ) -> Result<AssetHandle<ImageAsset>, AssetError> {
        let rel = relative.as_ref().to_path_buf();
        let abs = self.resolve(&rel)?;

        if let Some(handle) = self.lookup::<ImageAsset>(&rel) {
            debug!(path = %rel.display(), "asset cache hit");
            return Ok(handle);
        }

        let bytes = std::fs::read(&abs).map_err(|source| AssetError::NotFound {
            path: abs.clone(),
            source,
        })?;
        let img = image::load_from_memory(&bytes).map_err(|source| AssetError::ImageDecode {
            path: abs.clone(),
            source,
        })?;
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        let asset = ImageAsset {
            width,
            height,
            rgba: rgba.into_raw(),
            source: abs.clone(),
        };
        info!(
            path = %rel.display(),
            width,
            height,
            bytes = asset.rgba.len(),
            "loaded image asset"
        );

        let mut inner = self.inner.write();
        let id = AssetId(inner.next_id);
        inner.next_id += 1;
        inner.by_path.insert(rel, id);
        inner.images.insert(id, Arc::new(asset));
        Ok(AssetHandle {
            id,
            _marker: PhantomData,
        })
    }

    /// Get the cached image data for a handle, if present.
    pub fn get_image(&self, handle: AssetHandle<ImageAsset>) -> Option<Arc<ImageAsset>> {
        let inner = self.inner.read();
        inner.images.get(&handle.id).cloned()
    }

    /// Look up an existing handle for `path` in the cache.
    fn lookup<T: ?Sized>(&self, path: &Path) -> Option<AssetHandle<T>> {
        let inner = self.inner.read();
        inner.by_path.get(path).copied().map(|id| AssetHandle {
            id,
            _marker: PhantomData,
        })
    }

    /// Start watching the asset root for filesystem changes. Returns an
    /// [`AssetWatcher`] whose `receiver` yields [`AssetEvent`]s. The
    /// watcher continues running until the returned value is dropped.
    ///
    /// The root must exist on disk at the time of the call.
    pub fn watch(&self) -> Result<AssetWatcher, AssetError> {
        use notify::{EventKind, RecursiveMode, Watcher};

        let root_abs =
            std::fs::canonicalize(&self.root).map_err(|source| AssetError::NotFound {
                path: self.root.clone(),
                source,
            })?;
        let root_for_closure = root_abs.clone();
        let (tx, rx) = std::sync::mpsc::channel::<AssetEvent>();

        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                let Ok(ev) = res else {
                    return;
                };
                let is_modified = matches!(ev.kind, EventKind::Modify(_) | EventKind::Create(_));
                let is_removed = matches!(ev.kind, EventKind::Remove(_));
                if !is_modified && !is_removed {
                    return;
                }
                for path in ev.paths {
                    let rel = path
                        .strip_prefix(&root_for_closure)
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|_| path.clone());
                    let event = if is_modified {
                        AssetEvent::Modified(rel)
                    } else {
                        AssetEvent::Removed(rel)
                    };
                    let _ = tx.send(event);
                }
            })?;
        watcher.watch(&root_abs, RecursiveMode::Recursive)?;
        info!(root = %root_abs.display(), "filesystem watcher started");

        Ok(AssetWatcher {
            _watcher: watcher,
            receiver: rx,
        })
    }

    /// List every file under the asset root recursively. Returned paths
    /// are relative to the root, sorted lexicographically. Directories
    /// are not included. Missing root returns an empty `Vec` rather than
    /// an error.
    pub fn list_files(&self) -> std::io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        if !self.root.exists() {
            return Ok(out);
        }
        walk_dir(&self.root, &self.root, &mut out)?;
        out.sort();
        Ok(out)
    }

    /// Look up the relative path that owns `handle`, if any.
    pub fn handle_path(&self, handle: AssetHandle<ImageAsset>) -> Option<PathBuf> {
        let inner = self.inner.read();
        inner
            .by_path
            .iter()
            .find_map(|(p, id)| (*id == handle.id).then(|| p.clone()))
    }

    /// Replace the cached image at `handle` with a freshly decoded copy.
    /// Used by hot reload (Phase 3.2) - safe to call even when no file
    /// has changed.
    pub fn reload_image(
        &self,
        handle: AssetHandle<ImageAsset>,
    ) -> Result<Arc<ImageAsset>, AssetError> {
        let path = {
            let inner = self.inner.read();
            inner
                .images
                .get(&handle.id)
                .map(|a| a.source.clone())
                .ok_or_else(|| AssetError::OutsideRoot("<unknown handle>".into()))?
        };
        let bytes = std::fs::read(&path).map_err(|source| AssetError::NotFound {
            path: path.clone(),
            source,
        })?;
        let img = image::load_from_memory(&bytes).map_err(|source| AssetError::ImageDecode {
            path: path.clone(),
            source,
        })?;
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        let asset = Arc::new(ImageAsset {
            width,
            height,
            rgba: rgba.into_raw(),
            source: path,
        });
        let mut inner = self.inner.write();
        inner.images.insert(handle.id, asset.clone());
        warn!(id = handle.id.0, width, height, "image asset reloaded");
        Ok(asset)
    }
}

fn walk_dir(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk_dir(root, &path, out)?;
        } else {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            out.push(rel);
        }
    }
    Ok(())
}

/// Strip leading `./` and `/` so the path joins cleanly with `root`.
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => continue,
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_is_copy_and_hashable() {
        let h = AssetHandle::<ImageAsset> {
            id: AssetId(1),
            _marker: PhantomData,
        };
        let h2 = h;
        assert_eq!(h, h2);
        let mut map = HashMap::new();
        map.insert(h, "ok");
        assert_eq!(map.get(&h2), Some(&"ok"));
    }

    #[test]
    fn normalize_strips_leading_components() {
        let n = normalize(Path::new("./foo/bar.png"));
        assert_eq!(n, Path::new("foo").join("bar.png"));
    }
}
