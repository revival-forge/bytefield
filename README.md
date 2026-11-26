# Bytefield

*A free engine (GPL-3.0-or-later) that plays Battlefield 1942 and its mods **with the data of your own copy**, the
way OpenMW plays Morrowind. It contains no game or mod data.* · [Español](README.es.md)

Not affiliated with Electronic Arts or DICE. "Battlefield 1942" is a trademark of its owners and is used here only to
say which data this engine works with.

## The idea

First, **fidelity**: play the original game and its classic mods as they were, on today's systems, with the original
files: the same maps, vehicles, weapons, rules and look. The original is the reference, and differences from it are
bugs.

Then, on that base, what the 2002 engine could not do:

- **better bots**, and bots in mods and maps that never had them;
- **more players** per server;
- **new vehicles, game modes and mods**, built on the same data formats the community already knows;
- **better graphics** as an option (the *Enhanced* profile, HD texture packs), with the original look always one click away.

## What works today

- **Content:**
  - Battlefield 1942 1.6;
  - its expansions, The Road to Rome and Secret Weapons of WWII;
  - Desert Combat Final 0.8 (the most tested mod), and Desert Combat 0.7, which loads;
  - Galactic Conquest 8.5;
  - the HD Remaster texture packs.

  Other mods show up in the menu too; how well they play depends on what they use. What is promised for each kind of content, in
  [docs/user/features.md](docs/user/features.md).
- **Game modes:** Conquest, Capture the Flag, objective maps, cooperative and team deathmatch, with vehicles, aircraft,
  ships and fixed weapons.
- **Bots:** this is where Bytefield parts from the original on purpose, for better or worse. They are our own, not a
  copy of the original's: they take flags, drive, fly, sail, man fixed guns, heal and repair, though not very skilfully
  yet. Making them better is on the roadmap.
- **Multiplayer** between Bytefield clients, with Bytefield's own protocol (`bytefield-server`, and *Join* in the
  menu). It does not connect to the original servers.
- **CD key:** Bytefield does not check it. Since the original online service closed, the community's servers stopped
  checking keys, and Bytefield does the same. You still play with the files of your own copy of the game.
- **Graphics profiles:** *Original* (the original look) and *Enhanced*, plus single settings and an *Experimental* page.

## Known limitations (0.2)

Expect differences from the original: each one is a bug, and reports help most.

- **Multiplayer** has been tested with several clients on one machine; playing across machines is new.
- **Galactic Conquest** loads and plays, but it has had far less play by people than Desert Combat.
- **Bots** play every mode and walk on a navigation mesh, but they still get stuck in some places and some do not fly well.
- **Sound** is incomplete in places, and some mods' sounds may be missing or too quiet.
- On integrated GPUs the *Enhanced* profile can be slow on large maps; *Original* is lighter.
- There is no launcher yet: content is switched on in the menu or with `bytefield-content`.

## What you need

Your own Battlefield 1942, installed (version 1.6). Expansions and mods are optional; which ones Bytefield knows and
where to get them is in [docs/user/features.md](docs/user/features.md). The mods, from their own pages (Bytefield
does not host them):

- Desert Combat: [ModDB](https://www.moddb.com/mods/desert-combat);
- Galactic Conquest: [ModDB](https://www.moddb.com/mods/battlefield-galactic-conquest).

Step by step for players: **[Windows guide](docs/user/windows.md)** ([en español](docs/user/windows.es.md)).

## Build and play

Ready-made packages for Windows and Linux are on the [Releases](https://github.com/revival-forge/bytefield/releases)
page. To build Bytefield yourself (it is written in Rust) you need:

1. **Rust**, installed with [rustup](https://rustup.rs) (stable). It brings `cargo`, the tool that builds the project.
2. **On Linux**, a C compiler, `pkg-config` and the ALSA headers for sound: on Debian and Ubuntu,
   `sudo apt install build-essential pkg-config libasound2-dev`; on Fedora, `sudo dnf install gcc pkg-config alsa-lib-devel`.
   **On Windows**, the Visual Studio Build Tools, which rustup offers to install.

Then, in the source folder:

```
cargo build --release -p bytefield-client
target/release/bytefield
```

The first build takes several minutes. On start Bytefield looks for the game: what is saved in `content.cfg`, then
the usual install folders and Wine prefixes; if in doubt it asks for the folder on a first screen. It can also be told:
`target/release/bytefield <folder> [mod] [level] [--bots 16]`.

More: building for Windows or for older Linux distributions in [docs/user/building.md](docs/user/building.md); every
command in [docs/commands.md](docs/commands.md); how the code is put together in [docs/dev/](docs/dev/README.md)
(architecture, data formats, the `.con` dialect, the headless bench).

## Tests

```
cargo test
cd python && uv run pytest
```

Tests that need real game data skip when they do not find it; the rest use synthetic data.

## Roadmap

1. Close the remaining gaps with the original in the most played maps, vanilla and expansions, then Desert Combat and
   Galactic Conquest.
2. A launcher for content and packs (today `bytefield-content` on the command line, plus the in-game menu).
3. Better bots everywhere, including Galactic Conquest's vehicles.
4. Larger servers.
5. More mods: tell us which one you play (see [CONTRIBUTING.md](CONTRIBUTING.md)).

## Contributing and bugs

See [CONTRIBUTING.md](CONTRIBUTING.md): how to report a bug (Ctrl+F11 in game leaves a mark with a screenshot), how to
ask for a mod, and the rules for code, among them that **nothing derived from decompiling EA's executables is accepted**.

## Licence and contact

GPL-3.0-or-later. Third-party credits in [CREDITS.md](CREDITS.md).
Rights holders and private matters: **revival-forge@proton.me**; we answer within days.
