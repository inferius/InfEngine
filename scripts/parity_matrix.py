"""Generate the full MC 26.1.2 parity matrix for voxel_demo_mp.

Diffs the vanilla registries (blockstates / items / entity loot /
biomes / configured features / structures / sounds / particles)
against the engine sources and writes a markdown matrix into
docs/mc-comparison/. Systematic: every vanilla id is classified,
nothing hand-picked.
"""
import io, os, re, json, collections

MC = r"F:\Data\mc_assets\mc_src\26.1.2"
SRC = r"F:\Projects\InfEngine\examples\voxel_demo_mp\src"
OUT = r"F:\Projects\InfEngine\docs\mc-comparison\parity-matrix-26.1.2.md"

def names_in_dir(path, ext=".json"):
    try:
        return sorted(f[:-len(ext)] for f in os.listdir(path) if f.endswith(ext))
    except FileNotFoundError:
        return []

def read(p):
    return io.open(p, encoding="utf-8", errors="replace").read()

# ---- engine-side name sets --------------------------------------
reg = read(os.path.join(SRC, "block", "registry.rs"))
block_names = set(re.findall(r'name:\s*"([a-z0-9_]+)"', reg))
item_src = read(os.path.join(SRC, "item", "mod.rs"))
item_names = set(re.findall(r'name:\s*"([a-z0-9_]+)"', item_src))
blockmap = read(os.path.join(SRC, "worldgen", "mcgen", "blockmap.rs"))
renames = dict(re.findall(r'\("([a-z0-9_]+)",\s*"([a-z0-9_]+)"\)', blockmap))
kinds = read(os.path.join(SRC, "mob", "kinds.rs"))
mob_variants = set(
    n.lower() for n in re.findall(r"^\s*([A-Z][A-Za-z]+)\s*=\s*\d+", kinds, re.M)
)
def camel_to_snake(s):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", s).lower()
mob_variants |= {camel_to_snake(n) for n in re.findall(r"^\s*([A-Z][A-Za-z]+)\s*=\s*\d+", kinds, re.M)}

# whole mcgen source blob for feature/structure substring probes
mcgen_blob = ""
for root, _dirs, files in os.walk(os.path.join(SRC, "worldgen")):
    for f in files:
        if f.endswith(".rs"):
            mcgen_blob += read(os.path.join(root, f))
biome_blob = mcgen_blob
sounds_rs = ""
sp = os.path.join(SRC, "resource_pack", "sounds.rs")
if os.path.exists(sp):
    sounds_rs = read(sp)

# ---- vanilla registries -----------------------------------------
mc_blocks = names_in_dir(os.path.join(MC, "assets", "minecraft", "blockstates"))
mc_items = names_in_dir(os.path.join(MC, "assets", "minecraft", "items"))
mc_entities = names_in_dir(os.path.join(MC, "data", "minecraft", "loot_table", "entities"))
mc_biomes = names_in_dir(os.path.join(MC, "data", "minecraft", "worldgen", "biome"))
mc_features = names_in_dir(os.path.join(MC, "data", "minecraft", "worldgen", "configured_feature"))
mc_structures = names_in_dir(os.path.join(MC, "data", "minecraft", "worldgen", "structure"))
mc_particles = names_in_dir(os.path.join(MC, "assets", "minecraft", "particles"))
sounds_json = {}
sj = os.path.join(
    os.environ.get("APPDATA", ""),
    "InfEngine", "voxel_demo_mp", "data", "cache", "mc_assets",
    "26.1.2", "assets", "minecraft", "sounds.json",
)
if os.path.exists(sj):
    sounds_json = json.loads(read(sj))

# ---- classification ---------------------------------------------
def classify_block(n):
    if n in block_names:
        return "direct"
    if n in renames:
        tgt = renames[n]
        return "alias-same" if tgt.replace("cobble", "cobblestone") in (n, n.replace("cobblestone", "cobble")) or tgt in n or n in tgt else "alias-substitute"
    return "missing"

block_rows = collections.Counter()
block_missing, block_subst = [], []
for n in mc_blocks:
    c = classify_block(n)
    block_rows[c] += 1
    if c == "missing":
        block_missing.append(n)
    elif c == "alias-substitute":
        block_subst.append(f"{n} -> {renames[n]}")

item_missing = [n for n in mc_items if n not in item_names and n not in block_names and n not in renames]

ENT_IRRELEVANT = {"player", "armor_stand", "mannequin", "giant", "illusioner"}
ent_missing = [n for n in mc_entities if n not in mob_variants and n not in ENT_IRRELEVANT]

