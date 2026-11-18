//! A level's static world: terrain and placed objects, built from the template database.
//!
//! Coordinates: Refractor is Y-up; a position is `x/y/z` in metres, with the terrain spanning
//! 0..worldSize in x and z. How the heightmap's rows/columns and the colour tiles map onto x/z
//! is not assumed: [`Orientation`] values are checked against the data (see the `topdown` tool).

pub mod scene;
pub mod shadow;
pub mod tree;

use bytefield_con::templates::TemplateDb;
use bytefield_formats::dds;
use bytefield_formats::image::{Image, Orientation};
use bytefield_formats::terrain::{Heightmap, TerrainParams};
use bytefield_vfs::Vfs;

/// A `x/y/z` vector (any number of parts). Each part is read like the original's `atof`: its numeric prefix, so junk
/// after a number (`0.0884857s` in Tobruk, a backtick in Battle_of_Britain of DC_Final: 35 such lines in DC, vanilla
/// and GC) is dropped and the number kept. A part with no number at all (`objects/x`) is `None`,
/// which callers use to tell a vector from a text.
pub fn parse_vec(s: &str) -> Option<Vec<f64>> {
    s.split('/').map(|p| atof_prefix(p.trim())).collect()
}

/// The number at the start of `s`, or `None` if it does not start with one (sign, digits, point, exponent).
pub fn atof_prefix(s: &str) -> Option<f64> {
    if let Ok(n) = s.parse::<f64>() {
        return Some(n);
    }
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - start
    };
    let mut n = digits(&mut i);
    if i < b.len() && b[i] == b'.' {
        i += 1;
        n += digits(&mut i);
    }
    if n == 0 {
        return None;
    }
    let mantissa = i;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if digits(&mut j) > 0 {
            i = j;
        }
    }
    s[..i].parse::<f64>().ok().or_else(|| s[..mantissa].parse::<f64>().ok())
}

#[derive(Debug, Clone)]
pub struct Placed {
    pub template: String,
    pub kind: Option<String>,
    pub pos: [f64; 3],
}

/// An `ObjectSpawner` placed in the level: what it makes for each team and how.
#[derive(Debug, Clone)]
pub struct Spawner {
    /// the spawner's own template (`lighttankspawner`)
    pub name: String,
    /// `Object.setName`: the instance's own name
    pub label: Option<String>,
    pub pos: [f64; 3],
    /// `Object.rotation` (yaw, pitch, roll, degrees): the spawned object gets it
    pub rotation: [f64; 3],
    /// `Object.setTeam` (0: none)
    pub team: u32,
    /// `Object.setOSId`: the control point (`objectSpawnerId`) whose team it follows
    pub os_id: Option<i64>,
    /// `setObjectTemplate <team> <template>`
    pub templates: Vec<(u32, String)>,
    /// `spawnOffset`, added to the position in the world frame (not turned)
    pub offset: [f64; 3],
    /// `spawnDelayAtStart`: wait a spawn delay before the first object (0: at once)
    pub delay_at_start: bool,
    /// `minSpawnDelay`, `maxSpawnDelay`, `timeToLive`, `distance` (engine defaults 30, 60, 30, 100)
    pub min_delay: f64,
    pub max_delay: f64,
    pub time_to_live: f64,
    pub distance: f64,
    /// `damageWhenLost` (1): what an abandoned object loses a second once its time is up
    pub damage_when_lost: f64,
    /// `holdObject`
    pub hold_object: bool,
    /// `teamOnVehicle`: the object it makes is of its team (`setTeam`); else of none
    /// until someone gets in (the battleships, destroyers and carriers have it)
    pub team_on_vehicle: bool,
    /// `maxNrOfObjectSpawned` (1)
    pub max_objects: usize,
    /// `radius` (5): a wreck this close is cleared when the next object comes; an object
    /// farther than this counts as gone (its countdown runs)
    pub radius: f64,
    /// a spawner in a placed static's tree (`Armory_Mp5` ∋ `Mp5_Spawner`, DC): the static's
    /// template; its position and rotation are then the composed ones (in the world)
    pub in_static: Option<String>,
}

