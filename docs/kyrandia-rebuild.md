# Rebuilding the Kyrandia project from its sources

The whole `_assets/kyrandia` project is derived state. Every byte of it
must be reproducible by the steps below, in this order, from the two
source installs. Nothing in `_assets` is allowed to be hand-edited-only:
if a change cannot be re-created by a tool, it belongs in a tool first
(that rule exists because every regression we had came from a tool
overwriting a by-hand fix it knew nothing about).

## Sources

| name  | path                                             | provides |
|-------|--------------------------------------------------|----------|
| GOG   | `F:/Games/Legend of Kyrandia/DAT`                | room PAKs (DAT/MSC/CPS/EMC/MSK), `.APK` room animations, `.VRM` voice, music |
| FLOPPY| `C:/tmp/Legend_of_Kyrandia`                      | loose files: `MAIN.EXE` (intro bytecode + strings), intro WSA/CPS/COL, `.FNT`, `.ADL` |
| kyra.dat | ScummVM's data file                           | the room graph |

The floppy `MAIN.EXE` matters: the intro/outro choreography (frame
order, loops, tick timing, placements, music tracks, effect cues, all
intro text) is bytecode inside it, not in any data file.

## Order of steps

Ownership rule: **every artifact has exactly one producing tool.** A
tool never overwrites another tool's output (see `bake` in
`kyra_intro_fit` vs `kyra_intro_time`). All index-writing tools refuse
to run when the index does not parse, instead of "starting fresh".

```text
 1. adv-import kyra <GOG> -o _assets/kyrandia --kyra-dat <kyra.dat>
       rooms, pictures, scripts (EMC translation), loca text, items
 2. kyra_expand         _assets/kyrandia/rooms <kyra.dat>
       room graph, exits, entry points
 3. kyra_scene_anims    <GOG> _assets/kyrandia
       per-room ambient animations from the .DAT tables (slot ids <room>_<n>)
 4. kyra_wsa_index      <GOG> _assets/kyrandia
       index rows for named room WSAs (sheets <room>_<stem>.png; CD keeps
       them in .APK archives of PAK format)
 5. kyra_actor          _assets/kyrandia
       Brandon: walk cycles + actor_stand idle; manifest actor block
       (side sheet faces LEFT; naming both sides is the moonwalk)
 6. kyra_font_ron       <FLOPPY>/8FAT.FNT _assets/kyrandia <lang>
       glyph nibble 1 = dark outline, >=2 = white fill
 7. adv-import music    (MT-32 render)  -> music_mt32/*.ogg
 8. adl_render          <dir with INTRO.ADL> _assets/kyrandia/sounds
       intro effects (OPL2)  -> sounds/sfx0NN.wav, ids sfx.intro_N
 9. adl_render          <dir with KYRA1A.ADL> _assets/kyrandia/sounds/kyra1a
       chapter-1 game effects -> ids kyra.sfxN  (1B..5B, KYRAMISC still open)
10. kyra_voice          <GOG> _assets/kyrandia en
       .VRM -> voice/<room>/NNN.wav; NNN == line index == "<room>.NN"
11. kyra_intro_fit      <FLOPPY> _assets/kyrandia
       backdrops (tree, mal_kal), the 280-row scroll strip
       (TOP rows 91..195 + TOP 0..64 + BOTTOM 64..176 - exactly how
       seq_introLogos composes its pages), island ambient sheets only
12. kyra_intro_time     <FLOPPY> _assets/kyrandia
       story animation sheets re-baked in PLAYED order with exact tick
       rates read from MAIN.EXE (loops included, per loop slot)
13. kyra_intro_script   <FLOPPY> _assets/kyrandia
       the four intro rooms + scripts (positions, music tracks, effect
       cues at measured tick offsets, subtitles from the EXE's own text
       table) + game.ron start/intro pointers
14. kyra_spots          (re-run after any script regeneration)
```

Build gates after tool changes: workspace clippy `-D warnings`, crate
tests, and a smoke run of `adventure_game` reading the project.

## Known facts the tools encode (do not re-derive)

- Loose WSA header is 8 bytes (no flags word); table offsets are
  absolute and `offsets[0]` lands exactly on the table's end (+768 with
  palette). PAK'd WSAs use the 10-byte header. A leading zero offset
  means "no first frame - plays over the screen" (`decode_over`).
- Scene palettes are the `.COL` files: WW (sky, equals the palette
  embedded in TOP/BOTTOM.CPS), TREE_EXP, KALLAK, MAL-KAL, KTITLE.
- Room script hooks: 0 init, 1 click (regs 1,2 = mouse, 4 = item),
  2 entry scene (runs after 0), 5 death, 6 second click. The engine
  runs entry0 then entry2 on arrival.
- The playfield is 320x144; the CPS rows below it are a sprite stash
  the original covers with its UI panel.
- `looping: true` + `PlayAnim wait: true` never releases; intro pieces
  are `looping: false`.
- MALCOLM.CPS is a sprite sheet, not a screen. WRITING.CPS is the
  writing-hand shape bank (drawn by code; drawShape is still open).
