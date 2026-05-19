//! Headless server tick-loop coordinator.
//!
//! `HeadlessServer` composes the moving parts the dedicated server
//! needs:
//!
//! - `infengine_voxel::World` for the authoritative voxel grid.
//! - `infengine_net::NetServer` for client connections (UDP, length-
//!   prefixed bincode messages).
//! - The save-layer typed primitives exposed by
//!   `voxel_demo_mp::world_save` for persistence.
//! - An `audit::AuditLog` for the connection / chat / command log.
//!
//! Phase B.7b deliberately stops short of porting mob AI, drop
//! physics, scheduled timers, and the chat / admin command set out
//! of the GUI demo bin - those subsystems are deeply intertwined
//! with `voxel_demo_mp::main::Voxel` and need a dedicated extraction
//! pass. The server hosts the world, accepts clients, drains net
//! events into the audit log, and saves cleanly on shutdown. Phase
//! B.7c (stdin REPL) and the parity follow-up will reach for the
//! deferred subsystems.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use infengine_net::NetServer;
use infengine_voxel::{CHUNK_SIZE, ChunkCoord, World};
use tracing::{info, warn};

use voxel_demo_mp::world_save::{
    chunks::{self, CHUNK_PAYLOAD_VERSION, ChunkPayload, ChunkVoxels},
    layout,
    meta::{self, create_meta},
    world_json::{self, create_world},
};
use voxel_demo_mp::worldgen::{WorldGen, WorldgenCtx, generate_chunk_at_ctx};

use crate::args::Args;
use crate::audit::AuditLog;

pub mod shutdown;
pub mod stdin_repl;
pub mod tick;

/// Number of chunks (in each horizontal direction) generated on
/// initial bootstrap when a brand-new slug is provisioned. Matches
/// the GUI demo's `INITIAL_CHUNKS` so the server-side worldgen output
/// is shape-compatible.
const INITIAL_CHUNKS: i32 = 3;
/// Number of stacked chunk layers along the Y axis at boot.
/// `voxel_demo_mp`'s GUI demo uses 4 - mirror that so the freshly
/// generated terrain has a sane vertical footprint (64 voxels) for
/// the first client joining.
const VERTICAL_CHUNKS: i32 = 4;
/// Single-dimension overworld tag - matches the GUI demo's path.
/// Phase F bumps this to per-dimension sweeps.
const OVERWORLD: &str = "overworld";

/// The composed headless server runtime. Owns the world, the net
/// listener, and the autosave bookkeeping.
pub struct HeadlessServer {
    /// Configuration captured from CLI args at boot time. Held for
    /// the autosave cadence + the world dir override.
    args: Args,
    /// Filesystem slug (canonicalised from `args.world`). All path
    /// helpers below feed this into `layout::*` helpers.
    slug: String,
    /// The TCP / UDP listener. Held in an `Option` so future test
    /// hooks can swap a mock in without restructuring the type.
    net: NetServer,
    /// The authoritative voxel world.
    world: World,
    /// Wall-clock instant of the most recent save. Drives
    /// `autosave_due`.
    last_save: Instant,
    /// Counter for how many ticks we've processed since boot.
    /// Surfaced in periodic info-level log lines so an operator can
    /// confirm the loop is alive.
    tick_count: u64,
    /// Append-only audit log writer. Flushed on every save + on
    /// graceful shutdown.
    audit: AuditLog,
    /// Last-known client ids; the difference set against the live
    /// net roster on each poll tells us who connected / disconnected
    /// since the previous tick (for audit log entries).
    known_clients: std::collections::HashSet<u32>,
}

impl std::fmt::Debug for HeadlessServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessServer")
            .field("slug", &self.slug)
            .field("tick_count", &self.tick_count)
            .field("known_clients", &self.known_clients.len())
            .finish()
    }
}