impl Spawner {
    /// The template it makes for its current team.
    pub fn template(&self) -> Option<&str> {
        self.templates.iter().rev().find(|(t, _)| *t == self.team).map(|(_, n)| n.as_str())
    }
}

pub struct Level {
    pub name: String,
    pub terrain: TerrainParams,
    pub heightmap: Heightmap,
    /// x, z, width, height (`game.setActiveCombatArea`)
    pub combat_area: Option<[f64; 4]>,
    pub placed: Vec<Placed>,
    /// the `ObjectSpawner`s, in the order the scripts create them
    pub spawners: Vec<Spawner>,
    /// `materialMap` (`<name>.raw`): one material id per byte, `materialSize`² cells, row = z
    pub material_map: Option<(usize, Vec<u8>)>,
}

fn arg<'a>(t: &'a bytefield_con::templates::Template, name: &str) -> Option<&'a str> {
    t.get(name).and_then(|a| a.first()).map(String::as_str)
}

/// The level's `ObjectSpawner`s, in the order the scripts create them: the placed ones and those
/// in a placed object's tree (a static's: the server registers a spawner in its `init`, wherever
/// it is). A placed spawner without `Object.setTeam` takes the team of the control point
/// whose `objectSpawnerId` is its `setOSId` (HYPOTHESIS: the control point hands its team to its
/// spawners; Berlin sets both and they agree); one in a tree, its template's `team`.
pub fn spawners(db: &TemplateDb) -> Vec<Spawner> {
    let last = |i: &bytefield_con::templates::Instance, n: &str| i.props.iter().rev().find(|p| p.name == n).and_then(|p| p.args.first()).cloned();
    let vec3 = |s: Option<String>| s.and_then(|s| parse_vec(&s)).filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]);
    // control points: objectSpawnerId -> team (their placed instances)
    let cp_team = |os: i64| {
        db.instances.iter().filter_map(|i| db.object_template(&i.template)).find(|t| {
            t.kind.eq_ignore_ascii_case("controlpoint") && arg(t, "objectSpawnerId").and_then(|v| v.parse::<i64>().ok()) == Some(os)
        }).and_then(|t| arg(t, "team")).and_then(|v| v.parse().ok())
    };
    let mut out = Vec::new();
    for i in &db.instances {
        let Some(t) = db.object_template(&i.template) else { continue };
        let Some(pos) = vec3(last(i, "absoluteposition")) else { continue };
        let rot = vec3(last(i, "rotation").or_else(|| last(i, "setrotation"))).unwrap_or([0.0; 3]);
        if t.kind.eq_ignore_ascii_case("objectspawner") {
            let os_id = last(i, "setosid").and_then(|v| v.parse().ok());
            let team = last(i, "setteam")
                .and_then(|v| v.parse().ok())
                .or_else(|| os_id.and_then(cp_team))
                .or_else(|| arg(t, "team").and_then(|v| v.parse().ok()))
                .unwrap_or(0);
            let mut s = spawner_from(t, pos, rot, team, os_id);
            s.label = last(i, "setname");
            // Setting the team of a spawner while the level loads (game state 3) leaves the countdown at −1: the
            // constructor's `calcSpawnDelay` of `spawnDelayAtStart` is undone, and the first object comes at once
            // (DC_Cornered's T72, BM21, BMP2, SCUD-B and MLRS).
            // TODO: a later round's `restartMap` resets the spawner and does apply it (`calcSpawnDelay` when active
            // and `spawnDelayAtStart`); the bench starts every round as the first.
            if last(i, "setteam").is_some() {
                s.delay_at_start = false;
            }
            out.push(s);
            continue;
        }
        let placed = scene::Mat4::from_pos_ypr(pos, rot);
        for (sp, m) in child_spawners(db, &i.template) {
            let w = placed.mul(&m);
            out.push(Spawner { pos: w.transform_point([0.0; 3]), rotation: w.ypr(), in_static: Some(i.template.clone()), ..sp });
        }
    }
    out
}

