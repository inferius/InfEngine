# _assets — imported game data, temporary

Everything in here is **derived from commercial games** and is a
development placeholder, not part of the project. It exists so the
engine and the editor have real rooms, real art and real dialogue to be
tested against while our own content does not exist yet.

Three rules:

1. **Never committed.** The whole directory is gitignored. These files
   are someone else's copyrighted work run through a converter; the
   converter is ours to ship, the output is not.
2. **Never shipped.** When the game has its own art, this directory goes
   away. Nothing outside it may depend on a file in here.
3. **Always regenerable.** Delete it and run the commands below. Nothing
   in here is authored by hand, so losing it costs minutes, not work.

## Regenerating

Paths are the local copies of games owned by the developer.

```bash
# The Legend of Kyrandia, English and Czech into one project
cargo run -p infengine_advimport --release --bin adv-import -- \
    kyra "F:/Games/Legend of Kyrandia/DAT" \
    --out _assets/kyrandia --language en
cargo run -p infengine_advimport --release --bin adv-import -- \
    kyra "C:/tmp/The_Legend_Of_Kyrandia_1992_1.3_04022026_2013/kyrandia1-cd-gog-scummvm-cestina_v1.3/GOG/DAT" \
    --out _assets/kyrandia --language cs

# Black Mirror 2 and 3 (large: about 1.6 GB of art and voice)
cargo run -p infengine_advimport --release --bin adv-import -- \
    bm "D:/SteamLibrary/steamapps/common/Black Mirror 2 - Reigning Evil" \
    --out _assets/blackmirror2
cargo run -p infengine_advimport --release --bin adv-import -- \
    bm "D:/SteamLibrary/steamapps/common/Black Mirror 3 - Final Fear" \
    --out _assets/blackmirror3
```

## Opening one in the editor

```bash
cargo run -p adventure_editor --release -- _assets/kyrandia
```