impl HeadlessServer {
    /// Bootstrap from parsed CLI args:
    ///
    /// 1. Resolve the world directory (`layout::world_dir_in` if
    ///    `--data-dir` is set; the OS data dir otherwise).
    /// 2. Read `meta.json` + `world.json`. Create a fresh slot if
    ///    either is missing.
    /// 3. Pin the global worldgen variant + seed from the loaded
    ///    `world.json`.
    /// 4. Load every persisted chunk under
    ///    `<world_dir>/chunks/overworld/`.
    /// 5. Pre-generate the initial chunk grid (`INITIAL_CHUNKS` x
    ///    `VERTICAL_CHUNKS`) for any coord not already loaded from
    ///    disk - lets the first client join into a populated world.
    /// 6. Bind the `NetServer` on `<bind>:<port>`.
    pub fn bootstrap(args: &Args) -> Result<Self> {
        let slug = layout::slug(&args.world);
        let world_dir = resolve_world_dir(args, &slug);
        std::fs::create_dir_all(&world_dir).with_context(|| {
            format!(
                "creating world directory {}",
                world_dir.display()
            )
        })?;
        info!(slug = %slug, dir = %world_dir.display(), "resolving world slot");

        // 1. meta.json: read existing or mint a fresh one.
        let meta_path = world_dir.join("meta.json");
        let mut meta = if meta_path.exists() {
            meta::read_meta_in_dir_with_registry(
                world_dir.parent().unwrap_or(&world_dir),
                &slug,
                voxel_demo_mp::world_save::migrate::default_registry(),
            )
            .with_context(|| format!("reading {}", meta_path.display()))?
        } else {
            let m = create_meta(&args.world, &slug);
            meta::write_meta_in_dir(
                world_dir.parent().unwrap_or(&world_dir),
                &slug,
                &m,
            )
            .with_context(|| format!("writing fresh {}", meta_path.display()))?;
            info!(slug = %slug, "minted new world slot");
            m
        };
        // Bump last_played + clamp the per-session timer so subsequent
        // autosaves accumulate from now, not the previous owner's
        // session.
        meta.last_played = chrono::Utc::now();

        // 2. world.json: load or create. world.json is immutable
        // post-creation - CLI worldgen / seed overrides are ignored
        // once the slot exists.
        let world_json_path = world_dir.join("world.json");
        let wj = if world_json_path.exists() {
            world_json::read_world_in_dir_with_registry(
                world_dir.parent().unwrap_or(&world_dir),
                &slug,
                voxel_demo_mp::world_save::migrate::default_registry(),
            )
            .with_context(|| format!("reading {}", world_json_path.display()))?
        } else {
            let seed = args.seed.unwrap_or_else(generate_seed);
            let w = create_world(args.worldgen.tag(), seed);
            world_json::write_world_in_dir(
                world_dir.parent().unwrap_or(&world_dir),
                &slug,
                &w,
            )
            .with_context(|| {
                format!("writing fresh {}", world_json_path.display())
            })?;
            info!(
                slug = %slug,
                worldgen = %args.worldgen.tag(),
                seed,
                "minted world.json"
            );
            w
        };

        // 3. Build a `WorldgenCtx` snapshot for the overworld so
        // every `generate_chunk_at_ctx` call below reads the
        // right per-world inputs. W5.1b: no global atomics to
        // pin; the ctx is plumbed through explicitly.
        let wg = WorldGen::from_str(&wj.worldgen).unwrap_or_else(|| {
            warn!(
                worldgen = %wj.worldgen,
                "unknown worldgen tag in world.json, defaulting to classic"
            );
            WorldGen::Classic
        });
        let worldgen_ctx = WorldgenCtx {
            base_seed: wj.seed,
            dim: OVERWORLD.to_string(),
            profile: wg,
        };

        // 4. Load every persisted chunk under chunks/overworld/.
        let mut world = World::new();
        let chunks_dir = world_dir.join("chunks").join(OVERWORLD);
        let mut chunks_loaded: usize = 0;
        if chunks_dir.is_dir() {
            for entry in std::fs::read_dir(&chunks_dir)
                .with_context(|| format!("reading {}", chunks_dir.display()))?
                .flatten()
            {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("bin") {
                    continue;
                }
                let Some(coord) = parse_chunk_filename(&path) else {
                    warn!(path = %path.display(), "unrecognised chunk filename");
                    continue;
                };
                let payload = match chunks::read_chunk_in_dir(
                    world_dir.parent().unwrap_or(&world_dir),
                    &slug,
                    OVERWORLD,
                    coord,
                )? {
                    Some(p) => p,
                    None => continue,
                };
                apply_chunk_payload(&mut world, payload)?;
                chunks_loaded += 1;
            }
        }
        info!(chunks_loaded, "loaded chunks from disk");

        // 5. Pre-generate the initial chunk footprint around origin
        // for any coord that wasn't already loaded.
        let mut chunks_generated: usize = 0;
        for cy in 0..VERTICAL_CHUNKS {
            for cx in -INITIAL_CHUNKS..=INITIAL_CHUNKS {
                for cz in -INITIAL_CHUNKS..=INITIAL_CHUNKS {
                    let coord = [cx, cy, cz];
                    if world.chunk(coord).is_some() {
                        continue;
                    }
                    let chunk = generate_chunk_at_ctx(coord, &worldgen_ctx);
                    world.insert_chunk(coord, chunk);
                    chunks_generated += 1;
                }
            }
        }
        if chunks_generated > 0 {
            info!(chunks_generated, "pre-generated initial chunk footprint");
        }

        // Persist the bumped meta so a kill-9 immediately after boot
        // doesn't lose the timestamp.
        meta::write_meta_in_dir(
            world_dir.parent().unwrap_or(&world_dir),
            &slug,
            &meta,
        )
        .with_context(|| "writing meta.json")?;

        // 6. Bind the net listener.
        let bind_addr = format!("{}:{}", args.bind, args.port);
        let net = NetServer::bind(&bind_addr)
            .with_context(|| format!("binding {}", bind_addr))?;
        info!(addr = %bind_addr, "net server listening");

        let audit_path = world_dir.join("audit.log");
        let audit = AuditLog::open(&audit_path)
            .with_context(|| format!("opening {}", audit_path.display()))?;

        // Move the args struct through but normalise the world slug
        // so subsequent path resolutions don't re-slug on every call.
        let mut owned_args = args.clone();
        owned_args.world = slug.clone();
        Ok(Self {
            args: owned_args,
            slug,
            net,
            world,
            last_save: Instant::now(),
            tick_count: 0,
            audit,
            known_clients: std::collections::HashSet::new(),
        })
    }