/// An `ObjectSpawner` of template `t` at `pos`, turned by `rotation`, of `team`.
pub fn spawner_from(t: &bytefield_con::templates::Template, pos: [f64; 3], rotation: [f64; 3], team: u32, os_id: Option<i64>) -> Spawner {
    let num = |n: &str, d: f64| arg(t, n).and_then(|v| v.parse().ok()).unwrap_or(d);
    // `setSpawnDelay x` sets both bounds (Midway's ships), so the lines apply in order
    let (mut min_delay, mut max_delay) = (30.0, 60.0);
    for p in &t.props {
        let Some(v) = p.args.first().and_then(|v| v.parse::<f64>().ok()) else { continue };
        match p.name.strip_prefix("set").unwrap_or(&p.name) {
            "minspawndelay" => min_delay = v,
            "maxspawndelay" => max_delay = v,
            "spawndelay" => (min_delay, max_delay) = (v, v),
            _ => {}
        }
    }
    Spawner {
        name: t.name.clone(),
        label: None,
        pos,
        rotation,
        team,
        os_id,
        templates: t.all("setobjecttemplate").iter().filter_map(|a| Some((a.first()?.parse().ok()?, a.get(1)?.clone()))).collect(),
        offset: arg(t, "spawnOffset").and_then(parse_vec).filter(|v| v.len() == 3).map_or([0.0; 3], |v| [v[0], v[1], v[2]]),
        delay_at_start: arg(t, "spawnDelayAtStart").is_some_and(|v| v.trim() == "1"),
        min_delay,
        max_delay,
        time_to_live: num("timeToLive", 30.0),
        distance: num("distance", 100.0),
        damage_when_lost: num("damageWhenLost", 1.0),
        hold_object: arg(t, "holdObject").is_some_and(|v| v.trim() == "1"),
        team_on_vehicle: arg(t, "teamOnVehicle").is_some_and(|v| v.trim() == "1"),
        max_objects: num("maxNrOfObjectSpawned", 1.0).max(1.0) as usize,
        radius: num("radius", 5.0),
        in_static: None,
    }
}

/// The `ObjectSpawner`s in a template's tree (a ship's: its planes and boats; the tree the server
/// creates, [`tree::instantiate`]), each with its matrix in the root's frame; their team is the
/// template's `team` (0: none).
pub fn child_spawners(db: &TemplateDb, template: &str) -> Vec<(Spawner, scene::Mat4)> {
    tree::of_kind(db, template, "objectspawner")
        .into_iter()
        .filter_map(|(name, m)| {
            let t = db.object_template(&name)?;
            let team = arg(t, "team").and_then(|v| v.parse().ok()).unwrap_or(0);
            Some((spawner_from(t, [0.0; 3], [0.0; 3], team, None), m))
        })
        .collect()
}

/// The `SpawnPoint`s in template `template`'s tree,
/// each with its matrix in the template's frame.
pub fn child_spawn_points(db: &TemplateDb, template: &str) -> Vec<(String, scene::Mat4)> {
    child_templates_of_kind(db, template, "spawnpoint")
}

/// The templates of kind `kind` (`SpawnPoint`, `SupplyDepot`) in template `template`'s tree (the
/// one the server creates: none under a leaf such as a `SimpleObject`, and an `ObjectSpawner`'s
/// objects are not in it), each with its matrix in the template's frame.
pub fn child_templates_of_kind(db: &TemplateDb, template: &str, kind: &str) -> Vec<(String, scene::Mat4)> {
    tree::of_kind(db, template, &kind.to_ascii_lowercase())
}

