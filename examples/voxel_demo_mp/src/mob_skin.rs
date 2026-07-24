//! Wave 12.8 - Mob skin import.
//!
//! Walk `assets/minecraft/textures/entity/<mob>/<mob>.png` for
//! every `MobKind` and resolve the file path.
//!
//! The actual GPU texture upload + UV mapping per body part is
//! layered on top by the mob render path; this module ships the
//! path discovery + fallback predicate so the engine knows which
//! mobs the user has skins for.
//!
//! Per W12 design decisions: this wave ships the **simplest**
//! version - each mob gets a single full-texture sample stretched
//! on a billboard / flat-cube mesh. Per-body-part UV (head / body /
//! legs separate) is a W13+ follow-up.
//!
//! Cite the MC source for the texture file names:
//! `J:\Temp\template-mod-template-26.1.2\sources\
//! minecraft-merged-52430b475d-26.1.2-sources\
//! net\minecraft\client\renderer\entity\` - per-mob renderer
//! classes (e.g. `ZombieRenderer.java` references
//! `textures/entity/zombie/zombie.png`).

use std::path::{Path, PathBuf};

use crate::mob::kinds::MobKind;

/// Per-MobKind subdirectory + file stem inside
/// `assets/minecraft/textures/entity/`. Most kinds match MC 1:1.
pub(crate) fn entity_texture_subpath(kind: MobKind) -> (&'static str, &'static str) {
    match kind {
        MobKind::Zombie => ("zombie", "zombie"),
        MobKind::Skeleton => ("skeleton", "skeleton"),
        MobKind::Spider => ("spider", "spider"),
        MobKind::Sheep => ("sheep", "sheep"),
        // Cow + pig + chicken use the standard layout too. Vanilla
        // MC variants (mooshroom, baby, ...) live in subdirs we
        // can route to in a follow-up.
        MobKind::Cow => ("cow", "cow"),
        MobKind::Pig => ("pig", "pig"),
        MobKind::Chicken => ("chicken", "chicken"),
        // W158.
        MobKind::Wolf => ("wolf", "wolf"),
        // W161: MC ships the polar bear under entity/bear/.
        MobKind::PolarBear => ("bear", "polarbear"),
        // W162: MC ships rabbit colour variants; brown is the default.
        MobKind::Rabbit => ("rabbit", "rabbit_brown"),
        // W300: piglin family shares the piglin subdir.
        MobKind::ZombifiedPiglin => ("piglin", "zombified_piglin"),
        // W302: base villager skin (type/profession layers later).
        MobKind::Villager => ("villager", "villager"),
        // W315: tabby is the pack default cat variant.
        MobKind::Cat => ("cat", "cat_tabby"),
        // W322: golem + the brown default horse variant.
        MobKind::IronGolem => ("iron_golem", "iron_golem"),
        MobKind::Horse => ("horse", "horse_brown"),
        // W327: nether pair (magma cube shares the slime folder).
        MobKind::Ghast => ("ghast", "ghast"),
        MobKind::MagmaCube => ("slime", "magmacube"),
        // W330.
        MobKind::ZombieVillager => ("zombie_villager", "zombie_villager"),
        // W343.
        MobKind::Creeper => ("creeper", "creeper"),
        // W344: MC ships the donkey under entity/horse/.
        MobKind::Donkey => ("horse", "donkey"),
        MobKind::Bee => ("bee", "bee"),
        // W376: MC ships the witch flat under entity/ and the slime
        // next to the magma cube.
        MobKind::Witch => ("", "witch"),
        MobKind::Slime => ("slime", "slime"),
        // W377: MC ships the zombie variants under entity/zombie/
        // and the stray under entity/skeleton/.
        MobKind::Drowned => ("zombie", "drowned"),
        MobKind::Husk => ("zombie", "husk"),
        MobKind::Stray => ("skeleton", "stray"),
        MobKind::Pillager => ("illager", "pillager"),
        MobKind::Vindicator => ("illager", "vindicator"),
        MobKind::Evoker => ("illager", "evoker"),
        MobKind::Blaze => ("", "blaze"),
        MobKind::WitherSkeleton => ("skeleton", "wither_skeleton"),
    }
}

