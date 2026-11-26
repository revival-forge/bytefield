# Bytefield for players: content, mods and settings

Bytefield works like OpenMW: the engine brings no game data. You point it at your Battlefield 1942 install and choose
what to play with: the base game, the expansions, mods, and graphics packs.

## Content it knows

| Content | Folder in `Mods/` | Needs | Where to get it |
|---|---|---|---|
| Battlefield 1942 1.6 (1.61 also) | `bf1942` | | your own copy |
| The Road to Rome | `XPack1` | bf1942 | your own copy |
| Secret Weapons of WWII | `XPack2` | bf1942 | your own copy |
| Desert Combat 0.7 | `DesertCombat` | bf1942 | [archive.org](https://archive.org/details/desert-combat-0.7-full-install_202310) |
| Desert Combat Final 0.8 | `DC_Final` | Desert Combat 0.7 | [archive.org](https://archive.org/details/dc_final_client) |
| Galactic Conquest 8.5 | `GCMOD` | bf1942 | the mod's own page |
| HD Remaster (packs and the *EXTRA* mods) | see below | bf1942, the expansions for its xpack part | [ModDB](https://www.moddb.com/mods/high-definition-remaster) |

## What to expect from each kind of content

Content comes in layers, and the tests are organised the same way ([tests](../dev/tests.md)). What Bytefield promises
is not the same for all of them.

| Layer | Content | What to expect |
|---|---|---|
| Base game | Battlefield 1942 1.6 | **Promised.** This is the content Bytefield is built and checked against. |
| Expansions | The Road to Rome, Secret Weapons of WWII | **Supported.** Their maps and kits play, and are checked, but the base game comes first. |
| Supported mods | Desert Combat Final, Galactic Conquest 8.5 | **Tested.** Played and checked regularly. Desert Combat Final is the one played the most. |
| Recognised mods | Desert Combat 0.7, Desert Combat Extended, Galactic Conquest Redux | Found and loaded from `Mods/`, **no promises**. Desert Combat 0.7 is known to load its levels; Desert Combat Extended and Galactic Conquest Redux load and run a level each with bots (a smoke check, not played). |
| Everything else | Any other mod that installs into `Mods/` like these | Shown in the menu, **no guarantees**: whether it plays well depends on what it uses. Tell us (see [CONTRIBUTING.md](../../CONTRIBUTING.md)). |
| Packs | HD Remaster, any graphics pack (also `BYTEFIELD_HD`) | Not a game: files laid over a mod of your install. See *Graphics* below. |

A mod is found by its `Mods/<name>/init.con`, and its chain (Desert Combat Final → Desert Combat → bf1942) comes from the
`addModPath` lines in that file, as in the original game.

## Choosing what to play

In the menu: **MULTIPLAYER → CREATE GAME**. Pick the map from the list and the game type; on the side, the **MOD**,
your team, the bots and friendly fire. **START** begins.

## The configuration files

| File | What | Linux | Windows |
|---|---|---|---|
| `content.cfg` | where the game is, extra mods folders, packs, graphics | `~/.config/bytefield/` | `%APPDATA%\Bytefield\` |
| `settings.cfg` | menu options (volume, language, name, last game…) | same folder | same folder |
| `controls.cfg` | keys and mouse, written by **SAVE** in the controls menu | same folder | same folder |
| tickets, screenshots, log | Ctrl+F11 marks, crashes, Print Screen, `logs/latest.log` (last run) and `previous.log` | `~/.cache/bytefield/` | `%LOCALAPPDATA%\Bytefield\` |

`content.cfg` is plain text, one `key=value` per line:

```text
install=C:\Program Files (x86)\EA GAMES\Battlefield 1942
mods_dir=D:\hd\custom\Mods
pack=hd-base|D:\hd\base\Graphics Mod|on
graphics=enhanced
aniso=8
```

You rarely need to edit it: the first screen writes `install`, the content folders (below) add packs and mods, and the
`bytefield-content` tool writes the rest.

## The content folders

Packs and mods dropped in a content folder are found when Bytefield starts: each in its own subfolder, packs turned on
the first time (as copying them over the game did with the original). They go to `content.cfg`, so one you turn off
stays off; one whose folder you delete goes away. The folders: `content` next to the program, and
`~/.local/share/bytefield/content` (Linux) or `%LOCALAPPDATA%\Bytefield\content` (Windows).

A subfolder with `init.con` is a mod (`content/DC_Final`). One with `Mods/` (or one folder down, as the HD Remaster's
`Graphics Mod`) is a pack if it lays files over a mod of your install, and a folder of new mods otherwise.

## `bytefield-content`: the content tool

Next to `Bytefield` there is `bytefield-content` (`bytefield-content.exe` on Windows), run from a terminal:

```
bytefield-content list              # the install, the mods it found (with their chain and version), the packs, the profile
bytefield-content verify            # your files against the versions we know (1.6, 1.61, expansions)
bytefield-content missing           # what we know of and you do not have, with where to get it
bytefield-content install <folder>  # where Battlefield 1942 is
bytefield-content add <folder>      # a pack or a mod from any folder, whichever it is
bytefield-content mods-dir add <folder>          # a Mods folder kept elsewhere (the HD Remaster's "custom")
bytefield-content pack add <folder> [name]       # a graphics pack; the last one added goes on top
bytefield-content pack on|off|rm|up|down <name>
bytefield-content graphics original|enhanced
bytefield-content set <key> <value>              # a single graphics setting ("-" removes it)
```

## Graphics: what is drawn, and how

Two separate things:

- **What is drawn: packs.** A pack is a folder with the same layout as the install (`Mods/bf1942/Archives/…`) laid over
  it, as if copied, without touching your install. The HD Remaster comes as three packs: *Graphics Mod* (textures and
  meshes), *Skybox* and *Xpack*: unzip them in a content folder, or add them in that order with `bytefield-content add`.
  **OPTIONS → GENERAL** has one line per pack to turn it on or off, from the next round. Where a pack's level files
  differ from your install's, the pack's win, even over the 1.6 patch.
- **How it is drawn: the profile.** *Original* reproduces the original look; *Enhanced* adds better filtering,
  antialiasing, shadows, reflections, the shadows of clouds drifting over the ground (with the sun's shadows on), the air
  shimmering over running engines and blasts, grass on the green ground
  near you (half density), the terrain's relief (its fine detail lit by the sun) and aerial perspective (a light haze
  over what is farther, brighter towards the sun; it never lets you see farther than the original's fog), both at half strength. **OPTIONS → VIDEO** has the profile and each setting (it becomes *Custom*
  when you change one); **EFFECTS** has glow, colour, vignette, view distance, field of view and vsync;
  **EXPERIMENTAL** has what is still being tried: soft particles (smoke and fire fading out where they meet the ground) and the light of muzzle flashes and explosions
  on their surroundings, the marks bullets and blasts leave on the ground and the walls, and the same effects as Enhanced at their full strength. Settings marked `*` apply from the next round.
  The grass is only drawn on your screen: it hides no one from the bots and the other players, so a soldier lying down
  can look half covered by it to you while the others see him clearly.

Plain *vanilla* is no packs and the *Original* profile.

## Multiplayer

Bytefield has its own network protocol: Bytefield clients play with each other, not with the original servers.

- To host from the menu: **CREATE GAME**, then others use **MULTIPLAYER → JOIN** with your address (`ip[:port]`).
- A dedicated server: `bytefield-server <game folder> <mod> <map> [--bots N] [--port 14567] [--max-players N]`.
  The port is UDP 14567 by default; open it in your router or firewall to play over the Internet.

## In game

- **Esc**: resume, options, change team or kit, disconnect, quit.
- **M**: the big map; **N**: minimap zoom. **Tab**: the scoreboard. **F1–F8**: radio. **F9–F12**: views; **C**
  switches inside and behind.
- **G**: pick up the kit at your feet.
- **Ctrl+F11**: a mark for a bug report (screenshot plus game state; type a note, Enter). **Ctrl+F12**: an overlay
  with frame rate and position.
- The intro movies need `ffmpeg` installed; without it they are skipped.