impl Level {
    pub fn from_db(vfs: &Vfs, db: &TemplateDb, name: &str) -> Result<Level, String> {
        let pt = db
            .templates
            .values()
            .filter(|t| t.namespace == "geometrytemplate" && t.kind.eq_ignore_ascii_case("patchterrain"))
            .max_by_key(|t| t.origin.contains(&format!("levels/{}", name.to_lowercase())))
            .ok_or("no PatchTerrain template")?;
        let num = |n: &str, default: f64| arg(pt, n).and_then(|v| v.parse().ok()).unwrap_or(default);
        let terrain = TerrainParams {
            heightmap: arg(pt, "file").ok_or("terrain has no file")?.replace('\\', "/"),
            world_size: num("worldSize", 2048.0),
            y_scale: num("yScale", 1.0),
            water_level: num("waterLevel", 0.0),
            tex_base: arg(pt, "texBaseName").unwrap_or("").replace('\\', "/"),
            detail_tex: arg(pt, "detailTexName").unwrap_or("").replace('\\', "/"),
            tex_offset: [num("texOffsetX", 0.0), num("texOffsetY", 0.0)],
            sea_floor: num("seaFloorLevel", 5.0),
        };
        let raw = vfs.read(&format!("{}.raw", terrain.heightmap)).map_err(|e| format!("{}.raw: {e}", terrain.heightmap))?;
        let heightmap = Heightmap::from_raw(&raw).ok_or("heightmap is not square")?;
        let combat_area = db
            .globals
            .iter()
            .rev()
            .find(|c| c.command == "game.setactivecombatarea")
            .and_then(|c| c.args.iter().map(|a| a.parse::<f64>().ok()).collect::<Option<Vec<_>>>())
            .and_then(|v| v.try_into().ok());
        let placed = db
            .instances
            .iter()
            .filter_map(|i| {
                let p = i.props.iter().rev().find(|p| p.name == "absoluteposition")?;
                let v = parse_vec(p.args.first()?)?;
                (v.len() == 3).then(|| Placed {
                    template: i.template.clone(),
                    kind: db.object_template(&i.template).map(|t| t.kind.clone()),
                    pos: [v[0], v[1], v[2]],
                })
            })
            .collect();
        let spawners = spawners(db);
        let material_map = arg(pt, "materialMap").and_then(|m| {
            let bytes = vfs.read(&format!("{}.raw", m.replace('\\', "/"))).ok()?;
            let size = arg(pt, "materialSize").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            (size > 0 && bytes.len() >= size * size).then_some((size, bytes))
        });
        Ok(Level { name: name.to_string(), terrain, heightmap, combat_area, placed, spawners, material_map })
    }

    /// The terrain material at (x, z): the cell of the material map
    /// that holds it (the coordinates are truncated: checked against the server), 0 outside the
    /// map or without one.
    pub fn material_at(&self, x: f64, z: f64) -> u32 {
        let Some((n, bytes)) = &self.material_map else { return 0 };
        let ws = self.terrain.world_size;
        if !(0.0..=ws).contains(&x) || !(0.0..=ws).contains(&z) {
            return 0;
        }
        let s = (*n as f32 / ws as f32) as f64;
        let (c, r) = ((x * s) as i64, (z * s) as i64);
        let i = (r * *n as i64 + c).clamp(0, (n * n) as i64 - 1) as usize;
        (bytes[i] & 0xf) as u32
    }

    /// Terrain height at world (x, z), bilinear, with the heightmap laid down by `o`
    /// (identity: column = x, row = z).
    /// The terrain's height at (x, z), as the engine computes it,
    /// matched against the original game to 1e-5 m: cells of worldSize / n with heightmap
    /// node (c, r) at (c·cell, r·cell), each cell split in two triangles along the diagonal
    /// (c, r)–(c+1, r+1) when c + r is even and along the other one when it is odd.
    pub fn height_at(&self, x: f64, z: f64, o: Orientation) -> Option<f64> {
        self.terrain_triangle(x, z, o).map(|(_, _, h)| h)
    }