/// Resolve the full path to a mob skin inside the user's MC
/// asset tree. Tries `.tga` first (Bedrock / Java extracted packs),
/// then `.png` (vanilla Java jar). Returns `None` when neither
/// file exists; caller falls back to the kind's hard-coded color.
///
/// W29.3: added `.tga` probe so Bedrock-layout resource packs work
/// without requiring the user to rename their files.
pub(crate) fn mob_skin_path(
    assets_root: &Path,
    kind: MobKind,
) -> Option<PathBuf> {
    let (subdir, stem) = entity_texture_subpath(kind);
    let base = assets_root
        .join("textures")
        .join("entity")
        .join(subdir);
    // Try TGA first (common in Bedrock packs + extracted Java jars).
    let tga = base.join(format!("{stem}.tga"));
    if tga.is_file() {
        return Some(tga);
    }
    let png = base.join(format!("{stem}.png"));
    if png.is_file() {
        Some(png)
    } else {
        None
    }
}

/// True when EVERY supported mob has a resolvable skin under
/// `assets_root`. Used by the engine startup banner to report
/// "MC mob skins detected" vs "falling back to colours".
pub(crate) fn full_skin_set_present(assets_root: &Path) -> bool {
    [
        MobKind::Sheep,
        MobKind::Zombie,
        MobKind::Chicken,
        MobKind::Pig,
        MobKind::Cow,
        MobKind::Skeleton,
        MobKind::Spider,
    ]
    .iter()
    .all(|k| mob_skin_path(assets_root, *k).is_some())
}

/// Aggregate result handed to the renderer. Each kind that
/// resolved a PNG carries the path; kinds that fell through stay
/// at `None` so the render path falls back to the kind's hard-
/// coded colour.
pub(crate) struct MobSkinSet {
    pub(crate) sheep: Option<PathBuf>,
    pub(crate) zombie: Option<PathBuf>,
    pub(crate) chicken: Option<PathBuf>,
    pub(crate) pig: Option<PathBuf>,
    pub(crate) cow: Option<PathBuf>,
    pub(crate) skeleton: Option<PathBuf>,
    pub(crate) spider: Option<PathBuf>,
    /// W158.
    pub(crate) wolf: Option<PathBuf>,
    /// W161.
    pub(crate) polar_bear: Option<PathBuf>,
    /// W162.
    pub(crate) rabbit: Option<PathBuf>,
    pub(crate) zombified_piglin: Option<PathBuf>,
    pub(crate) villager: Option<PathBuf>,
    pub(crate) cat: Option<PathBuf>,
    pub(crate) iron_golem: Option<PathBuf>,
    pub(crate) horse: Option<PathBuf>,
    pub(crate) ghast: Option<PathBuf>,
    pub(crate) magma_cube: Option<PathBuf>,
    pub(crate) zombie_villager: Option<PathBuf>,
    pub(crate) creeper: Option<PathBuf>,
    pub(crate) donkey: Option<PathBuf>,
    pub(crate) bee: Option<PathBuf>,
    pub(crate) witch: Option<PathBuf>,
    pub(crate) slime: Option<PathBuf>,
    pub(crate) drowned: Option<PathBuf>,
    pub(crate) husk: Option<PathBuf>,
    pub(crate) stray: Option<PathBuf>,
    pub(crate) pillager: Option<PathBuf>,
    pub(crate) vindicator: Option<PathBuf>,
    pub(crate) evoker: Option<PathBuf>,
    pub(crate) blaze: Option<PathBuf>,
    pub(crate) wither_skeleton: Option<PathBuf>,
}

impl MobSkinSet {
    pub(crate) fn discover(assets_root: &Path) -> Self {
        Self {
            sheep: mob_skin_path(assets_root, MobKind::Sheep),
            zombie: mob_skin_path(assets_root, MobKind::Zombie),
            chicken: mob_skin_path(assets_root, MobKind::Chicken),
            pig: mob_skin_path(assets_root, MobKind::Pig),
            cow: mob_skin_path(assets_root, MobKind::Cow),
            skeleton: mob_skin_path(assets_root, MobKind::Skeleton),
            spider: mob_skin_path(assets_root, MobKind::Spider),
            wolf: mob_skin_path(assets_root, MobKind::Wolf),
            polar_bear: mob_skin_path(assets_root, MobKind::PolarBear),
            rabbit: mob_skin_path(assets_root, MobKind::Rabbit),
            zombified_piglin: mob_skin_path(assets_root, MobKind::ZombifiedPiglin),
            villager: mob_skin_path(assets_root, MobKind::Villager),
            cat: mob_skin_path(assets_root, MobKind::Cat),
            iron_golem: mob_skin_path(assets_root, MobKind::IronGolem),
            horse: mob_skin_path(assets_root, MobKind::Horse),
            ghast: mob_skin_path(assets_root, MobKind::Ghast),
            magma_cube: mob_skin_path(assets_root, MobKind::MagmaCube),
            zombie_villager: mob_skin_path(assets_root, MobKind::ZombieVillager),
            creeper: mob_skin_path(assets_root, MobKind::Creeper),
            donkey: mob_skin_path(assets_root, MobKind::Donkey),
            bee: mob_skin_path(assets_root, MobKind::Bee),
            witch: mob_skin_path(assets_root, MobKind::Witch),
            slime: mob_skin_path(assets_root, MobKind::Slime),
            drowned: mob_skin_path(assets_root, MobKind::Drowned),
            husk: mob_skin_path(assets_root, MobKind::Husk),
            stray: mob_skin_path(assets_root, MobKind::Stray),
            pillager: mob_skin_path(assets_root, MobKind::Pillager),
            vindicator: mob_skin_path(assets_root, MobKind::Vindicator),
            evoker: mob_skin_path(assets_root, MobKind::Evoker),
            blaze: mob_skin_path(assets_root, MobKind::Blaze),
            wither_skeleton: mob_skin_path(assets_root, MobKind::WitherSkeleton),
        }
    }

