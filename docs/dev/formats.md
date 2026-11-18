# The data formats

bytefield reads the files of an installed Battlefield 1942 (version 1.6 or 1.61) and of its mods as they are; it never
needs them unpacked or converted. All readers take bytes (the file system supplies them) and have no window or GPU
dependency, so they can be tested on their own. Each reader's module starts with the layout it reads and its sources.

| File | What it holds | Reader |
|---|---|---|
| `.rfa` | Archives: every other file lives in one. A directory of entries, each stored or LZO1X-compressed. A mod's archives are searched in its `game.addModPath` order, and a patch archive (`Name_003.rfa`) overrides an earlier one (`Name.rfa`) | `vfs::rfa`, `vfs::lzo` |
| `.con`, `.inc` | Scripts: templates, levels, game modes, settings (see [con-dialect.md](con-dialect.md)) | `con-lang` |
| `.dds` | Textures: DXT1/3/5 and uncompressed, with their mip chains | `formats::dds` |
| `.tga` | HUD and menu images, some lightmaps: true colour, grey and indexed, raw or run-length | `formats::tga` |
| `.sm` | StandardMesh: the visible levels of detail, the collision meshes (with a material per face and per vertex) and the shadow meshes | `formats::sm` |
| `.rs` | The material of each mesh's submesh: textures, transparency, lighting flags | `formats::rs` |
| `.tm` | TreeMesh: trees, with their billboard sprites | `formats::tm` |
| `Heightmap.raw` and `Terrain.con` | The terrain: a 16-bit heightmap and its scale, size and water level | `formats::terrain` |
| `LightmapShadowBits.lsb` | The terrain's baked shadow, one tile per colour tile, run-length encoded | `formats::lsb` |
| `.ske`, `.baf`, `.skn` | Skeletons, bone animations and skins of the soldiers | `formats::anim` |
| `.ssc` | Sound scripts: which samples an object plays, layered and pitched by its state | `formats::ssc` |
| `lexiconAll.dat` | The mod's texts in every language (UTF-16) | `formats::lexicon` |
| `Pathfinding/*.raw` | The bots' pathfinding maps that the levels ship with | `formats::pathmap` |

Coordinates are as Refractor stores them: metres, Y up, the terrain spanning 0 to its world size in x and z. How
heightmap rows and texture tiles map onto x and z is checked against the data, not assumed.

## Credits

The layouts come from the community's documentation and free projects, credited in `CREDITS.md` and at the top of
each reader: OpenField42 (GPL-3) for archives, tree meshes and terrain; ahrkylien's Blender add-on (MIT) for the
standard mesh; and measurement against the game's own files for the rest.