    /// Height and the (unit, upward) normal of the terrain triangle under (x, z).
    pub fn height_and_normal(&self, x: f64, z: f64, o: Orientation) -> Option<(f64, [f64; 3])> {
        let (sx, sz, h) = self.terrain_triangle(x, z, o)?;
        let cell = self.terrain.world_size / self.heightmap.size as f64;
        let (gx, gz) = (sx / cell, sz / cell);
        let l = (gx * gx + gz * gz + 1.0).sqrt();
        Some((h, [-gx / l, 1.0 / l, -gz / l]))
    }

    /// The triangle under (x, z): its slopes along x and z (per cell), and the height there. (Apart from the normal: the
    /// AI's lines of sight ask only the height, every 2 m.)
    fn terrain_triangle(&self, x: f64, z: f64, o: Orientation) -> Option<(f64, f64, f64)> {
        let n = self.heightmap.size;
        let cell = self.terrain.world_size / n as f64;
        let (u, v) = (x / cell, z / cell);
        if !(0.0..=(n - 1) as f64).contains(&u) || !(0.0..=(n - 1) as f64).contains(&v) {
            return None;
        }
        let sample = |c: usize, r: usize| {
            let (mut c, mut r) = if o.transpose { (r, c) } else { (c, r) };
            if o.flip_x {
                c = n - 1 - c;
            }
            if o.flip_y {
                r = n - 1 - r;
            }
            self.terrain.height_m(self.heightmap.at(c.min(n - 1), r.min(n - 1)))
        };
        // u and v are >= 0 (checked above), so truncation is the floor: `floor` would call libm's software one (no SSE4.1)
        let (c, r) = ((u as usize).min(n - 2), (v as usize).min(n - 2));
        let (fu, fv) = (u - c as f64, v - r as f64);
        let (a, b, d0, d) = (sample(c, r), sample(c + 1, r), sample(c, r + 1), sample(c + 1, r + 1));
        // slopes along x and z of the triangle (per cell), then the height
        // on a diagonal both triangles give the same height; the engine takes the second one there
        let (sx, sz, h) = if (c + r) % 2 == 0 {
            if fu > fv { (b - a, d - b, a + (b - a) * fu + (d - b) * fv) } else { (d - d0, d0 - a, a + (d - d0) * fu + (d0 - a) * fv) }
        } else if fu + fv < 1.0 {
            (b - a, d0 - a, a + (b - a) * fu + (d0 - a) * fv)
        } else {
            (d - d0, d - b, d + (d0 - d) * (1.0 - fu) + (b - d) * (1.0 - fv))
        };
        // a height of exactly 0 is the sea bed (`seaFloorLevel`); the normal stays the triangle's
        Some((sx, sz, if h == 0.0 { self.terrain.sea_floor } else { h }))
    }

    /// The terrain's height and normal at (x, z) the way the engine computes them, to the bit:
    /// the cell from the truncated
    /// x / cell and z / cell (heightmap indices wrap, as the height map masks them), the
    /// triangle's interpolation of the raw samples, and only then the scale (yScale / 256, as a
    /// float); the normal is the triangle's cross product in floats, normalised. Exactly 0 is the
    /// sea bed. Only for the identity orientation (the engine's).
    pub fn engine_height_and_normal(&self, x: f32, z: f32) -> (f32, [f32; 3]) {
        let (h, n) = self.engine_height_and_normal_fpu(x, z);
        (h as f32, n)
    }

