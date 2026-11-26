# Credits and third-party notices

Bytefield is GPL-3.0-or-later (`LICENSE`); its authors are listed in `AUTHORS`. It contains no data from any game or mod:
you need your own copy.

Battlefield 1942 and Refractor are trademarks of Electronic Arts / DICE. Bytefield is not affiliated with them.
Rights holders: **revival-forge@proton.me**; we answer within days.

## Ported code
| What | From | Licence | In Bytefield |
|---|---|---|---|
| PKWARE DCL "implode" decoder | zlib's `contrib/blast/blast.c`, Mark Adler | zlib | `crates/extract/src/blast.rs` |
| LZO1X decompressor | minilzo (`lzo1x_d.ch`), Markus F.X.J. Oberhumer | GPL-2.0-or-later | `crates/vfs/src/lzo.rs` |
| `.tm` (tree mesh) reader | OpenField42, `tree_mesh.cpp` | GPL-3 | `crates/formats/src/tm.rs` |

### zlib notice (blast.c)
> Copyright (C) 2003, 2012, 2013 Mark Adler.
> This software is provided 'as-is', without any express or implied warranty. In no event will the author be held
> liable for any damages arising from the use of this software. Permission is granted to anyone to use this software
> for any purpose, including commercial applications, and to alter it and redistribute it freely, subject to the
> following restrictions: 1. The origin of this software must not be misrepresented; you must not claim that you
> wrote the original software. If you use this software in a product, an acknowledgment in the product documentation
> would be appreciated but is not required. 2. Altered source versions must be plainly marked as such, and must not
> be misrepresented as being the original software. 3. This notice may not be removed or altered from any source
> distribution.

## Formats and references consulted
| What | Licence | What for |
|---|---|---|
| OpenField42 (librfa, mesh and terrain readers, render values) | GPL-3 | The `.rfa` format (`crates/vfs/src/rfa.rs`), terrain, rendering |
| RefractorForge (luccawulf) | GPL-3 | The `.con` script dialect (`crates/con-lang`) |
| BF1942 Blender add-on and BSP-tree-generator (Ahrkylien) | MIT | The `.sm` format (`crates/formats/src/sm.rs`) |
| BF1942-Extraction-Readout-Scripts (Ahrkylien) | none | Read only; no code copied |

## Inspiration
OpenMW (a free engine that reads the original data) and its `dummy-content` by Mudcrab (synthetic test data).

Bot techniques, ideas only (no code or text taken; described in our own words in the code):
- *Game AI Pro* (ed. Steve Rabin), chapter 26, "Tactical Position Selection", Matthew Jack (Crytek): the position query of
  the bots (`crates/sim/src/bots/tactics.rs`).
- *Game AI Pro 2*, chapter 34, "Human Enemy AI in The Last of Us", Travis McIntosh (Naughty Dog): firing spots from cover
  and the flanker chosen by the combat vector.
- *Game AI Pro*, chapter 29, "Hierarchical AI for Multiplayer Bots in Killzone 3", Remco Straatman, Tim Verweij, Alex
  Champandard, Robert Morcus and Hylke Kleve (Guerrilla); *Game AI Pro 2*, chapter 30, "Modular Tactical Influence Maps", Dave
  Mark; *Game AI Pro 3*, chapter 24, "Being Where It Counts: Telling Paragon Bots Where to Go", Mieszko Zieliński (Epic Games):
  what a side knows of the enemy (`crates/sim/src/bots/intel.rs`) and the split of the side by priority and need.
- *Game AI Pro*, chapter 3, "Advanced Randomness Techniques for Game AI", Steve Rabin, section on filtered randomness: draws
  without streaks (`crates/sim/src/bots/streetwise.rs`).
- *Game AI Pro 2*, chapter 2, "Combat Dialogue in F.E.A.R.", Jeff Orkin (Monolith): two-line exchanges between bots that say
  what they do.

## Compatible content (not included, not redistributed)
- Battlefield 1942, The Road to Rome and Secret Weapons of WWII (DICE / EA).
- Desert Combat (Trauma Studios) and the volunteers' Desert Combat Final.
- Galactic Conquest (the Galactic Conquest team; maintained by Ahrkylien). Its licence forbids redistributing it.
- High Definition Remaster (its authors, on [ModDB](https://www.moddb.com/mods/high-definition-remaster)): used as a
  pack over the player's install, never copied into it.

## Dependencies
The Rust crates and their licences, generated: `THIRD_PARTY.md`. All of them are compatible with GPL-3.0
(`cargo deny check licenses`, `deny.toml`). Worth naming apart because they carry C code or were chosen on purpose:
`mimalloc` (Microsoft, MIT), the client's allocator; `rerecast` (Jan Hohenheim, MIT or Apache-2.0), the Rust port of
Mikko Mononen's Recast (recastnavigation, zlib) that builds the bots' walkable-surface navigation mesh
(`crates/sim/src/navmesh`).