biome_missing = [n for n in mc_biomes if f'"{n}"' not in biome_blob and n not in biome_blob]
feat_missing = [n for n in mc_features if n not in mcgen_blob]
def struct_covered(n):
    # Variant configs of one engine structure (village biomes,
    # ruined-portal placements, warm/cold ocean ruins, beached
    # shipwreck) count as covered when the base is implemented.
    cands = [n, n.replace("_", "/")]
    for pref in ("village_", "ruined_portal", "ocean_ruin", "shipwreck"):
        if n.startswith(pref):
            cands.append(pref.rstrip("_"))
    if n == "jungle_pyramid":
        cands += ["jungle_temple", "jungle pyramid"]
    if n.startswith("village_"):
        cands.append("village/" + n[len("village_"):])
    return any(c in mcgen_blob for c in cands)

struct_missing = [n for n in mc_structures if not struct_covered(n)]
part_blob = ""
for root, _dirs, files in os.walk(SRC):
    for f in files:
        if f.endswith(".rs"):
            part_blob += read(os.path.join(root, f))
particles_missing = [n for n in mc_particles if n not in part_blob]
sound_keys = sorted(sounds_json.keys())
sound_used = [k for k in sound_keys if k in sounds_rs]

# ---- write matrix -----------------------------------------------
os.makedirs(os.path.dirname(OUT), exist_ok=True)
w = io.open(OUT, "w", encoding="utf-8", newline="\n")
def line(s=""):
    w.write(s + "\n")

line("# MC 26.1.2 parity matrix (auto-generated)")
line()
line("Generated by scripts in the W368 wave from the decompiled 26.1.2")
line("registries vs the engine sources. Regenerate after every content")
line("wave; a shrinking `missing` column is the progress meter. This is")
line("REGISTRY coverage - behavioral fidelity is tracked per system in")
line("the section notes at the bottom.")
line()
line("| domain | vanilla | covered | missing | notes |")
line("|--------|---------|---------|---------|-------|")
line(f"| blocks | {len(mc_blocks)} | {len(mc_blocks)-len(block_missing)} | {len(block_missing)} | {block_rows['alias-substitute']} family-substitute aliases |")
line(f"| items | {len(mc_items)} | {len(mc_items)-len(item_missing)} | {len(item_missing)} | block-form items resolve via places |")
line(f"| entities | {len(mc_entities)-len(ENT_IRRELEVANT)} | {len(mc_entities)-len(ENT_IRRELEVANT)-len(ent_missing)} | {len(ent_missing)} | player/armor_stand/mannequin/giant excluded |")
line(f"| biomes | {len(mc_biomes)} | {len(mc_biomes)-len(biome_missing)} | {len(biome_missing)} | name-probe vs worldgen sources |")
line(f"| configured features | {len(mc_features)} | {len(mc_features)-len(feat_missing)} | {len(feat_missing)} | substring probe (coarse) |")
line(f"| structures | {len(mc_structures)} | {len(mc_structures)-len(struct_missing)} | {len(struct_missing)} | |")
line(f"| particles | {len(mc_particles)} | {len(mc_particles)-len(particles_missing)} | {len(particles_missing)} | substring probe |")
line(f"| sound events | {len(sound_keys)} | {len(sound_used)} | {len(sound_keys)-len(sound_used)} | engine plays via ENGINE_TO_MC key map |")
line()

def section(title, items, per_line=6):
    line(f"## {title} ({len(items)})")
    line()
    for i in range(0, len(items), per_line):
        line("- " + ", ".join(items[i:i+per_line]))
    line()

section("Missing blocks", block_missing)
section("Family-substitute block aliases (render as a cousin)", block_subst)
section("Missing items", item_missing)
section("Missing entities", ent_missing)
section("Missing biomes", biome_missing)
section("Missing configured features", feat_missing)
section("Missing structures", struct_missing)
section("Missing particles", particles_missing)
w.close()
print("written", OUT)
print("blocks", len(mc_blocks), "missing", len(block_missing))
print("items", len(mc_items), "missing", len(item_missing))
print("entities missing", len(ent_missing))
print("biomes", len(mc_biomes), "missing", len(biome_missing))
print("features", len(mc_features), "missing", len(feat_missing))
print("structures", len(mc_structures), "missing", len(struct_missing))
print("particles", len(mc_particles), "missing", len(particles_missing))
print("sounds", len(sound_keys), "used", len(sound_used))