    /// The same with the height as the FPU leaves it (before it is stored as a float; the sea bed
    /// as its float).
    pub fn engine_height_and_normal_fpu(&self, x: f32, z: f32) -> (f64, [f32; 3]) {
        let n = self.heightmap.size as u32;
        let mask = n - 1;
        let cell = (self.terrain.world_size as u32 / n) as f32;
        let inv = 1.0f32 / cell;
        let k = (self.terrain.y_scale as f32 as f64 * 0.003_906_25) as f32;
        let (fx, fz) = (x as f64 * inv as f64, inv as f64 * z as f64);
        let (c, r) = (fx as i64 as u32, fz as i64 as u32);
        let (fu, fv) = (fx - c as i32 as f64, fz - r as i32 as f64);
        let s = |c: u32, r: u32| self.heightmap.at((c & mask) as usize, (r & mask) as usize) as f64;
        // (c, r just under 0 are u32::MAX: the next one wraps to 0, and the mask makes it the map's other side)
        let f = |v: f64| v as f32;
        let (base, sum, n3) = if (c & 1) == (r & 1) {
            if fv >= fu {
                let base = s(c, r.wrapping_add(1));
                let (a, b) = (s(c.wrapping_add(1), r.wrapping_add(1)) - base, s(c, r) - base);
                let (da, db) = (f(a * k as f64), f(b * k as f64));
                (base, a * fu + b * (1.0 - fv), [f(-(cell as f64) * da as f64), f(cell as f64 * cell as f64), f(db as f64 * cell as f64)])
            } else {
                let base = s(c.wrapping_add(1), r);
                let (a, b) = (s(c, r) - base, s(c.wrapping_add(1), r.wrapping_add(1)) - base);
                let (da, db) = (f(a * k as f64), f(b * k as f64));
                (base, a * (1.0 - fu) + b * fv, [f(cell as f64 * da as f64), f(cell as f64 * cell as f64), f(db as f64 * -(cell as f64))])
            }
        } else if fv < 1.0 - fu {
            let base = s(c, r);
            let (a, b) = (s(c.wrapping_add(1), r) - base, s(c, r.wrapping_add(1)) - base);
            let (da, db) = (f(a * k as f64), f(b * k as f64));
            (base, a * fu + b * fv, [f(-(cell as f64) * da as f64), f(cell as f64 * cell as f64), f(-(cell as f64) * db as f64)])
        } else {
            // on the diagonal, the second triangle
            let base = s(c.wrapping_add(1), r.wrapping_add(1));
            let (a, b) = (s(c, r.wrapping_add(1)) - base, s(c.wrapping_add(1), r) - base);
            let (da, db) = (f(a * k as f64), f(b * k as f64));
            (base, a * (1.0 - fu) + b * (1.0 - fv), [f(cell as f64 * da as f64), f(cell as f64 * cell as f64), f(cell as f64 * db as f64)])
        };
        let h = (sum + base) * k as f64;
        let h = if h == 0.0 { self.terrain.sea_floor as f32 as f64 } else { h };
        // normalised
        let [nx, ny, nz] = n3.map(|v| v as f64);
        let l2 = nz * nz + ny * ny + nx * nx;
        let normal = if (l2 - 1.0).abs() < f32::from_bits(0x3400_0000) as f64 {
            n3
        } else {
            let il = 1.0 / l2.sqrt();
            [(nx * il) as f32, (ny * il) as f32, (nz * il) as f32]
        };
        (h, normal)
    }