    /// One server-tick: drain net events, run sim subsystems (none
    /// yet at the headless layer, by design), broadcast roster
    /// updates, audit new connections / disconnects.
    pub fn tick(&mut self) {
        self.tick_count += 1;
        tick::run(self);
    }

    /// Wall-clock check whether `autosave_secs` have elapsed since
    /// the previous save.
    pub fn autosave_due(&self) -> bool {
        self.last_save.elapsed()
            >= std::time::Duration::from_secs(self.args.autosave_secs as u64)
    }

    /// Flush every dirty file in the slot. The current shape
    /// re-writes every loaded chunk (matching the GUI demo's Phase
    /// B.7a baseline); a dirty-chunk tracking pass lands later.
    pub fn save(&mut self) -> Result<()> {
        let world_dir = resolve_world_dir(&self.args, &self.slug);

        // Bump meta timestamps.
        let mut meta = meta::read_meta_in_dir_with_registry(
            world_dir.parent().unwrap_or(&world_dir),
            &self.slug,
            voxel_demo_mp::world_save::migrate::default_registry(),
        )
        .with_context(|| "reading meta for save bump")?;
        let now = chrono::Utc::now();
        let elapsed_secs = (now - meta.last_played).num_seconds().max(0) as u64;
        meta.last_played = now;
        meta.play_time_seconds = meta.play_time_seconds.saturating_add(elapsed_secs);
        meta::write_meta_in_dir(
            world_dir.parent().unwrap_or(&world_dir),
            &self.slug,
            &meta,
        )
        .with_context(|| "writing meta.json")?;

        // Iterate every loaded chunk + write its payload.
        let coords: Vec<ChunkCoord> =
            self.world.iter_chunks().map(|(c, _)| *c).collect();
        let mut chunk_count: usize = 0;
        for coord in coords {
            let Some(chunk) = self.world.chunk(coord) else {
                continue;
            };
            let payload = ChunkPayload {
                format_version: CHUNK_PAYLOAD_VERSION,
                coord,
                voxels: ChunkVoxels::from_chunk(chunk),
                liquid_levels: Vec::new(),
                torch_orient: Vec::new(),
                snow_layers: Vec::new(),
            };
            chunks::write_chunk_in_dir(
                world_dir.parent().unwrap_or(&world_dir),
                &self.slug,
                OVERWORLD,
                &payload,
            )?;
            chunk_count += 1;
        }
        self.audit.flush()?;
        self.last_save = Instant::now();
        info!(
            slug = %self.slug,
            chunks = chunk_count,
            "autosave ok"
        );
        Ok(())
    }

