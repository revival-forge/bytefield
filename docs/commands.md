# Commands for trying Bytefield

Everything from the repository root. First, once per terminal:

```
export PATH=$HOME/.cargo/bin:$PATH
cargo build --release                       # sim, tools, server
cargo build --release -p bytefield-client      # the client (not part of cargo build)
```

`data/game` is a Battlefield 1942 1.6 install (with `Mods/bf1942`, `XPack1`, `XPack2`, `DC_Final`, `GCMOD`).

## 1. Playing normally: the menu

```
scripts/bytefield data/game                    # the bf1942 menu (like starting the game)
scripts/bytefield data/game DC_Final           # the Desert Combat menu
scripts/bytefield data/game --lang en          # in English (en fr it es de); also BYTEFIELD_LANG=en
scripts/bytefield data/game --hd               # with the registered packs; --no-hd leaves them out
scripts/bytefield data/game --graphics enhanced # the Enhanced graphics profile (original: the game's look)
```

Two separate things: **what** is drawn (texture packs, like the HD Remaster) and **how** (the graphics profile:
Original or Enhanced). Vanilla is no packs and Original. They are set with `scripts/bytefield-content`
(saved in `~/.config/bytefield/content.cfg`; another path with `BYTEFIELD_CONTENT`):
```
scripts/bytefield-content list                          # install, mods (with their chain and version), packs, profile
scripts/bytefield-content install data/game             # where BF1942 is
scripts/bytefield-content mods-dir add data/hd/custom/Mods   # mods kept elsewhere (graphicsplus)
scripts/bytefield-content pack add "data/hd/base/Graphics Mod"   # a pack (then skybox, xpack: the last ones on top)
scripts/bytefield-content pack off hd-skybox            # on, off, rm, up, down
scripts/bytefield-content graphics enhanced             # or original
scripts/bytefield-content set bloom 0.15               # single settings over the profile, the same as the VIDEO/EFFECTS menu: aniso, max_texture, mip_bias, shadows 1/0, shadow_quality low|medium|high, aa off|msaa|fxaa|smaa, sharpen 0–1, reflections 1/0, coverage 1/0, bloom 0–1, look neutral|vivid|warm, vignette 0–1, view_distance 0.5–4, grass 0–1, clouds 0–1, decals 1/0, heat 1/0, relief 0–1, aerial 0–1 ("-" removes it)
scripts/bytefield-content missing                       # what we know of and you do not have, with where to get it
scripts/bytefield-content verify                        # the install's files against the versions we know
```

In the menu:
- **MULTIPLAYER → CREATE GAME** (level, side, bots) and START.
- **MULTIPLAYER → JOIN** (`ip[:port]`).
- **OPTIONS → GENERAL**: volume, language, ADD FOLDER (type a pack's or a mod's folder and press Enter, or drop the folder on the window: it is added as `bytefield-content add` does) and a row per pack (HD PACKS when there are many).
- **OPTIONS → CONTROLS**: keys, mouse sensitivity and inversion per map (common, infantry, air, land and sea) and keyboard sensitivity in vehicles. Changes are written on SAVE, to `controls.cfg`, next to `settings.cfg`. Without that file, the first start reads the Custom profile of your install of the original, if there is one.
- **OPTIONS → VIDEO**: the graphics quality (Original/Enhanced; "Custom" once something is touched) and each single setting:
  antialiasing, sharpening, anisotropic filtering, textures, shadows, reflections, soft foliage. **EFFECTS**: bloom, colour, vignette,
  view distance, field of view, vertical sync. Those marked with `*` apply from the next round.

