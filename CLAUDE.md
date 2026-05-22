# InfEngine - Pravidla pro Claude

## File size + modularita (TVRDE PRAVIDLO)

**Nikdy nedovolit jakemukoli .rs souboru prekrocit ~1500 radku.** Pokud
soubor roste, hned ho rozdelit do submodulu **DRIVE nez dosahne 1500**.
Idealni rozsah: 200-800 radku per soubor.

Pri implementaci nove funkce / waveu:

1. **Pred psanim noveho kodu**: kde to logicky patri? Existujici modul
   nebo novy?
2. **Pokud novy subsystem** (rendering ucohosi, UI panel, AI tick,
   net dispatch, asset loader, ...): zalozit `mod <name>;` v
   prislusnem parent module/crate. Soubor `src/<area>/<name>.rs` nebo
   `src/<area>/<name>/mod.rs` pro vetsi.
3. **Volna funkce s pure args** > **metoda App struct na 200 fieldu**.
   Pokud nekde napises `impl App { fn render_X(&mut self, ...) }` nad
   100 radku, **STOP**. Extrahuj do free fn `pub fn render_x(args)`
   v dedicated module a volaj zvenci.
4. **NIKDY nepridavat nove kod do `main.rs` voxel_demo_mp**. Soubor je
   pres 16k radku, kazdy dalsi radek je dluh. Vsechno nove jde do
   submodulu.

Strom modularity pro `examples/voxel_demo_mp/src/`:

- `main.rs` - jen `fn main()` + bootstrap. Cilovy stav < 500 radku.
- `app.rs` - `Voxel` (App) struct, fields, impl new + on_start +
  on_update + on_exit (pouze top-level glue).
- `render/` - vsechno co kresli. `mobs.rs`, `chunks.rs`, `particles.rs`,
  `ui.rs`, `decor.rs` (snow/water/torch).
- `world/` - place/break/orient logic, tick.
- `net/` - dispatch arms, messages.
- `entity/` - mob AI, mob state machines.
- `input/` - kb/mouse.
- `audio/` - SFX trigger.
- `lifecycle/` - enter_world / exit_world / save / load.

Tyhle adresare AT existuji od zacatku noveho subsystemu. Kdyz zacne
nova funkcionalita psat se do existujiciho `main.rs`, je to chyba.

## Rust idioms

- Free functions s `&` args **preferovat** pred metodami na App.
  Borrow checker je friendly k explicit `(&mut field_a, &field_b, ...)`
  signaturam. Mensi diff kdyz se neco refactoruje.
- Pokud potrebujes 5+ argumentu, doporucit `#[derive(Default)] struct
  XCtx<'a> { ... }` a passnout `&XCtx` misto dlouhe arg list.
- `pub(crate)` default visibility. `pub` jen pokud cross-crate.

## Konkretni veci pro tento projekt

- Komentar konvence: `Wxx.y` prefix = wave / iteration tag (existing
  prace W1..W47).
- Conventional Commits: `feat(crate_name): WXX.Y - <short>`,
  `fix(crate_name): WXX.Y - <short>`, `refactor(crate_name): WXX.Y`.
- Commit messages anglicky. Komentare v kodu anglicky. Chat cesky.
- Co-author tag na konci commitu: `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>`
- Cargo: `cargo build -p voxel_demo_mp --release` + `cargo clippy --workspace --release --all-targets -- -D warnings` + `cargo test -p voxel_demo_mp --release --lib` mezi commity.
- Bin tests maji 7 pre-existing failures (z W42/W43); lib tests musi
  byt 590/590 green.

## Co nedelat

- Nepridavat nove fce do `main.rs`. Nikdy.
- Nepouzivat `impl App` pro neco co se nepouziva nikde jinde nez na
  jednom miste - free fn lepsi.
- Necpat 500-radkove bloky logiky do `on_update`. Extrahovat.
- Neprilezet refactor "az pozdeji" - delat to **prubezne pri psani
  nove fce**, ne pozdeji v dedikovane refactor wave.