    pub fn net_mut(&mut self) -> &mut NetServer {
        &mut self.net
    }

    pub fn audit_mut(&mut self) -> &mut AuditLog {
        &mut self.audit
    }

    pub fn known_clients_mut(&mut self) -> &mut std::collections::HashSet<u32> {
        &mut self.known_clients
    }
}

/// Resolve the per-slot world directory. Honours `--data-dir` for
/// integration tests; falls back to the `directories`-resolved
/// default the GUI demo writes into.
fn resolve_world_dir(args: &Args, slug: &str) -> PathBuf {
    match &args.data_dir {
        Some(base) => layout::world_dir_in(base, slug),
        None => layout::world_dir(slug),
    }
}

/// Lift a `cx_cy_cz.chunk.bin` filename into the corresponding
/// `ChunkCoord`. Mirror of `world_save::runtime::helpers::
/// parse_chunk_filename` - copied here so the lib's headless slice
/// doesn't have to expose the runtime/helpers internals.
fn parse_chunk_filename(path: &std::path::Path) -> Option<ChunkCoord> {
    let stem = path.file_stem()?.to_str()?;
    let stem = stem.strip_suffix(".chunk").unwrap_or(stem);
    let parts: Vec<&str> = stem.split('_').collect();
    if parts.len() != 3 {
        return None;
    }
    let cx = parts[0].parse::<i32>().ok()?;
    let cy = parts[1].parse::<i32>().ok()?;
    let cz = parts[2].parse::<i32>().ok()?;
    Some([cx, cy, cz])
}

/// Decode a `ChunkPayload` into a `Chunk` + insert it at `payload.coord`.
fn apply_chunk_payload(world: &mut World, payload: ChunkPayload) -> Result<()> {
    let coord = payload.coord;
    let chunk = payload
        .voxels
        .into_chunk()
        .with_context(|| format!("decoding chunk {:?}", coord))?;
    world.insert_chunk(coord, chunk);
    Ok(())
}

/// Quick wall-clock-based seed for fresh worlds. Just hashes the
/// current epoch nanos so consecutive launches get distinct seeds
/// without a `rand` dep.
fn generate_seed() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let mut h = nanos.wrapping_mul(0x9E37_79B1_7F4A_7C15);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h as u32
}

// Re-export the chunk-size constant so the smoke test can build
// expected payloads without re-importing infengine_voxel.
#[allow(dead_code)]
pub(crate) const CHUNK_DIM: usize = CHUNK_SIZE;