In a round:
- **Esc**: resume, options (applied at once), change team or kit, disconnect or quit.
- **M**: the big map; **N**: the minimap zoom (three steps, like the original).
- **Ctrl+F12**: shows or hides the debug overlay (FPS, tick, position, vehicle, speed, engine).
- **Ctrl+F11**: leaves a mark: a ticket in `data/out/tickets/<date>-markN/` with a screenshot, the state and the commit, a `MARK n` line in the log, and a line to type a note (Enter saves, Esc without a note). To report something odd: "mark 3".
- **Ctrl+F9**: shows or hides the navigation mesh drawn over the world (translucent faces a hand above each walkable polygon, green near the ground and warmer the higher; grey for islands nobody reaches; blue lines for drops, cyan for ladders). The first time it builds or reads the mesh in the background. `BYTEFIELD_NAVMESH_VIEW=1` starts with it on.
- Each game's log stays in `data/out/logs/` (`latest.log`, the last one), with a `state:` line every 5 s.
- **Tab**: the scoreboard.
- **P**: pause, in your own game (not on a server). **Print Screen**: a screenshot in `data/out/shots/` (or the user data folder).
- **G**: pick up the kit at your feet (a dead soldier's, or one placed on the map); yours stays in its place. A dropped kit lasts 30 s.
- **F1–F8**: the radio.
- **Plane** (like the original): the mouse flies (forward dives, sideways rolls); holding **Shift**, it looks around. ↑ dives and ↓ climbs (also on the ground: ↑ is +1). **Space** fires and keypad **0** is the secondary. On foot, **Shift** walks (Alt too).
- **F9–F12**: the views, as in the original: inside, behind, in front and the fixed camera the vehicle drives past (these three, in a vehicle; on foot, F10–F12 are the one behind). **C** toggles inside and behind.
- On joining, the briefing: **READY** or **Enter** to go on (not shown with `--kit` or a script).
- On joining and after dying: **← →** change the spawn point and **1–5** the kit. The first time it waits for **READY**, CLOSE or **Enter** (with `--kit N` you spawn at once; `--choose` forces the wait even with `--script` or `--screenshot`).

The options are saved in `~/.config/bytefield/settings.cfg`. Another path with `BYTEFIELD_CONFIG=/path/x.cfg`. Keys: `volume`, `lang`, `name`, `team`, `bots`, `last_mod`, `last_level`, `last_mode`, `last_server`, `fov` (degrees), `vsync`, `compute_threads` (cap on compute threads; 4 by default). Priority: command line > environment variable > file > default.

**If the CPU is tight, above all on laptops.** Two helps, at the cost of more battery and noise: the performance power profile (`powerprofilesctl launch -p performance -- scripts/bytefield …`) and, on Intel hybrid or Ryzen X3D CPUs, `gamemoderun scripts/bytefield …` if you have GameMode, which pins the game to the fast cores. Measured on a laptop, Berlin with 64 bots: from 66 to 101 fps and to 106, each one alone and without today's thread cap.

## 2. Skipping the menu: straight into a level

```
scripts/bytefield data/game bf1942 El_Alamein                       # conquest with bots (bots by default)
scripts/bytefield data/game bf1942 El_Alamein --team 2 --bots 16
scripts/bytefield data/game bf1942 El_Alamein --team 0 --bots 15         # `--team 0`: whichever side comes up (like the original, by count); the bots are split the same way
scripts/bytefield data/game bf1942 Berlin --mode coop
scripts/bytefield bf1942 Wake                  # without the folder: the one in content.cfg or one found on its own (if the first argument is not a folder, it is the mod)
BYTEFIELD_SCORE_LIMIT=3 scripts/bytefield data/game bf1942 El_Alamein --mode ctf   # capture the flag (whoever reaches 3 wins)
scripts/bytefield data/game bf1942 El_Alamein --third-person --sens 1.5 --invert-y   # --sens multiplies the map's sensitivity; --invert-y also inverts it, for that game only
scripts/bytefield data/game bf1942 El_Alamein --kit 2 --size 1920x1080   # --size or --windowed: in a window this time (full screen by default; OPTIONS > FULL SCREEN)
scripts/bytefield data/game bf1942 El_Alamein --hd
scripts/bytefield data/game --no-intro             # the menu without the opening videos (EA, Dice, Intro…; they need ffmpeg)
scripts/bytefield data/game --menu create          # opens the menu already in CREATE GAME
scripts/bytefield data/game --menu create,start    # and starts with the last choice
```

## 3. Network

```
scripts/bytefield data/game bf1942 El_Alamein --host [--port 14567] [--bots 8]   # play and serve at once
scripts/bytefield data/game --connect 192.168.1.10[:14567] [--team 2]             # join (also from the menu)
target/release/bytefield-server data/game bf1942 El_Alamein --port 14567 --bots 8 [--seconds 600] [--start-delay 20] [--min-players 2] [--friendly-fire 100] [--kickback 0] [--no-nose-cam] [--welcome "line"]... [--snapshots-per-second 30] [--packet-bytes 1200] [--bot-ai bots]   # dedicated server (bots by default): the last two are only for modest uplinks (15 or 10 per second; 300 to 1400 B per packet); each round starts with a 20 s wait; with no bots and in conquest, the map restarts when the second person arrives; `--friendly-fire` is CREATE GAME's FRIENDLY FIRE (in %, soldiers and vehicles), `--kickback` the share that comes back to whoever damages a teammate (in %, 0 by default, like the original), each `--welcome` a COMMENTS line in the briefing (`game.setServerWelcomeMessage`; five are shown); they also work for the client
cargo test --release -p bytefield-server -- --nocapture                           # server + 2 in-memory clients with loss
```

## 4. Looking without playing

```
scripts/bytefield data/game DC_Final DC_Basrahs_Edge --fly                              # free viewer, no simulation
scripts/bytefield data/game bf1942 El_Alamein --fly --at 1264.9/60/645.6 --look 1300/40/700 --screenshot data/out/shots/x.png
target/release/topdown data/game bf1942 El_Alamein data/out/el_alamein               # top-down view as PNG, no GPU
target/release/vehicles data/game bf1942 --show T34                                  # a vehicle's component tree
```

## 5. Automatic tests with screenshots (scripts)

`--script`: parts as `ticks:keys`, separated by `;`. The simulation runs at 30 ticks per second.
- Keys: `w a s d space ctrl alt z e r g c lmb rmb up down left right 1`–`9` `f1`–`f12`.
- Mouse per tick: `mx=` and `my=`.
- `kill`: the player dies; `close`: closes the death screen (like CLOSE).

```
scripts/bytefield data/game bf1942 El_Alamein --script "0-17:mx=1;17-75:w;60:e" --screenshot data/out/shots/x.png
scripts/bytefield data/game bf1942 El_Alamein --script "0-30:w;31:kill" --shot-tick 120 --screenshot data/out/shots/dead.png
scripts/bytefield data/game bf1942 El_Alamein --third-person --chase-yaw 90 --script "0-60:w" --screenshot data/out/shots/3p.png
```

`--steps`: goes through the menu in a check, as `frame:action`.
- Actions: `main multiplayer create join-page options start join disconnect exit team hd graphics volume+ volume- lang+ lang- level+ esc shot`.
- With `--screenshot`, `--script` or `--steps`, the options file is not written, unless `BYTEFIELD_CONFIG` is given.

```
scripts/bytefield data/game --steps "20:options;30:volume+;90:shot" --screenshot data/out/menu/options.png
scripts/bytefield data/game --lang de --steps "20:options;60:shot" --screenshot data/out/menu/de.png
scripts/bytefield data/game --steps "20:create;40:start;400:esc;430:shot" --screenshot data/out/menu/esc.png
```

A ticket from Ctrl+F11 in your own game can be played again without a window: `target/release/bytefield --replay data/out/tickets/<date>-markN`
replays it tick by tick and at the mark's tick says `REPLAY … SAME` (exits with 0) or `DIFFERENT` (exits with 3).

## 6. Tests and sweeps

```
cargo test --release                                  # core
cargo test --release -p bytefield-client                 # client (with the real data)
cd python && uv run pytest                            # Python (skips what has no data)
target/release/rfa check data/game                    # decode every .rfa
target/release/meshcheck data/game bf1942             # every mesh and material
cargo run --release -p bytefield-sim --example bots -- data/game bf1942 El_Alamein 32 20               # a bots-only round
cargo run --release -p bytefield-sim --example speed -- data/game bf1942 El_Alamein 32 3000            # steps per second
```

## 7. Useful debug variables

| Variable | What it does |
|---|---|
| `BYTEFIELD_LANG=en` | the language of the texts |
| `BYTEFIELD_HD=…` | packs over the install, instead of those in `content.cfg`; does not change the graphics profile |
| `BYTEFIELD_MODS_DIRS=…` | folders with more mods (`<x>/init.con`), for the tools that do not read `content.cfg` |
| `BYTEFIELD_CONTENT=…` | another `content.cfg` |
| `BYTEFIELD_<KEY>=…` | a single graphics setting over everything (`BYTEFIELD_AA=smaa`, `BYTEFIELD_BLOOM=0.3`, `BYTEFIELD_LOOK=warm`…; the keys of `bytefield-content set`) |
| `BYTEFIELD_MIP_BIAS=…` | the cameras' mip bias (0 in Original, −1 in Enhanced) |
| `BYTEFIELD_BC=0` | textures as RGBA instead of BC1–3 |
| `BYTEFIELD_ANISO=N` | anisotropic filtering (1, 2, 4, 8, 16); 1 in Original, 16 in Enhanced |
| `BYTEFIELD_MAX_TEXTURE=N` | largest texture side; 2048 in Original, 4096 in Enhanced |
| `BYTEFIELD_CONFIG=path` | another options file |
| `BYTEFIELD_HUD_DEBUG=1` | the HUD's status line (position, health, weapon in words) |
| `BYTEFIELD_SOUND_DEBUG=1` | sound trace |
