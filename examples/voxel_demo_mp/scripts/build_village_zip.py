# W245.2: regenerate src/worldgen/mcgen/data/village_data.zip from the
# local MC data store (see the mc-source-pipeline memory note). Run
# from the repo root after an MC version bump:
#   python examples/voxel_demo_mp/scripts/build_village_zip.py
# Packs the village template pools (+ the empty fallback), the village
# structure NBTs and the processor lists. The village_data.rs test
# `every_pool_element_and_socket_resolves` verifies the result.
import zipfile
import os

src = r"F:/Data/mc_assets/mc_data_26.1.2/data/minecraft"
src2 = r"F:/Data/mc_assets/mc_src/26.1.2/data/minecraft"
out = r"examples/voxel_demo_mp/src/worldgen/mcgen/data/village_data.zip"

zf = zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED)
count = {"pool": 0, "nbt": 0, "proc": 0}


def norm(p, base):
    return os.path.relpath(p, base).replace(os.sep, "/")


for root, dirs, files in os.walk(os.path.join(src, "worldgen/template_pool/village")):
    for f in sorted(files):
        p = os.path.join(root, f)
        zf.write(p, norm(p, src))
        count["pool"] += 1
zf.write(
    os.path.join(src, "worldgen/template_pool/empty.json"),
    "worldgen/template_pool/empty.json",
)
count["pool"] += 1

for root, dirs, files in os.walk(os.path.join(src2, "structure/village")):
    for f in sorted(files):
        if not f.endswith(".nbt"):
            continue
        p = os.path.join(root, f)
        zf.write(p, norm(p, src2))
        count["nbt"] += 1

for root, dirs, files in os.walk(os.path.join(src, "worldgen/processor_list")):
    for f in sorted(files):
        p = os.path.join(root, f)
        zf.write(p, norm(p, src))
        count["proc"] += 1

zf.close()
print(count, "size:", os.path.getsize(out))