    pub(crate) fn for_kind(&self, k: MobKind) -> Option<&Path> {
        let opt = match k {
            MobKind::Sheep => &self.sheep,
            MobKind::Zombie => &self.zombie,
            MobKind::Chicken => &self.chicken,
            MobKind::Pig => &self.pig,
            MobKind::Cow => &self.cow,
            MobKind::Skeleton => &self.skeleton,
            MobKind::Spider => &self.spider,
            MobKind::Wolf => &self.wolf,
            MobKind::PolarBear => &self.polar_bear,
            MobKind::Rabbit => &self.rabbit,
            MobKind::ZombifiedPiglin => &self.zombified_piglin,
            MobKind::Villager => &self.villager,
            MobKind::Cat => &self.cat,
            MobKind::IronGolem => &self.iron_golem,
            MobKind::Horse => &self.horse,
            MobKind::Ghast => &self.ghast,
            MobKind::MagmaCube => &self.magma_cube,
            MobKind::ZombieVillager => &self.zombie_villager,
            MobKind::Creeper => &self.creeper,
            MobKind::Donkey => &self.donkey,
            MobKind::Bee => &self.bee,
            MobKind::Witch => &self.witch,
            MobKind::Slime => &self.slime,
            MobKind::Drowned => &self.drowned,
            MobKind::Husk => &self.husk,
            MobKind::Stray => &self.stray,
            MobKind::Pillager => &self.pillager,
            MobKind::Vindicator => &self.vindicator,
            MobKind::Evoker => &self.evoker,
            MobKind::Blaze => &self.blaze,
            MobKind::WitherSkeleton => &self.wither_skeleton,
        };
        opt.as_deref()
    }

    /// Count of resolved kinds. Used by the startup banner.
    pub(crate) fn loaded_count(&self) -> usize {
        [
            &self.sheep,
            &self.zombie,
            &self.chicken,
            &self.pig,
            &self.cow,
            &self.skeleton,
            &self.spider,
        ]
        .iter()
        .filter(|o| o.is_some())
        .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_root(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "infengine_mob_skin_{}_{}_{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(&p).expect("mkdir");
        p
    }

    fn write_skin(root: &Path, kind: MobKind) {
        let (sub, stem) = entity_texture_subpath(kind);
        let dir = root.join("textures").join("entity").join(sub);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join(format!("{stem}.png")), b"\x89PNG").expect("write");
    }

