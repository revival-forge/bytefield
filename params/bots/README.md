# Bot parameter files

These are our own numbers, not taken from the original game (`crates/sim/src/bots/params.rs`).

## Layers
The bots' named parameters (`BotParams`: name, default, range, unit, why) are read in layers, each over the one before:

1. the defaults in the code;
2. `global.toml`;
3. the mod's layer: `bf1942.toml` (and the expansions), `DC_Final.toml` (any `DC_` map); Galactic Conquest has none for now;
4. `BYTEFIELD_BOTS_PARAMS=<file>` (same format);
5. what the mind's name forces (`bots:rally_m=140`), which is how a side's file reaches the mind.

Layers 2 and 3 are compiled into the program. A layer file has `about` and a `[params]` table; a value outside its range is an error.

## Side files
A side's parameters for `tournament3` (`--a <file> --b <file>`): the mind with its variants, the team head's levers (`[levers]`)
and named parameters (`[params]`) over the layers.

- `anchor-0.toml`: the fixed rival, bots as they were when the learning loop started (1.18.0). Anchors are never edited.
- `anchor-1.toml`: the rival from 1.19.0, with `prone-fight` by default (named `bots:no-fps` since 1.20.0).
- `anchor-2.toml`: the rival from 1.20.0, with the five FPS tweaks by default (named `bots:no-pistol-close+...` since 1.21.0).
- `anchor-3.toml`: the rival from 1.21.0, with `pistol-close` (named `bots:no-armour+no-close-fight+no-grenade-throw` since 1.22.0).
- `anchor-4.toml`: the rival from 1.22.0, with `armour`, `close-fight` and `grenade-throw` by default; what a new candidate is
  measured against.
- The basket of rivals: `anchor-0.toml`, `regroup.toml`, `cover-fight.toml` (these two are anchor 0 with one variant, so they carry
  `no-prone-fight` and `no-fps`), `anchor-1.toml`, `anchor-2.toml`, `anchor-3.toml` and `anchor-4.toml`.
