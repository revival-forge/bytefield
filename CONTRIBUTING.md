# Contributing to Bytefield

Thanks for wanting to help. Bytefield is a free engine that plays Battlefield 1942 and its mods with the data of the
player's own copy, in the spirit of OpenMW. Contributions are welcome: code, testing, bug reports, documentation,
support for more mods.

## The one rule that matters most: where your knowledge comes from

We accept work based on what anyone can observe or read legally:

- **Yes:** playing the game and measuring it from the outside (screenshots, timings, positions, network captures of
  Bytefield itself), reading the `.con` scripts and file formats of **your own** copy, public community documentation and
  tools with a compatible licence, your own ideas.
- **No:** anything derived from disassembling or decompiling EA's executables (the game, the dedicated server, the
  expansions' binaries), **including when an AI tool did the decompiling or summarised decompiled code for you**;
  leaked source code; copies of game or mod data (textures, meshes, sounds, levels, scripts) in the repository;
  copyrighted text (manuals, strategy guides) pasted in.

Every pull request states **how** the behaviour it implements was found out (for example: "measured in game, see the
attached screenshots", "read from `Objects/.../Weapons.con` of DC Final 0.8", "my own design"). A pull request that
does not say it, or whose method falls in the "no" list, may be closed without review. This is not a judgement on you:
it protects the project, which cannot afford to carry code it does not have the right to publish.

Writing code with the help of an AI assistant is fine, with the same responsibility as code you typed: you must be able
to explain it and to vouch for where its knowledge came from. The AI does not make anything "clean".

## Sign your commits (DCO)

We use the [Developer Certificate of Origin](https://developercertificate.org/): every commit carries a
`Signed-off-by: Your Name <you@example.org>` line (`git commit -s` adds it). With it you state that you have the right
to submit the work under the project's licence (GPL-3.0-or-later). You keep your copyright; there is no CLA.

## Reporting a bug

Open an issue with the **Bug** template. What helps most:

1. Bytefield's version (the first line it prints, `Bytefield <commit>`, or the release name) and your OS.
2. The game edition (CDs, a "Gold"/"Anthology" pack, Origin…) and its language; `bytefield-content verify` prints whether
   your files match a version we know.
3. Mod, map, game mode, and what you did: the shortest way to make it happen again.
4. **A mark:** press **Ctrl+F11** in game the moment it happens, type a short note and press Enter. It leaves a folder
   with a screenshot and the game's state in the tickets folder (on Windows `%LOCALAPPDATA%\Bytefield\tickets`, on Linux
   `~/.cache/bytefield/tickets`). Zip it and attach it. A crash leaves one there by itself.
5. For graphics: a screenshot (**Print Screen** saves one in the `shots` folder next to `tickets`) and, if you can, the
   same view in the original game.
6. **The log:** `logs/latest.log`, next to `tickets` (the run before is `previous.log`). It begins with the version.

Never attach game files, even small ones.

## A mod that does not work (or is not supported yet)

Mods beyond the ones we test (Desert Combat, Galactic Conquest, the expansions) are welcome, also current ones such as
Forgotten Hope. Open an issue with the **Mod** template: its name and version, where it is officially downloaded from
(we link, we never host), what `bytefield-content list` says about it, and what happens. We will not ask for its files.

## Code

- Start with [docs/dev/](docs/dev/README.md): the crates, the data formats, the `.con` dialect and the headless bench.
- Working with an AI coding agent? Point it at [AGENTS.md](AGENTS.md): the commands, the style and, in short, the rules above.
- The core (`crates/vfs`, `con-lang`, `formats`, `sim`) does not depend on Bevy and is deterministic: a change to the
  simulation comes with a test that runs headless (`cargo test`). Tests use synthetic data
  (`python/tests/fixtures`), never game files; tests that need a real install ask for its layers (base game,
  expansions, mods) and skip when they are not there ([docs/dev/tests.md](docs/dev/tests.md)).
- `scripts/tests quick` (or `cargo build --release && cargo test`, and `cd python && uv run pytest`) must pass.
- The repository has no rustfmt style; do not run `cargo fmt` on the whole tree. Keep the style of the file you edit.
- Comments say *what* and *why*; do not describe how the original binary is laid out.

## Talking to us

Issues and pull requests on the project's forge. Rights holders and anything private:
**revival-forge@proton.me**, answered within days.