    #[test]
    fn mob_skin_loaded_for_zombie() {
        let root = temp_root("zombie");
        write_skin(&root, MobKind::Zombie);
        let p = mob_skin_path(&root, MobKind::Zombie)
            .expect("zombie skin should resolve");
        assert!(p.is_file());
        assert!(p.to_string_lossy().contains("zombie"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn mob_skin_falls_back_to_color_when_missing() {
        let root = temp_root("missing");
        // No skins written.
        assert!(mob_skin_path(&root, MobKind::Sheep).is_none());
        assert!(mob_skin_path(&root, MobKind::Zombie).is_none());
        let set = MobSkinSet::discover(&root);
        assert_eq!(set.loaded_count(), 0);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn partial_install_resolves_only_present_kinds() {
        let root = temp_root("partial");
        write_skin(&root, MobKind::Chicken);
        write_skin(&root, MobKind::Pig);
        let set = MobSkinSet::discover(&root);
        assert!(set.for_kind(MobKind::Chicken).is_some());
        assert!(set.for_kind(MobKind::Pig).is_some());
        assert!(set.for_kind(MobKind::Zombie).is_none());
        assert_eq!(set.loaded_count(), 2);
        assert!(!full_skin_set_present(&root));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn full_install_resolves_every_kind() {
        let root = temp_root("full");
        for k in [
            MobKind::Sheep,
            MobKind::Zombie,
            MobKind::Chicken,
            MobKind::Pig,
            MobKind::Cow,
            MobKind::Skeleton,
            MobKind::Spider,
        ] {
            write_skin(&root, k);
        }
        let set = MobSkinSet::discover(&root);
        assert_eq!(set.loaded_count(), 7);
        assert!(full_skin_set_present(&root));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn entity_subpath_uses_mob_name() {
        // Every kind's subdir + stem should equal the kind's
        // canonical short name so the loader can use the
        // MobKind::name() helper to build the path too.
        for k in [
            MobKind::Sheep,
            MobKind::Zombie,
            MobKind::Chicken,
            MobKind::Pig,
            MobKind::Cow,
            MobKind::Skeleton,
            MobKind::Spider,
        ] {
            let (sub, stem) = entity_texture_subpath(k);
            assert_eq!(sub, k.name(), "subdir mismatch for {k:?}");
            assert_eq!(stem, k.name(), "stem mismatch for {k:?}");
        }
    }

    #[test]
    fn for_kind_returns_path_when_set() {
        let root = temp_root("for_kind");
        write_skin(&root, MobKind::Spider);
        let set = MobSkinSet::discover(&root);
        let p = set.for_kind(MobKind::Spider).expect("spider path");
        assert!(p.to_string_lossy().contains("spider"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn mob_skin_path_rejects_directory() {
        // If someone puts a directory at the texture path the
        // resolver should NOT return it (we need a file).
        let root = temp_root("is_dir");
        let (sub, stem) = entity_texture_subpath(MobKind::Sheep);
        let dir = root
            .join("textures")
            .join("entity")
            .join(sub)
            .join(format!("{stem}.png"));
        fs::create_dir_all(&dir).expect("mkdir at png path");
        assert!(mob_skin_path(&root, MobKind::Sheep).is_none());
        fs::remove_dir_all(&root).ok();
    }

    // W29.3 - TGA fallback tests ------------------------------------------

    /// W29.3: mob_skin_path resolves a .tga file when no .png exists.
    #[test]
    fn mob_skin_path_resolves_tga() {
        let root = temp_root("tga_sheep");
        let (sub, stem) = entity_texture_subpath(MobKind::Sheep);
        let dir = root.join("textures").join("entity").join(sub);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join(format!("{stem}.tga")), b"fake_tga").expect("write tga");
        let p = mob_skin_path(&root, MobKind::Sheep).expect("should resolve tga");
        assert!(p.to_string_lossy().ends_with(".tga"), "expected .tga path, got {p:?}");
        fs::remove_dir_all(&root).ok();
    }

    /// W29.3: .tga takes priority over .png when both exist.
    #[test]
    fn mob_skin_path_prefers_tga_over_png() {
        let root = temp_root("tga_priority");
        let (sub, stem) = entity_texture_subpath(MobKind::Zombie);
        let dir = root.join("textures").join("entity").join(sub);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join(format!("{stem}.tga")), b"fake_tga").expect("write tga");
        fs::write(dir.join(format!("{stem}.png")), b"\x89PNG").expect("write png");
        let p = mob_skin_path(&root, MobKind::Zombie).expect("should resolve");
        assert!(p.to_string_lossy().ends_with(".tga"), "tga must win over png, got {p:?}");
        fs::remove_dir_all(&root).ok();
    }

    /// W29.3: full_skin_set_present works with all-TGA install.
    #[test]
    fn full_skin_set_present_with_tga_files() {
        let root = temp_root("tga_full");
        for k in [
            MobKind::Sheep, MobKind::Zombie, MobKind::Chicken,
            MobKind::Pig, MobKind::Cow, MobKind::Skeleton, MobKind::Spider,
        ] {
            let (sub, stem) = entity_texture_subpath(k);
            let dir = root.join("textures").join("entity").join(sub);
            fs::create_dir_all(&dir).expect("mkdir");
            fs::write(dir.join(format!("{stem}.tga")), b"fake_tga").expect("write");
        }
        assert!(full_skin_set_present(&root), "all-TGA install must report full set");
        fs::remove_dir_all(&root).ok();
    }
}
