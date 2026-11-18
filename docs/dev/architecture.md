# Architecture

bytefield is a Cargo workspace. The rule that shapes it: **the simulation never depends on the renderer.** Everything that
decides what happens in a match runs headless and deterministically (same inputs, same results, bit for bit); the
window is one client of it among others (tests, Python, command-line tools, the dedicated server).

## The crates

| Crate | What it is | Depends on |
|---|---|---|
| `vfs` | The game's file system: `.rfa` archives (LZO1X), mod search paths, patch archives overriding base ones, case-insensitive lookup | — |
| `con-lang` | The `.con` script interpreter: language, sessions (a mod mounted and its scripts run) and the template database | `vfs` |
| `formats` | Readers for the data files: textures (DDS, TGA), meshes (`.sm`, `.tm`), terrain, animations, sounds' scripts, lexicon | — |
| `world` | A level's static world: terrain and placed objects, built from the template database | `vfs`, `con-lang`, `formats` |
| `sim` | The simulation core: vehicles from their templates, physics, contact, damage, soldiers, weapons, game modes, bots | `vfs`, `con-lang`, `formats`, `world` |
| `server` | A dedicated server over `sim`: players' actions in, snapshots out, transport-free | `sim` |
| `content` | What the player has and turns on: the install, the mods, graphics packs and the graphics profile | `con-lang` |
| `py` | Python bindings of the headless bench (`import bytefield`) | `sim`, `world` |
| `client` | The game client and viewer, built on Bevy | all of the above |
| `extract` | Unpacks the installers that the game's mods shipped in, without running them | — |
| `testdata` | For tests only: finds the install and the content layers it has (see [tests.md](tests.md)) | — |

`vfs`, `con-lang`, `formats`, `world` and `sim` are the core; none of them knows about Bevy, a window or a clock.

## How a level gets loaded

1. `content` finds the install and the mod; `con-lang`'s session mounts the mod's search path through `vfs`.
2. The mod's and the level's scripts run: every `ObjectTemplate.create`, `GeometryTemplate.*` and so on lands in the
   template database. The scripts are data; nothing in them is executed outside the interpreter.
3. `world` builds the static level from the database: the terrain (heightmap and its parameters), the placed
   objects with their meshes, the water level, the combat area.
4. `sim::Sim::new` takes the world and a fixed step (1/30 s, the original's simulation rate). Spawners, control points
   and players are made when the round starts.

## The simulation's state

`Sim` holds the whole match. Its systems are written as `impl Sim` blocks, one file per system (`damage.rs`,
`projectile.rs`, `players.rs`, …), over a state grouped by who owns it:

| Field | What | Written by |
|---|---|---|
| `clock` | the step, the time and the number of steps (`Clock`) | `Sim::step` |
| `rules` | the server's configuration, fixed during a round (`RoundRules`) | the setup, before the round |
| `round` | the round's progress: playing, pregame, its end, the respawn waves (`RoundState`) | `round.rs` |
| `modes` | the game mode's state: conquest, capture the flag, objectives (`Modes`) | each mode's file |
| `damage` | the frame's pending damage and who caused it (`DamageState`) | `damage.rs` |
| `journal` | what the round records for tests, tools and the network: shots, hits, spawns, kits (`Journal`) | each system, appending only |
| `drivers` | the bots' minds (`Drivers`) | `bot_api.rs` |
| `role` | server, remote copy or replica (`Role`) | the network code |
| `players`, `rng` | the players and the one seeded random generator | `players.rs` and the bots |
| `vehicles`, `bodies`, `projectiles`, `items`, … | the entities every system reads and moves | every system |

A system reads anything but writes its own context; the entities are the shared exception.

## One simulation step

`Sim::step` is a fixed sequence; its order matters, because the original's order is part of what is reproduced:

1. **Minds**: each bot and each player's input decides its controls for this frame.
2. **Vehicles and soldiers**: every moving object integrates (rigid bodies with their engines, wings, springs and
   wheels), then the collision pass against the terrain and the obstacles, then friction; parts that move with their
   parents (turrets, wheels) follow.
3. **Projectiles**: flight, hits and explosions.
4. **Kits and supplies**: medic and repair packs, detonators, supply depots, weapons fired this frame.
5. **The game**: the damage of the frame applied to armours, the game mode's frame (conquest flags and tickets, capture
   the flag, objectives), players' deaths and respawns, the end of the round, the spawners.
6. **Point bodies** (wrecks' debris and the like), and in checked builds an assertion that nothing became NaN.

Everything in a step uses the step's own numbers and a seeded random generator, so a run can be replayed exactly: the
tests, the bot tournaments and the replays of bug reports rely on it.

## The client

The client (`crates/client`) loads the same world, runs the same `Sim` at 30 Hz and draws it with Bevy, interpolating
between steps. Input becomes the same per-frame controls a bot produces. Menus, HUD, sound and the viewer's camera
live here; the server and the network code talk to `sim` and `server`, not to the client.

## Fidelity levels

Each piece of the original that bytefield reproduces is done at one of three levels, chosen before starting:
1. **exact**, checked against the original's behaviour step by step (physics, damage, game rules);
2. **faithful in form**, the same rule and structure, checked on what can be measured (rendering, HUD layout);
3. **heuristic**, good enough to play with (some of the bots' behaviour).
When an approach costs more than it is worth, the piece drops a level instead of stalling.
