# Notes for AI coding agents

Bytefield is a free engine (GPL-3.0-or-later) that plays Battlefield 1942 and its mods (Desert Combat, Galactic Conquest)
from the player's own installed copy, in the spirit of OpenMW. The repository holds no game data. This file is for AI agents
working on the code; people should read `CONTRIBUTING.md` (it applies to agents too) and `docs/dev/`.

## Where knowledge may come from (read this first)

This is the rule that decides whether a change can be accepted at all (`CONTRIBUTING.md`):

- **Never** disassemble, decompile or trace the internals of EA's executables (the game, the dedicated server, the
  expansions), and do not use or summarise anything someone else derived that way, leaked source code included.
- **Never** copy game or mod data into the repository: textures, meshes, sounds, levels, `.con` scripts, or long quotes
  of them. Do not paste copyrighted text either, such as manuals or strategy guides.
- **Allowed:** reading the `.con` scripts and file formats of a local install, to write code that reads them; observing
  the game from the outside (screenshots, timings, positions); public documentation and tools whose licence is compatible.
- In every pull request, say **how** the behaviour you implemented was found out. If you cannot say, do not write it.

## Layout

- `crates/vfs`, `crates/con-lang`, `crates/formats`, `crates/sim`: the core. It does not depend on Bevy and it is
  deterministic: the same inputs give the same state, bit for bit.
- `crates/world`, `crates/client`: the Bevy client. `crates/server`: the dedicated server and our own network protocol.
  `crates/content`: install detection and the content catalog. `crates/extract`: importing from the discs. `crates/py`: the
  Python bindings (`import bytefield`). `crates/testdata`: the test data layers.
- `python/`: tools and tests, managed with `uv`.
- `docs/dev/`: architecture, formats, the `.con` dialect, the headless bench, tests. `docs/user/`: for players.

## Build and test

```
cargo build --release && cargo test       # Rust; tests that need game data skip themselves without it
cd python && uv run maturin develop --release -m ../crates/py/Cargo.toml && uv run pytest
scripts/tests quick                       # the base game layer only, without the client's tests
scripts/tests ci                          # what the public CI runs: no game data at all
```

Rebuild the Python module (`maturin develop`) before running pytest, or the tests run against a stale one. Tests are
organised by content layer (base game, expansions, mods: `docs/dev/tests.md`). A test that needs real data asks for its
layer: in Rust with `bytefield_testdata::require!(Base, Dc)`, and in Python by the folder it lives in or with
`require("dc")`. It skips when the layer is missing. A test without data uses synthetic fixtures (`python/tests/fixtures`)
and is the only kind CI runs.

## Style

- Code, comments, file names and tool output are in English.
- **Do not run `cargo fmt` on the tree**: the repository has no rustfmt style. Keep the style of the file you edit, and
  format only your own lines, if at all.
- A change to the simulation comes with a headless test (`cargo test`), and it must keep the simulation deterministic:
  no wall clock, no unordered iteration that reaches the state, no randomness that is not seeded.
- Comments say what the code does and why. Whatever is still a guess about the original game is marked HYPOTHESIS where
  it is implemented, and what is left to do is a `TODO` comment in the code that explains itself.
- Commits are signed off (`git commit -s`, the DCO in `CONTRIBUTING.md`).