    /// The colour tiles `<texBase>XXxYY.dds` as one image, tile (XX, YY) at column XX, row YY,
    /// each scaled to `tile_px`. The grid is square, as large as the highest index found (a
    /// level may ship only some tiles: Berlin has 06..07 of 8, Tobruk 6 rows of 8); its tiles
    /// split worldSize evenly. Missing tiles stay transparent. Returns the image and the grid.
    pub fn colour_mosaic(&self, vfs: &Vfs, tile_px: usize) -> Option<(Image, usize)> {
        let base = &self.terrain.tex_base;
        let (found, grid) = self.colour_tiles(vfs)?;
        let mut img = Image::new(grid * tile_px, grid * tile_px);
        // (the tiles it lacks, the level's default if it has one: Philippines and Caen, the sea around)
        if let Some(tile) = vfs.read(&format!("{}terrainDefault.dds", base.trim_end_matches(|c: char| c != '/' && c != '\\'))).ok().and_then(|b| dds::decode(&b).ok()) {
            let tile = tile.resize(tile_px, tile_px);
            for (xx, yy) in (0..grid).flat_map(|xx| (0..grid).map(move |yy| (xx, yy))).filter(|c| !found.contains(c)) {
                img.blit(&tile, xx * tile_px, yy * tile_px);
            }
        }
        for (xx, yy) in found.into_iter().filter(|&(x, y)| x < grid && y < grid) {
            if let Ok(bytes) = vfs.read(&format!("{base}{xx:02}x{yy:02}.dds"))
                && let Ok(tile) = dds::decode(&bytes)
            {
                img.blit(&tile.resize(tile_px, tile_px), xx * tile_px, yy * tile_px);
            }
        }
        Some((img, grid))
    }

    /// The colour tiles there are, (XX, YY) of `<texBase>XXxYY.dds`, and the side of the square grid they make
    /// (`colour_mosaic`).
    pub fn colour_tiles(&self, vfs: &Vfs) -> Option<(Vec<(usize, usize)>, usize)> {
        let base = &self.terrain.tex_base;
        let found: Vec<(usize, usize)> = (0..32)
            .flat_map(|xx| (0..32).map(move |yy| (xx, yy)))
            .filter(|(xx, yy)| vfs.exists(&format!("{base}{xx:02}x{yy:02}.dds")))
            .collect();
        let grid = found.iter().map(|&(x, y)| x.max(y) + 1).max()?;
        Some((found, tile_grid(grid, self.terrain.world_size, self.terrain.tex_offset[0])))
    }

    /// Hill-shaded relief of the heightmap (file order), water in blue.
    pub fn relief(&self) -> Image {
        let n = self.heightmap.size;
        let mut img = Image::new(n, n);
        let cell = self.terrain.world_size / n as f64;
        for r in 0..n {
            for c in 0..n {
                let h = self.terrain.height_m(self.heightmap.at(c, r));
                let hx = self.terrain.height_m(self.heightmap.at((c + 1).min(n - 1), r)) - h;
                let hz = self.terrain.height_m(self.heightmap.at(c, (r + 1).min(n - 1))) - h;
                let shade = (0.5 + (-hx - hz) / cell * 1.5).clamp(0.0, 1.0);
                let p = if h < self.terrain.water_level {
                    [30, 60, (120.0 + 80.0 * shade) as u8, 255]
                } else {
                    let g = (60.0 + 180.0 * shade) as u8;
                    [g, g, g, 255]
                };
                img.put(c, r, p);
            }
        }
        img
    }
}

