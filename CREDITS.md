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

## Compatible content (not included, not redistributed)
- Battlefield 1942, The Road to Rome and Secret Weapons of WWII (DICE / EA).
- Desert Combat (Trauma Studios) and the volunteers' Desert Combat Final.
- Galactic Conquest (the Galactic Conquest team; maintained by Ahrkylien). Its licence forbids redistributing it.
- High Definition Remaster (its authors, on [ModDB](https://www.moddb.com/mods/high-definition-remaster)): used as a
  pack over the player's install, never copied into it.

## Dependencies
The Rust crates and their licences, generated: `THIRD_PARTY.md`. All of them are compatible with GPL-3.0
(`cargo deny check licenses`, `deny.toml`). Worth naming apart because they carry C code or were chosen on purpose:
`mimalloc` (Microsoft, MIT), the client's allocator.