/// How many tiles a side has, given the last one found: a tile measures a power of two of metres (256 in all of bf1942's)
/// and the first or last rows may be missing, all sea (Philippines and Caen: 01..06 of 8 in 2048; taking 7, each tile stretched to 292 m).
fn tile_grid(last: usize, world_size: f64, offset_x: f64) -> usize {
    let tile = world_size / (last as f64 + 2.0 * offset_x);
    // (with some slack: Kbely has a ninth row and column past the edge of its 8 × 256 m)
    let tile = 2f64.powf((tile.log2() + 0.25).floor());
    (world_size / tile - 2.0 * offset_x).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vector_keeps_the_number_before_junk() {
        // Tobruk and Battle_of_Britain of DC_Final
        assert_eq!(parse_vec("-106.236/0/0.0884857s"), Some(vec![-106.236, 0.0, 0.0884857]));
        assert_eq!(parse_vec("1197.97/109.181/1315.69`"), Some(vec![1197.97, 109.181, 1315.69]));
        assert_eq!(parse_vec("180.0/0.0/0.0`"), Some(vec![180.0, 0.0, 0.0]));
        assert_eq!(parse_vec("1e3/-.5/2.e-1x/3e"), Some(vec![1000.0, -0.5, 0.2, 3.0]));
        // a text stays a text
        assert_eq!(parse_vec("objects/vehicles"), None);
        assert_eq!(parse_vec("1/x"), None);
        assert_eq!(parse_vec(""), None);
    }

    #[test]
    fn a_tile_measures_a_power_of_two() {
        assert_eq!(tile_grid(7, 2048.0, 0.0), 8);
        assert_eq!(tile_grid(8, 2048.0, 0.0), 8);
        assert_eq!(tile_grid(9, 2048.0, 0.0), 8);
        assert_eq!(tile_grid(4, 2048.0, 2.0), 4);
        assert_eq!(tile_grid(4, 1024.0, 0.0), 4);
        assert_eq!(tile_grid(8, 4096.0, 4.0), 8);
        assert_eq!(tile_grid(4, 4096.0, 6.0), 4);
    }

    struct Mem(&'static str);
    impl bytefield_con::ScriptFs for Mem {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            (path == "init.con").then(|| self.0.as_bytes().to_vec())
        }
    }

    #[test]
    fn spawners_pick_their_team_template() {
        let script = "ObjectTemplate.create ObjectSpawner tankspawner\n\
            ObjectTemplate.setObjectTemplate 2 T34\n\
            ObjectTemplate.setObjectTemplate 1 Tiger\n\
            ObjectTemplate.spawnOffset 0/1/0\n\
            ObjectTemplate.create ControlPoint flag\n\
            ObjectTemplate.objectSpawnerId 3\n\
            ObjectTemplate.team 2\n\
            Object.create flag\n\
            Object.absolutePosition 0/0/0\n\
            Object.create tankspawner\n\
            Object.absolutePosition 10/20/30\n\
            Object.rotation 90/0/0\n\
            Object.setTeam 1\n\
            Object.create tankspawner\n\
            Object.absolutePosition 1/2/3\n\
            Object.setOSId 3\n\
            Object.create tankspawner\n\
            Object.absolutePosition 4/5/6\n";
        let fs = Mem(script);
        let mut it = bytefield_con::Interpreter::new(&fs, bytefield_con::templates::TemplateDb::default());
        it.run_file("init.con", &[]);
        let s = spawners(&it.host);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].team, s[0].template()), (1, Some("Tiger")));
        assert_eq!((s[0].pos, s[0].rotation, s[0].offset), ([10.0, 20.0, 30.0], [90.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
        assert_eq!((s[1].team, s[1].template()), (2, Some("T34"))); // its flag's team
        assert_eq!((s[2].team, s[2].template()), (0, None)); // nobody's: makes nothing
        assert!(!s[0].delay_at_start && s[0].min_delay == 30.0 && s[0].max_delay == 60.0);
    }

    #[test]
    fn spawn_delay_sets_both_bounds_in_order() {
        let script = "ObjectTemplate.create ObjectSpawner ship\n\
            ObjectTemplate.setObjectTemplate 1 Yamato\n\
            ObjectTemplate.SpawnDelay 300\n\
            ObjectTemplate.create ObjectSpawner tank\n\
            ObjectTemplate.setObjectTemplate 1 Tiger\n\
            ObjectTemplate.SpawnDelay 300\n\
            ObjectTemplate.minSpawnDelay 70\n\
            Object.create ship\n\
            Object.absolutePosition 0/0/0\n\
            Object.setTeam 1\n\
            Object.create tank\n\
            Object.absolutePosition 0/0/0\n\
            Object.setTeam 1\n";
        let fs = Mem(script);
        let mut it = bytefield_con::Interpreter::new(&fs, bytefield_con::templates::TemplateDb::default());
        it.run_file("init.con", &[]);
        let s = spawners(&it.host);
        assert_eq!((s[0].min_delay, s[0].max_delay), (300.0, 300.0));
        assert_eq!((s[1].min_delay, s[1].max_delay), (70.0, 300.0));
    }
}
