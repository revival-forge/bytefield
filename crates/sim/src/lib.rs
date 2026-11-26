//! The simulation core: a level's static world plus things that move, stepped at a fixed rate,
//! deterministically (same inputs, same results, bit for bit), with no window.
//!
//! What exists now is the frame the physics will grow in:
//! - [`World`]: the level (terrain, placed objects, templates) loaded through the same path as
//!   every other tool (`bytefield_con::session`)
//! - [`vehicle`]: the component tree of a vehicle, read from its templates
//! - [`Sim`]: point bodies moved the way the engine moves projectiles, with
//!   terrain/water contact, and vehicles ([`mover::Vehicle`]: rigid bodies with engines, wings,
//!   floats, springs and wheels) against the terrain and the level's objects.
//!
//! Units: metres, seconds, kilograms; Refractor's frame (left-handed, Y up).

pub mod check;
pub mod debug;
pub mod vecmath;
mod no_better_bots;
pub mod dumb;
pub mod quality;
pub mod round_events;
pub mod input;
pub mod anim;
pub mod bot_api;
pub mod bots;
pub mod conquest;
pub mod contact;
pub mod cover;
pub mod craft;
pub mod create;
pub mod ctf;
pub mod objective;
pub mod damage;
pub mod clock;
pub mod journal;
pub mod modes;
pub use journal::{LogBase, log_since};
pub mod entry;
pub mod explosives;
pub mod firearms;
pub mod kit;
pub mod kitpart;
pub mod ladder;
pub mod mover;
pub mod navgen;
pub mod navmesh;
pub mod obstacle;
pub mod players;
pub mod portal;
pub mod projectile;
pub mod radio;
pub mod ref2;
pub mod rigid;
pub mod scout;
pub mod sonar;
pub mod replica;
pub mod round;
pub mod skeleton;
pub mod soldier;
pub mod supply;
pub mod vehicle;

use std::path::Path;

use bytefield_con::session;
use bytefield_con::templates::TemplateDb;
use bytefield_formats::image::Orientation;
use bytefield_vfs::Vfs;
use bytefield_world::Level;
use serde::Serialize;

/// Gravity, m/s² downwards: the engine's default (`BasicPhysicsSystem`, read from the 1.61
/// Linux server). Not 9.81: BF1942 falls faster than the real world.
pub const GRAVITY: f64 = 14.73;

/// Acceleration magnitude a node may accumulate in one step.
pub const MAX_ACCELERATION: f64 = 1000.0;

/// Drag multiplier when fully under water.
pub const UNDERWATER_DRAG: f64 = 25.0;

/// The obstacle template's damage by default (the constructor's; barbed wire sets none).
pub const OBSTACLE_DAMAGE: f32 = 5.0;

/// Seconds a removed object's place in `Sim.vehicles` waits before [`Sim::spawn_vehicle`] may take it for another (ours,
/// long past the clients' snapshots kept, `HISTORY`, and a bot's second look; a level's tests last less)
pub const REUSE_AFTER: f64 = 60.0;

/// The meshes [`World::obstacles`] has loaded, by geometry.
type ObstacleCache = std::collections::HashMap<String, Option<(Vec<std::sync::Arc<obstacle::FaceMesh>>, [[f32; 3]; 2], [[f32; 3]; 2])>>;

/// `GeometryTemplate.scale`: one number or `x/y/z`; what cannot be read or is not positive counts as 1.
pub fn parse_scale(v: &str) -> [f32; 3] {
    let p: Vec<f32> = v.split('/').filter_map(|x| x.trim().parse().ok()).collect();
    match p[..] {
        [k] if k > 0.0 => [k; 3],
        [x, y, z] if x > 0.0 && y > 0.0 && z > 0.0 => [x, y, z],
        _ => [1.0; 3],
    }
}

/// The obstacles of `name`'s tree placed by `m` (`Object.geometry.scale` `scale`): every part whose
/// template has collision physics and whose geometry has collision meshes.
#[allow(clippy::too_many_arguments)]
fn obstacle_walk(
    w: &World,
    name: &str,
    m: bytefield_world::scene::Mat4,
    scale: [f32; 3],
    depth: usize,
    cache: &mut ObstacleCache,
    out: &mut Vec<obstacle::Obstacle>,
) {
    use bytefield_world::scene::children;
    use std::sync::Arc;
    if depth > 16 {
        return;
    }
    let Some(t) = w.db.object_template(name) else { return };
    let flag = |n: &str| t.get(n).or_else(|| t.get(&format!("set{n}"))).and_then(|a| a.first()).is_some_and(|v| v.trim() == "1");
    // without a geometry of its own, the one `findLodGeometry` finds below it (a Bundle
    // over a LodObject: `landrep1_supply`; checked against the server on El Alamein)
    if flag("hascollisionphysics")
        && let Some(g) = w.physics_geometry(name).as_ref()
    {
        let entry = cache.entry(g.to_lowercase()).or_insert_with(|| {
            let mesh = w.mesh(g)?;
            let lods: Vec<_> = mesh.collision.iter().filter(|c| !c.bsp_faces.is_empty()).map(|c| Arc::new(obstacle::FaceMesh::from_collision(c))).collect();
            // (the collision may reach past what is drawn: Anzio's merged rails, 39 m drawn and 200 m of collision)
            let mut reach = mesh.bounds;
            for l in &lods {
                for i in 0..3 {
                    reach[0][i] = reach[0][i].min(l.bounds[0][i]);
                    reach[1][i] = reach[1][i].max(l.bounds[1][i]);
                }
            }
            (!lods.is_empty()).then_some((lods, mesh.bounds, reach))
        });
        if let Some((lods, bounds, reach)) = entry.clone() {
            let damage = t.get("damage").and_then(|a| a.first()).and_then(|v| v.trim().parse().ok()).unwrap_or(OBSTACLE_DAMAGE);
            let projectiles_pass = t.all("addtocollisiongroup").iter().any(|a| a.first().is_some_and(|g| g.trim().eq_ignore_ascii_case("c_CGProjectiles")));
            let c = m.0;
            let rows = [0, 1, 2].map(|r| [c[r][0] as f32, c[r][1] as f32, c[r][2] as f32]);
            let pos = [c[3][0] as f32, c[3][1] as f32, c[3][2] as f32];
            let mid = [0, 1, 2].map(|i| (reach[0][i] + reach[1][i]) * 0.5 * scale[i]);
            let half = [0, 1, 2].map(|i| (reach[1][i] - reach[0][i]) * 0.5 * scale[i].abs());
            let center = m.transform_point(mid.map(|x| x as f64)).map(|x| x as f32);
            let radius = (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt();
            out.push(obstacle::Obstacle { name: t.name.clone(), rows, pos, scale, lods, center, radius, obstacle_class: t.kind.eq_ignore_ascii_case("obstacle"), damage, projectiles_pass, bounds, carrier: None });
        }
    }
    let lod = t.kind.eq_ignore_ascii_case("lodobject");
    for (child, local) in children(t).iter().take(if lod { 1 } else { usize::MAX }) {
        obstacle_walk(w, child, m.mul(local), scale, depth + 1, cache, out);
    }
}

pub struct World {
    pub vfs: Vfs,
    pub db: TemplateDb,
    pub level: Level,
    /// built on first use
    obstacles: std::sync::OnceLock<Vec<obstacle::Obstacle>>,
    obstacle_grid: std::sync::OnceLock<obstacle::ObstacleGrid>,
    /// [`World::physics_box`] by template, read once: the AI asks every tick, and each read decompresses the mesh
    physics_boxes: std::sync::Mutex<std::collections::HashMap<String, Option<[[f32; 3]; 2]>>>,
    /// [`World::bounding_radius`] by template: the AI's kit asks for its projectiles' every tick
    bounding_radii: std::sync::Mutex<std::collections::HashMap<String, f32>>,
    /// [`Sim::round_share`] by weapon and vehicle template
    round_shares: std::sync::Mutex<std::collections::HashMap<(String, String), [f32; 3]>>,
    /// the obstacles in each projectile template's tree ([`World::carried_obstacles`])
    carried: std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<Vec<obstacle::Obstacle>>>>,
    materials: std::sync::OnceLock<contact::Materials>,
    animations: std::sync::OnceLock<std::sync::Arc<anim::Machine>>,
    sectors: std::sync::OnceLock<Vec<portal::Sector>>,
    /// the bots' maps, the level's or made here ([`World::nav_map`])
    nav_maps: [std::sync::OnceLock<Option<std::sync::Arc<bytefield_formats::pathmap::PathMap>>>; 2],
    /// the game play mode (`GPM_CQ`, `GPM_CTF`…: `session::game_play_mode`)
    pub mode: String,
    /// the game's tree it was loaded from (the navigation mesh's cache lives next to it)
    pub game: std::path::PathBuf,
    /// the walkable-surface navigation mesh for a soldier, built or read from the cache on first use ([`World::navmesh`])
    navmesh: std::sync::OnceLock<std::sync::Arc<navmesh::NavMesh>>,
    /// the cover points over the mesh, on first use ([`World::cover_points`])
    cover: std::sync::OnceLock<std::sync::Arc<cover::CoverPoints>>,
}

impl World {
    pub fn load(game: &Path, mod_name: &str, level: &str, mode: &str) -> Result<World, String> {
        let (_, mut vfs) = session::mount(game, mod_name)?;
        let it = session::load(&vfs, Some(level), mode);
        let db = it.host.db;
        let level = Level::from_db(&vfs, &db, level)?;
        let dir = level.terrain.heightmap.rsplit_once('/').map_or("", |d| d.0).to_string();
        // the level's own meshes and textures, over the mod's (GC_Alaris_Prime's speederbike:
        // a reference run of the original)
        vfs.mount_level_assets(&dir);
        Ok(World {
            vfs, db, level, obstacles: Default::default(), obstacle_grid: Default::default(), physics_boxes: Default::default(), bounding_radii: Default::default(), round_shares: Default::default(), carried: Default::default(), materials: Default::default(), animations: Default::default(), sectors: Default::default(), nav_maps: Default::default(), mode: session::game_play_mode(mode).to_string(), game: game.to_path_buf(), navmesh: Default::default(), cover: Default::default(),
        })
    }

    pub fn height_at(&self, x: f64, z: f64) -> Option<f64> {
        self.level.height_at(x, z, Orientation::IDENTITY)
    }

    /// The walkable-surface navigation mesh for a soldier ([`navmesh::Params::infantry`]), read from the cache next to the
    /// game's tree or built (and written there) the first time it is asked.
    pub fn navmesh(&self) -> std::sync::Arc<navmesh::NavMesh> {
        self.navmesh
            .get_or_init(|| {
                let p = navmesh::Params::infantry(self);
                std::sync::Arc::new(navmesh::NavMesh::load_or_build(self, &p, 4, &navmesh::NavMesh::default_cache_dir(&self.game)).0)
            })
            .clone()
    }

    /// The level's cover points ([`cover::CoverPoints`]) over [`World::navmesh`], from the cache next to the game's tree (built and
    /// written there the first time: a few seconds; `examples/cover_points` makes them beforehand).
    pub fn cover_points(&self) -> std::sync::Arc<cover::CoverPoints> {
        self.cover
            .get_or_init(|| {
                let mesh = self.navmesh();
                std::sync::Arc::new(cover::CoverPoints::load_or_build(self, &mesh, 4, &cover::CoverPoints::default_cache_dir(&self.game)).0)
            })
            .clone()
    }

    /// Gives the level a mesh built elsewhere (a test's, over part of the map) in place of the cache's; only before the first
    /// [`World::navmesh`]. Whether it was taken.
    pub fn set_navmesh(&self, mesh: navmesh::NavMesh) -> bool {
        self.navmesh.set(std::sync::Arc::new(mesh)).is_ok()
    }

    /// Whether the terrain (samples every 2 m, a little over the ground) or one of the level's objects that stop bullets hides `b`
    /// from `a` ([`Sim::hidden`]: the AI's line of sight; static, so the cover points are made with it).
    pub fn hidden(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        let d: f32 = (0..3).map(|k| (b[k] - a[k]).powi(2)).sum::<f32>().sqrt();
        let n = (d / 2.0).ceil().max(1.0) as usize;
        let terrain = (1..n).any(|i| {
            let t = i as f32 / n as f32;
            let p: [f32; 3] = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
            self.height_at(p[0] as f64, p[2] as f64).is_some_and(|h| h as f32 > p[1] - 0.2)
        });
        if terrain {
            return true;
        }
        let dir = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        // (only the obstacles along the line: the same ones in the same order)
        thread_local!(static ALONG: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) });
        let obstacles = self.obstacles();
        ALONG.with_borrow_mut(|along| {
            self.obstacle_grid().along(a, dir, along);
            along.iter().map(|&i| &obstacles[i as usize]).filter(|o| !o.projectiles_pass && crate::projectile::segment_near_sphere(a, dir, o.center, o.radius + 1.0)).any(|o| {
                crate::obstacle::pick_lod(&o.lods, 1).is_some_and(|mesh| crate::obstacle::segment_blocked_skipping(mesh, &o.rows, o.pos, o.scale, a, dir, false, Some(crate::projectile::NON_SOLID_MATERIAL)))
            })
        })
    }

    /// Whether a soldier walking straight from `a` to `b` meets a wall: the terrain, or the faces a soldier collides with (LOD
    /// 1) seen from either side ([`bot_api::BotView::walk_blocked`]).
    pub fn walk_blocked(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        let d: f32 = (0..3).map(|k| (b[k] - a[k]).powi(2)).sum::<f32>().sqrt();
        let n = (d / 2.0).ceil().max(1.0) as usize;
        if (1..=n).any(|i| {
            let t = i as f32 / n as f32;
            let p: [f32; 3] = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
            self.height_at(p[0] as f64, p[2] as f64).is_some_and(|h| h as f32 > p[1])
        }) {
            return true;
        }
        let dir = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let obstacles = self.obstacles();
        let mut along = Vec::new();
        self.obstacle_grid().along(a, dir, &mut along);
        along.iter().map(|&i| &obstacles[i as usize]).filter(|o| crate::projectile::segment_near_sphere(a, dir, o.center, o.radius + 1.0)).any(|o| {
            crate::obstacle::pick_lod(&o.lods, crate::contact::faces_lod(0.5)).is_some_and(|mesh| crate::obstacle::segment_blocked_skipping(mesh, &o.rows, o.pos, o.scale, a, dir, true, None))
        })
    }

    /// The geometry the engine's physics uses for an object template (`findLodGeometry`):
    /// the template's own geometry; else, if its first child is a `LodObject`,
    /// the geometry of that one's most detailed child; else the most detailed child's geometry of
    /// the first `LodObject` below it (depth first) whose selector is a `DistCompareSelector`
    /// (`internalFindChildOfLodSelectorCID`: a `PlayerControlObject` ends the search among its
    /// siblings). DC's MH-6 hangs its fuselage off `lodH6Cockpit`, not off `lodMH6`
    /// (`DistCompareSelector2`, a bundle without geometry).
    pub fn physics_geometry(&self, template: &str) -> Option<String> {
        use bytefield_con::templates::Template;
        let own = |t: &Template| -> Option<String> {
            let g = t.get("geometry")?.first()?;
            self.db.get("geometrytemplate", g).map(|_| g.clone())
        };
        let is = |t: &Template, kind: &str| t.kind.eq_ignore_ascii_case(kind);
        let highest = |lod: &Template| -> Option<String> {
            let (first, _) = bytefield_world::scene::children(lod).into_iter().next()?;
            own(self.db.object_template(&first)?)
        };
        fn search<'a>(w: &'a World, t: &Template, depth: usize) -> Option<&'a Template> {
            if depth > 16 {
                return None;
            }
            for (child, _) in bytefield_world::scene::children(t) {
                let Some(c) = w.db.object_template(&child) else { continue };
                if c.kind.eq_ignore_ascii_case("playercontrolobject") {
                    return None;
                }
                if c.kind.eq_ignore_ascii_case("lodobject")
                    && let Some(sel) = c.get("lodselector").and_then(|a| a.first())
                    && w.db.get("lodselectortemplate", sel).is_some_and(|s| s.kind.eq_ignore_ascii_case("distcompareselector"))
                {
                    return Some(c);
                }
                if let Some(found) = search(w, c, depth + 1) {
                    return Some(found);
                }
            }
            None
        }
        let t = self.db.object_template(template)?;
        if let Some(g) = own(t) {
            return Some(g);
        }
        let first = bytefield_world::scene::children(t).into_iter().next().and_then(|(f, _)| self.db.object_template(&f));
        if let Some(g) = first.filter(|f| is(f, "lodobject")).and_then(|f| highest(f)) {
            return Some(g);
        }
        search(self, t, 0).and_then(|lod| highest(lod))
    }

    /// A geometry template's mesh (`standardmesh/<file>.sm`), read, decompressed and parsed each time: not for every tick
    /// (the AI asking through `physics_box` and `bounding_radius` took 85 % of a step).
    pub fn mesh(&self, geometry: &str) -> Option<bytefield_formats::sm::StandardMesh> {
        let gt = self.db.get("geometrytemplate", geometry)?;
        let file = gt.get("file")?.first()?.replace('\\', "/");
        let bytes = self.vfs.read(&format!("standardmesh/{file}.sm")).ok()?;
        bytefield_formats::sm::parse(&bytes).ok()
    }

    /// The geometry box (min, max) the engine's physics uses for an object template: the `.sm`
    /// header's bounds of `physics_geometry`.
    pub fn physics_box(&self, template: &str) -> Option<[[f32; 3]; 2]> {
        let mut boxes = self.physics_boxes.lock().unwrap();
        if let Some(b) = boxes.get(template) {
            return *b;
        }
        let b = self.physics_geometry(template).and_then(|g| self.mesh(&g)).map(|m| m.bounds);
        boxes.insert(template.to_string(), b);
        b
    }

    /// The parts of an object that touch the terrain, in the engine's order: the root and every
    /// part below it (the first child of a LodObject) whose template has response and mobile
    /// physics and a collision mesh. The root uses its LOD geometry (`physics_geometry`).
    pub fn contactors(&self, template: &str) -> Vec<contact::Contactor> {
        let mut out = Vec::new();
        if let Some(model) = vehicle::model(&self.db, template) {
            let _ = self.collect_contactors(&model, [0.0; 3], craft::IDENTITY, craft::IDENTITY, true, false, &mut None, &mut 0, &mut 0, &mut Vec::new(), &mut out);
        }
        out
    }

    /// `anchor`: the nearest moving ancestor (its index among the craft's parts, `parts` counts
    /// them as `craft::collect` does) and the placement in it, beside the one in the root.
    #[allow(clippy::too_many_arguments)]
    fn collect_contactors(&self, c: &vehicle::Component, pos: [f32; 3], rows: [[f32; 3]; 3], local: [[f32; 3]; 3], root: bool, engine_above: bool, anchor: &mut Option<(usize, [f32; 3], [[f32; 3]; 3])>, parts: &mut usize, walked: &mut usize, links: &mut Vec<([f32; 3], [[f32; 3]; 3], Option<(usize, [[f32; 3]; 3])>)>, out: &mut Vec<contact::Contactor>) -> bool {
        let mut own = false;
        if !root && craft::is_part(c) {
            *anchor = Some((*parts, [0.0; 3], craft::IDENTITY));
            *parts += 1;
        }
        if let Some(t) = self.db.object_template(&c.name) {
            let flag = |n: &str| t.get(n).and_then(|a| a.first()).is_some_and(|v| v.trim() == "1");
            let geometry = if root { self.physics_geometry(&c.name) } else { t.get("geometry").and_then(|a| a.first()).cloned() };
            let mesh = geometry.as_deref().and_then(|g| self.mesh(g));
            // a part with only `hasCollisionPhysics` (the T34-85's tower) is there for the
            // others to hit: its faces, no vertices of its own
            // the response is made whatever `hasResponsePhysics` says (`SimpleObject`'s constructor always calls
            // `setResponsePhysicsComponent`: rigid when mobile), so GC's `DroidCapsule` (`setHasResponsePhysics 0`) rests on
            // the ground; a part touches when it is mobile and has `hasCollisionPhysics` (the original checks GC's AT-AT feet,
            // `hasResponsePhysics 0`, and DC's `MortarSpring`, with no line, every frame; not the Lambda's front and back gears,
            // mobile with no collision line)
            // a rigid projectile's root is mobile whatever its template says,
            // so it also touches what it runs into (DC's Hydra, AT2 and Hellfire went through the hills)
            // TODO: where a Hydra goes off against the original (never compared). How: reference run a DC Hydra on the original server and compare with `examples/phys_diff`.
            let projectile_root = root && c.kind.eq_ignore_ascii_case("projectile");
            let active = (flag("hasmobilephysics") || projectile_root) && (root || flag("hascollisionphysics"));
            // a root with `hasCollisionPhysics` and no mobile physics (DC's Patriot and SA-3 PCOs, the fixed guns, the
            // static Nimitz): faces for the others to hit too. The original's PatriotGuidedRocket hits the object
            // named `Patriot` (the root, with `Patriot_Base`'s collision mesh) the frame after its birth (record 1536 of the original's reference run)
            let passive = flag("hascollisionphysics") && !active;
            if (passive || active)
                && let Some(m) = mesh
                && let Some(col) = m.collision.first()
            {
                let grip = t.get("grip").and_then(|a| a.first()).and_then(|g| contact::grip::parse(g)).unwrap_or(contact::grip::CONTACT);
                let scale_of = geometry.as_deref().and_then(|g| self.db.get("geometrytemplate", g)).and_then(|g| g.get("scale")?.first().map(|v| parse_scale(v))).unwrap_or([1.0; 3]);
                own = true;
                *walked += 1;
                // a spring's node keeps its parent's axes, not its own turn (`setRotation` under `addTemplate`): the original
                // checks the left front wheel that DC's Urals and BM21 hang at -179.999/0/0 with its mesh unmirrored (the
                // contact point of the original's first record, to the micrometre); mirrored, it touched 1 mm off and every
                // run of those trucks drifted apart from the first step
                let crows = if c.kind.eq_ignore_ascii_case("spring") { craft::mul(&craft::transpose(&local), &rows) } else { rows };
                out.push(contact::Contactor {
                    name: c.name.clone(),
                    walked: *walked,
                    // (not scaled: `GeometryTemplate.scale` only reaches the faces; scaling these took Technical/go from 0,49 to 17,5 m and BRDM2/go from 0,02 to 0,83 against the original)
                    vertices: if passive { Vec::new() } else { col.vertices.clone() },
                    materials: if passive { Vec::new() } else { col.vertex_materials.iter().map(|&m| m as u32).collect() },
                    bbox: Some(m.bounds),
                    pos,
                    // EXPERIMENT: a spring's vertices in its parent's axes, without its own turn
                    rows: crows,
                    grip,
                    is_root: root,
                    soldier: c.kind.eq_ignore_ascii_case("soldier"),
                    projectile: c.kind.eq_ignore_ascii_case("projectile"),
                    spring: c.kind.eq_ignore_ascii_case("spring").then(|| {
                        let num = |n: &str| t.get(n).and_then(|a| a.first()).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                        contact::Spring { strength: num("setstrength"), damping: num("setdamping"), local: [c.position[0] as f32, c.position[1] as f32, c.position[2] as f32], chain: links[..links.len().saturating_sub(1)].iter().map(|&(l, axes, _)| (l, axes)).collect(), chain_anchor: links[..links.len().saturating_sub(1)].iter().map(|&(_, axes, a)| (a, axes)).collect(), turn: (local != craft::IDENTITY).then_some(local), ..Default::default() }
                    }),
                    response: contact::Response::default(),
                    last_impulses: Vec::new(),
                    hits: Vec::new(),
                    faces: m.collision.iter().filter(|c| !c.bsp_faces.is_empty()).map(|c| std::sync::Arc::new(obstacle::FaceMesh::from_collision(c))).collect(),
                    scale: scale_of,
                    radius: self.bounding_radius(&c.name),
                    anchor: anchor.map(|(part, p, r)| contact::Anchor { part, pos: p, rows: r, rest: (pos, crows) }),
                    drive: None,
                    forward: if crows == rows { [local[0][2], local[1][2], local[2][2]] } else { [0.0, 0.0, 1.0] }, // (a spring is in its parent's axes already)
                    // the parent's rows in the root: rows = local · parent
                    side: (c.position[0] as f32, [0, 1, 2].map(|j| local[0][0] * rows[0][j] + local[1][0] * rows[1][j] + local[2][0] * rows[2][j])),
                    engine_above,
                    // every part, springs too, is placed in the world link by link down its parents, rounded to f32 at each, as the
                    // original does: from the root in one go GC's AT-ST on GC_Endor falls 1-2 ulp off at its right foot and turns away
                    // (3.84 m off at 8 s, 0.03 m placed down the chain)
                    chain: links.iter().map(|&(l, axes, _)| (l, axes)).collect(),
                    chain_anchor: links.iter().map(|&(_, axes, a)| (a, axes)).collect(),
                });
            }
        }
        // the original's ResponsePhysicsManager goes over a node's children that touch themselves first, in the
        // template's order, and then over the rest of each child's subtree, from the last child to the first (
        // over the `check` records of 137 DC_Final reference runs of the original, all alike: the M-109's right track before the
        // left, the Technical's back springs before the front wheels' and those right after left, the F-117A's three gears
        // and then their wheels backwards). The order of the impulses moves the
        // wheels' load. The walk itself stays in the template's order (`parts` counts as `craft::collect` does).
        // TODO: the mechanism behind it (how the children reach the manager's list) is not known
        // (this is the manager's list, what a vehicle without a driver goes through: with a driver it walks its own tree, in the
        // template's order, [`Contactor::walked`], mover::check_all)
        let mut later: Vec<Vec<contact::Contactor>> = Vec::new();
        // (a RotationalBundle right under a LodObject sits at the LodObject's place for the physics: craft::collect)
        let lod = c.kind.eq_ignore_ascii_case("lodobject");
        for ch in &c.children {
            let chp = if lod && ch.kind.eq_ignore_ascii_case("rotationalbundle") { [0.0f32; 3] } else { ch.position.map(|x| x as f32) };
            let p = craft::to_parent(chp, &rows);
            let local = craft::ypr(ch.rotation.map(|x| x as f32));
            let mut a = anchor.map(|(k, ap, ar)| {
                let q = craft::to_parent(chp, &ar);
                (k, [ap[0] + q[0], ap[1] + q[1], ap[2] + q[2]], craft::mul(&local, &ar))
            });
            let mut sub = Vec::new();
            // the links from the root down, each its place in its parent and the parent's axes in the root: a spring's parent placed as the original does (`contact::Spring::chain`)
            links.push((chp, rows, anchor.map(|(k, _, ar)| (k, ar))));
            let own = self.collect_contactors(ch, [pos[0] + p[0], pos[1] + p[1], pos[2] + p[2]], craft::mul(&local, &rows), local, false, engine_above || c.kind.eq_ignore_ascii_case("engine"), &mut a, parts, walked, links, &mut sub);
            links.pop();
            if !contact_order_v2() {
                out.append(&mut sub);
                continue;
            }
            if own {
                out.push(sub.remove(0));
            }
            later.push(sub);
        }
        for mut sub in later.into_iter().rev() {
            out.append(&mut sub);
        }
        own
    }

    /// The level's objects a mover can run into: every placed part whose template has
    /// collision physics and whose geometry (`physics_geometry`) has collision meshes, with its world placement and
    /// `Object.geometry.scale`. Static objects only: vehicles are not in it. Built once.
    pub fn obstacles(&self) -> &[obstacle::Obstacle] {
        self.obstacles.get_or_init(|| self.build_obstacles())
    }

    /// Adds obstacles to the level's (synthetic ones: test steps, ramps, bridges); before a [`Sim`] is made on the world.
    pub fn push_obstacles(&mut self, extra: Vec<obstacle::Obstacle>) {
        let mut all = self.obstacles().to_vec();
        all.extend(extra);
        self.obstacles = std::sync::OnceLock::from(all);
        self.obstacle_grid = Default::default();
        self.navmesh = Default::default();
    }

    /// [`World::obstacles`] by the ground they stand on, with the margin of the AI's lines ([`Sim::hidden`]).
    pub(crate) fn obstacle_grid(&self) -> &obstacle::ObstacleGrid {
        self.obstacle_grid.get_or_init(|| obstacle::ObstacleGrid::new(self.obstacles(), 1.0))
    }

    fn build_obstacles(&self) -> Vec<obstacle::Obstacle> {
        use bytefield_world::scene::Mat4;
        let mut cache = ObstacleCache::new();
        let mut out = Vec::new();
        let vec3 = |args: &[String]| args.first().and_then(|a| bytefield_world::parse_vec(a)).filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]);
        // a placed object's matrix as the engine builds it (`ref2::setRotation`); its children's
        // composed on it
        let placed = |pos: [f64; 3], rot: [f64; 3]| {
            let m = ref2::placed(pos, rot);
            Mat4(std::array::from_fn(|r| std::array::from_fn(|c| m[4 * r + c] as f64)))
        };
        for inst in &self.db.instances {
            let get = |n: &str| inst.props.iter().rev().find(|p| p.name == n).and_then(|p| vec3(&p.args));
            let Some(pos) = get("absoluteposition") else { continue };
            // a placed vehicle is one of the sim's ([`World::placed_vehicles`])
            if self.is_vehicle(&inst.template) {
                continue;
            }
            let rot = get("rotation").or_else(|| get("setrotation")).unwrap_or([0.0; 3]);
            let scale = get("geometry.scale").map_or([1.0; 3], |s| s.map(|x| x as f32));
            obstacle_walk(self, &inst.template, placed(pos, rot), scale, 0, &mut cache, &mut out);
        }
        // what the spawners make and does not move (fixed guns)
        for (t, pos, rot) in self.spawned_at_start() {
            if !self.is_mobile(&t) {
                obstacle_walk(self, &t, placed(pos, rot), [1.0; 3], 0, &mut cache, &mut out);
            }
        }
        out
    }

    /// The `Obstacle`s in a projectile template's tree, in the projectile's frame (GC's shield:
    /// `ShieldProjectile` ∋ `ShieldBlock`, which stops projectiles and slows soldiers down while the
    /// projectile lasts: a reference run of the original). Empty for most.
    pub fn carried_obstacles(&self, template: &str) -> std::sync::Arc<Vec<obstacle::Obstacle>> {
        let key = template.to_ascii_lowercase();
        if let Some(c) = self.carried.lock().unwrap().get(&key) {
            return c.clone();
        }
        let mut cache = ObstacleCache::new();
        let mut out = Vec::new();
        for (name, m) in bytefield_world::child_templates_of_kind(&self.db, template, "obstacle") {
            obstacle_walk(self, &name, m, [1.0; 3], 0, &mut cache, &mut out);
        }
        let c = std::sync::Arc::new(out);
        self.carried.lock().unwrap().insert(key, c.clone());
        c
    }

    /// Whether [`Sim::instantiate`] makes a template a vehicle: a `PlayerControlObject` that is
    /// mobile, a fixed gun or a target.
    pub fn is_vehicle(&self, template: &str) -> bool {
        self.is_player_control_object(template) && (self.is_mobile(template) || self.is_fixed_gun(template) || self.is_target(template))
    }

    /// The vehicles placed in the level's scripts (`Object.create` in `StaticObjects.con` or
    /// conquest's `ObjectSpawns.con`, not a spawner's: DC_First_Light's mortar, GC_BCanyon's medical
    /// bed, GC's E-Webs): the server makes them as any other `PlayerControlObject` (its `Pco` list;
    /// a reference run of the original).
    /// (template, position, rotation), in the scripts' order.
    pub fn placed_vehicles(&self) -> Vec<(String, [f64; 3], [f64; 3])> {
        let vec3 = |args: &[String]| args.first().and_then(|a| bytefield_world::parse_vec(a)).filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]);
        self.db
            .instances
            .iter()
            .filter(|inst| self.is_vehicle(&inst.template))
            .filter_map(|inst| {
                let get = |n: &str| inst.props.iter().rev().find(|p| p.name == n).and_then(|p| vec3(&p.args));
                let pos = get("absoluteposition")?;
                Some((inst.template.clone(), pos, get("rotation").or_else(|| get("setrotation")).unwrap_or([0.0; 3])))
            })
            .collect()
    }

    /// Whether an object template gets a mobile physics node (`hasMobilePhysics`): a vehicle for
    /// the [`Sim`]; without it a spawned object stays where it is made (a fixed gun: an obstacle).
    pub fn is_mobile(&self, template: &str) -> bool {
        self.db.object_template(template).and_then(|t| t.get("hasmobilephysics")).and_then(|a| a.first()).is_some_and(|v| v.trim() == "1")
    }

    /// Whether an object template is a `PlayerControlObject` (as a weapon's `projectileTemplate`:
    /// a guided missile, [`Sim::launch`]).
    pub fn is_player_control_object(&self, template: &str) -> bool {
        self.db.object_template(template).is_some_and(|t| t.kind.eq_ignore_ascii_case("PlayerControlObject"))
    }

    /// Whether a template without mobile physics can be manned (`PlayerControlObject` with a
    /// seat: a fixed gun): the [`Sim`] makes it a [`mover::Vehicle`] that does not move.
    pub fn is_fixed_gun(&self, template: &str) -> bool {
        !self.is_mobile(template)
            && self.db.object_template(template).is_some_and(|t| t.kind.eq_ignore_ascii_case("PlayerControlObject"))
            && vehicle::model(&self.db, template).is_some_and(|m| !entry::Seat::all_from_model(&m).is_empty())
    }

    /// Whether a template without seats can still be destroyed (`PlayerControlObject` with
    /// `hitpoints`): an objective's target, made as a [`mover::Vehicle`] that does not move.
    pub fn is_target(&self, template: &str) -> bool {
        self.db.object_template(template).is_some_and(|t| t.kind.eq_ignore_ascii_case("PlayerControlObject") && t.get("hitpoints").is_some())
    }

    /// What the level's spawners make when the round starts, in the order the server's
    /// physics lists hold them (the reverse of the scripts': checked on Berlin): (template,
    /// position, rotation) of each spawner with a template for its team and no `spawnDelayAtStart`.
    /// The spawner's spawn object: the spawner's position plus `spawnOffset` (not turned), and
    /// its rotation.
    /// Only the level's spawners, in reverse (not the ones on ships): the server's order, with those, is
    /// [`Sim::first_spawns_in_script_order`].
    pub fn spawned_at_start(&self) -> Vec<(String, [f64; 3], [f64; 3])> {
        self.level
            .spawners
            .iter()
            .rev()
            .filter(|s| !s.delay_at_start)
            .filter_map(|s| {
                let t = s.template()?;
                self.db.object_template(t)?;
                Some((t.to_string(), [0, 1, 2].map(|i| s.pos[i] + s.offset[i]), s.rotation))
            })
            .collect()
    }

    /// `getBoundingRadius` of an object template: the larger of its geometry's radius
    /// (the farthest corner of the `.sm` box; not an `AnimatedBundle`'s own) and, for every child, its distance from the
    /// parent plus its own radius; of a `LodObject`'s children, only the one its selector shows in the server (
    /// 115 of the server's 118, by `radius_tree --rule`).
    pub fn bounding_radius(&self, template: &str) -> f32 {
        const NO_BOUNDS: &[&str] = &["effectbundle", "emitter"];
        fn walk(w: &World, name: &str, depth: usize, firearms: bool) -> f32 {
            if depth > 16 {
                return 0.0;
            }
            let Some(t) = w.db.object_template(name) else { return 0.0 };
            // a weapon's effects (the muzzle flash) do not count, the rest do: the server's getBoundingRadius of
            // Berlin's four tanks and, with the water foam 9 m behind them, of the boats and DC's amphibians
            if firearms && NO_BOUNDS.iter().any(|k| t.kind.eq_ignore_ascii_case(k)) {
                return 0.0;
            }
            let firearms = firearms || t.kind.eq_ignore_ascii_case("firearms");
            // (an AnimatedBundle's own geometry does not count, its children do: the Shilka's tracks, 4.174 against the server's 4.072)
            let own = !t.kind.eq_ignore_ascii_case("animatedbundle");
            let mut r = t
                .get("geometry")
                .filter(|_| own)
                .and_then(|a| a.first())
                .and_then(|g| w.mesh(g))
                .map_or(0.0, |m| {
                    let c = |i: usize| m.bounds[0][i].abs().max(m.bounds[1][i].abs());
                    (c(0) * c(0) + c(1) * c(1) + c(2) * c(2)).sqrt()
                });
            // the missile drawn on a launcher (`visibleDummyProjectileTemplate`) counts, one at each `addFireArmsPosition`: the
            // SA-3's sub rocket at 3.29 m gives the server's 14.480;
            // and so does the muzzle effect (`visibleBarrelTemplate`, radius 0 under a FireArms), but only at the first position:
            // ZPU-4 1.2 + |0.3/0.3/2| = 3.245, Patriot 4.133 + |-0.55/1.45/6| = 10.330, and the 155 battery stays at 11.865
            // (its farthest position, 10/0/4.5, would give 14.722, census 118 of 118)
            let at: Vec<f32> = t
                .all("addfirearmsposition")
                .iter()
                .filter_map(|a| bytefield_world::parse_vec(a.first()?).filter(|v| v.len() == 3))
                .map(|v| ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as f32).sqrt())
                .collect();
            for (key, first) in [("visibledummyprojectiletemplate", false), ("visiblebarreltemplate", true)] {
                if let Some(d) = t.get(key).and_then(|a| a.first()) {
                    let at = if first { at.first().copied().unwrap_or(0.0) } else { at.iter().copied().fold(0.0, f32::max) };
                    r = r.max(at + walk(w, d.trim(), depth + 1, firearms));
                }
            }
            // a LodObject: only the child its selector picks with no camera near: a `DistCompareSelector`(2) or
            // `CompareSelector` the first (not the AH64's blurred propeller nor any wreck), a `DistanceSelector` the last (not the
            // AC-130's internal sight); with no selector, all
            // TODO: B17_PCO2 (XPack2) wants the first child of its `DistanceSelector` (the server's 1.138, the internal gun; ours
            // 0.996), the AH64 the last; what tells them apart is open.
            let mut children = bytefield_world::scene::children(t);
            if t.kind.eq_ignore_ascii_case("lodobject") && !children.is_empty() {
                let sel = t.get("lodselector").and_then(|a| a.first()).and_then(|s| w.db.get("lodselectortemplate", s)).map(|s| s.kind.to_ascii_lowercase());
                match sel.as_deref() {
                    Some("distcompareselector" | "distcompareselector2" | "compareselector") => children.truncate(1),
                    Some("distanceselector") => children = children.split_off(children.len() - 1),
                    _ => {}
                }
            }
            for (child, local) in children {
                let p = local.0[3];
                let d = ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]) as f32).sqrt();
                r = r.max(d + walk(w, &child, depth + 1, firearms));
            }
            r
        }
        if let Some(r) = self.bounding_radii.lock().unwrap().get(template) {
            return *r;
        }
        let r = walk(self, template, 0, false);
        self.bounding_radii.lock().unwrap().insert(template.to_string(), r);
        r
    }

    /// The material table (`MaterialManager.*` in the loaded scripts).
    pub fn materials(&self) -> &contact::Materials {
        self.materials.get_or_init(|| contact::Materials::from_commands(self.db.globals.iter().map(|c| (c.command.as_str(), c.args.as_slice()))))
    }

    /// The soldiers' animation state machine (`AnimationStateMachine.*` in the loaded scripts).
    pub fn animations(&self) -> std::sync::Arc<anim::Machine> {
        self.animations
            .get_or_init(|| {
                let m = anim::Machine::from_commands(self.db.globals.iter().map(|c| (c.command.as_str(), c.args.as_slice())), |f| self.vfs.read(f).ok());
                std::sync::Arc::new(m)
            })
            .clone()
    }

    /// `setVehicleCategory` of a vehicle (`operator>>`: `VCLand`/`Land` 0, the default; `VCSea`/`Sea` 1; `VCAir`/`Air` 2).
    pub fn vehicle_category(&self, template: &str) -> u32 {
        let c = self.db.object_template(template).and_then(|t| t.get("setvehiclecategory")).and_then(|a| a.first()).map(|s| s.trim().to_string());
        match c.as_deref() {
            Some("VCSea" | "Sea") => 1,
            Some("VCAir" | "Air") => 2,
            _ => 0,
        }
    }

    /// Whether `createObjectOnAllClients` holds a spawner's `template` back while a human's soldier
    /// is near (vehicle category 0, or 2 with a tool tip type not 0x24): a land
    /// `PlayerControlObject`, or an air one but the Wasserfall; never a sea one.
    pub fn held_near_humans(&self, template: &str) -> bool {
        let tip = self.db.object_template(template).and_then(|t| t.get("settooltiptype")).and_then(|a| a.first()).map(|s| s.trim().to_ascii_lowercase());
        self.is_player_control_object(template)
            && match self.vehicle_category(template) {
                0 => true,
                2 => !matches!(tip.as_deref(), Some("ttwasserfall" | "wasserfall")),
                _ => false,
            }
    }

    /// A vehicle ready to drop into a [`Sim`]: its craft, the parts that touch things and its
    /// bounding radius. At the origin, at rest, unrotated.
    pub fn vehicle(&self, template: &str) -> Option<mover::Vehicle> {
        if self.is_soldier(template) {
            return self.soldier(template);
        }
        let craft = self.craft(template)?;
        let weapons = craft
            .parts
            .iter()
            .enumerate()
            .filter_map(|(k, p)| match &p.kind {
                craft::Kind::Weapon(n) => self.weapon(n).map(|w| (k, firearms::FireArms::new(std::sync::Arc::new(w)))),
                _ => None,
            })
            .collect();
        let model = vehicle::model(&self.db, template);
        Some(mover::Vehicle {
            body: mover::Body {
                template: template.to_string(),
                craft,
                contacts: self.contactors(template),
                radius: self.bounding_radius(template),
                id: 0,
                solved: false,
                fixed: !self.is_mobile(template),
                physics_off: false,
            },
            movement: None,
            armor: self.db.object_template(template).and_then(damage::ArmorTemplate::from_template).map(damage::Armor::new),
            removed: false,
            gone_since: None,
            born: 0,
            place_gen: 0,
            team: 0,
            entry: crate::entry::Entry {
                submarine: self.db.object_template(template).and_then(|t| t.get("submarinedata")).and_then(entry::Submarine::parse),
                keep_team: self.db.object_template(template).and_then(|t| t.get("dontclearteamonexit")).and_then(|a| a.first()).is_some_and(|v| v.trim() == "1"),
                destroy_on_exit: self.db.object_template(template).and_then(|t| t.get("destroyonexit")).and_then(|a| a.first()).is_some_and(|v| v.trim() == "1"),
                sonar: model.as_ref().and_then(sonar::Sonar::from_model),
                ..crate::entry::Entry::with_seats(model.as_ref().map_or(Vec::new(), entry::Seat::all_from_model))
            },
            weapons,
            shots: Vec::new(),
        })
    }

    /// What a vehicle's collision pass runs against: the terrain, the level's obstacles and the
    /// material table.
    /// `carried`: those the projectiles carry ([`Sim::carried`]).
    pub fn surroundings<'a>(&'a self, carried: &'a [obstacle::Obstacle]) -> mover::Surroundings<'a> {
        mover::Surroundings { ground: &self.level, obstacles: obstacle::Obstacles { fixed: self.obstacles(), carried }, materials: self.materials() }
    }

    /// A rigid physics node for an object template: mass, drag and inertia modifier from its
    /// properties (the engine's defaults otherwise) and the geometry
    /// box. At the origin, at rest, unrotated. No `gravityModifier`: only particle and projectile
    /// templates have it, and the server ignores it elsewhere (GC's `GAIN-135` says 50; its node
    /// keeps 1).
    pub fn rigid_body(&self, template: &str) -> Option<rigid::RigidBody> {
        let t = self.db.object_template(template)?;
        let num = |n: &str, d: f32| t.get(n).and_then(|a| a.first()).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
        let mut b = rigid::RigidBody::new([0.0; 3], num("mass", 1.0), num("drag", 0.0), self.physics_box(template));
        if let Some(v) = t.get("inertiamodifier").and_then(|a| a.first()).and_then(|v| bytefield_world::parse_vec(v)).filter(|v| v.len() == 3) {
            b.inertia_modifier = [v[0] as f32, v[1] as f32, v[2] as f32];
        }
        Some(b)
    }

    /// A vehicle's flight physics (`craft`): its rigid body and its wings and engines.
    pub fn craft(&self, template: &str) -> Option<craft::Craft> {
        let model = vehicle::model(&self.db, template)?;
        let mut c = craft::Craft::new(&model, self.rigid_body(template)?);
        c.water_level = Some(self.level.terrain.water_level as f32);
        for p in c.parts.iter_mut() {
            if let craft::Kind::Floating(craft::Floating { node: Some(n), .. }) = &mut p.kind {
                n.sides = self.physics_box(&n.template).map(|[lo, hi]| [hi[0] - lo[0], hi[2] - lo[2]]);
            }
        }
        Some(c)
    }

    /// The terrain's height and normal at (x, z), as the engine computes them.
    pub fn terrain_at(&self, x: f64, z: f64) -> Option<(f64, [f64; 3])> {
        self.level.height_and_normal(x, z, Orientation::IDENTITY)
    }

    /// The highest of terrain and water at (x, z).
    pub fn surface_at(&self, x: f64, z: f64) -> Option<(f64, bool)> {
        let h = self.height_at(x, z)?;
        let w = self.level.terrain.water_level;
        Some(if h < w { (w, true) } else { (h, false) })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Contact {
    Ground,
    Water,
    OutOfWorld,
}

/// A point body, moved like the engine's `PointPhysicsNode` (projectiles):
/// - forces add to an acceleration accumulator; each step integrates it in 4 substeps of dt/4
///   (velocity first, then position), then empties it
/// - gravity (times `gravity_modifier`) is added to the accumulator *after* integrating, so it
///   acts from the next step on: a body spawned at rest does not move in its first step
/// - drag is linear in velocity: a -= pi r² drag / mass (v - wind) (x25 under water)
#[derive(Debug, Clone, Serialize)]
pub struct Body {
    pub pos: [f64; 3],
    pub vel: [f64; 3],
    pub acc: [f64; 3],
    /// multiplies gravity (`ObjectTemplate.gravityModifier`)
    pub gravity_modifier: f64,
    /// `ObjectTemplate.drag`
    pub drag: f64,
    /// `ObjectTemplate.mass`, kg
    pub mass: f64,
    /// bounding radius of the body's geometry, m (the drag's cross-section)
    pub radius: f64,
    pub contact: Option<(Contact, f64)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sample {
    pub t: f64,
    pub pos: [f64; 3],
    pub vel: [f64; 3],
}

/// The phases of a vehicle's frame, in the server's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Parts,
    Integrate,
    Collide,
}

/// Fixed-step world of bodies over a static level.
pub struct Sim<'w> {
    pub world: &'w World,
    /// the step's length, the world time and the steps taken ([`clock::Clock`])
    pub clock: clock::Clock,
    pub bodies: Vec<Body>,
    pub vehicles: Vec<mover::Vehicle>,
    /// shells, bullets, grenades ([`projectile`])
    pub projectiles: Vec<projectile::Projectile>,
    /// the spent projectiles' places, in the order they ended
    pub spent: std::collections::VecDeque<usize>,
    /// what the round counts and logs ([`journal::Journal`])
    pub journal: journal::Journal,
    /// Whose game this is: its own, or a copy of a server's ([`replica::Role`]).
    pub role: replica::Role,
    /// The spawners' frame updates to run at the end of the next step, when given (a
    /// reference run's: they follow the real clock); None: one of dt every step
    pub spawner_frames: Option<Vec<f32>>,
    /// the damage being given ([`damage::DamageState`])
    pub damage: damage::DamageState,
    /// the spawners (the level's, once `spawn_level_vehicles` made them; then those of the trees
    /// of what is made: [`create`])
    pub spawners: Vec<damage::SpawnerState>,
    /// the objects made that only lie there ([`create::Item`]: kits, bundles)
    pub items: Vec<create::Item>,
    /// the game mode's state ([`modes::Modes`])
    pub modes: modes::Modes,
    /// where the round is and its counters ([`round::RoundState`])
    pub round: round::RoundState,
    /// the players: teams, soldiers, deaths and spawns ([`players`]), and what happened to them
    pub players: Vec<players::Player>,
    pub rng: players::Rng,
    /// The server's settings the round plays by, fixed during it ([`round::RoundRules`])
    pub rules: round::RoundRules,
    /// who drives the bots ([`bot_api::Drivers`])
    pub drivers: bot_api::Drivers,
    /// the supply depots ([`supply`]): the level's (placed on the first step) and the vehicles'
    pub depots: Vec<supply::Depot>,
    pub depots_placed: bool,
    /// the pools of the soldiers' mines and charges ([`explosives`])
    pub pools: Vec<explosives::Pool>,
    /// the obstacles in the trees of the projectiles flying (GC's shield), placed for this frame
    /// ([`Sim::obstacles`]), and in each projectile's frame (`projectiles`, the template's)
    pub carried: Vec<obstacle::Obstacle>,
    pub carried_from: Vec<(usize, std::sync::Arc<Vec<obstacle::Obstacle>>)>,
}

impl<'w> Sim<'w> {
    pub fn new(world: &'w World, dt: f64) -> Sim<'w> {
        Sim {
            world,
            clock: clock::Clock { dt, time: 0.0, steps: 0 },
            bodies: Vec::new(),
            vehicles: Vec::new(),
            projectiles: Vec::new(),
            spent: Default::default(),
            journal: journal::Journal::default(),
            role: replica::Role::default(),
            spawner_frames: None,
            damage: damage::DamageState::default(),
            spawners: Vec::new(),
            items: Vec::new(),
            modes: modes::Modes::default(),
            rules: round::RoundRules::default(),
            players: Vec::new(),
            rng: players::Rng(0x9E37_79B9_7F4A_7C15),
            round: round::RoundState::default(),
            drivers: bot_api::Drivers::default(),
            depots: Vec::new(),
            depots_placed: false,
            pools: Vec::new(),
            carried: Vec::new(),
            carried_from: Vec::new(),
        }
    }

    pub fn spawn(&mut self, pos: [f64; 3], vel: [f64; 3], gravity_modifier: f64, drag: f64, mass: f64, radius: f64) -> usize {
        self.bodies.push(Body { pos, vel, acc: [0.0; 3], gravity_modifier, drag, mass, radius, contact: None });
        self.bodies.len() - 1
    }

    /// Add a vehicle at `pos`, turned by `rotation` (yaw, pitch, roll in degrees, as
    /// `Object.rotation`), at rest.
    pub fn spawn_vehicle(&mut self, template: &str, pos: [f32; 3], rotation: [f32; 3]) -> Option<usize> {
        let mut v = self.world.vehicle(template)?;
        v.craft.body.pos = pos;
        v.craft.body.rows = craft::ypr(rotation);
        // a server's place free for a while takes it (they only grew, and each one costs every client's
        // snapshots kept, `HISTORY` of them: 1650 in a two-hour soak, 1.5 GB); a copy follows the server's
        let i = match self.free_place() {
            Some(i) => {
                self.depots.retain(|d| d.on_projectile || d.on.is_none_or(|(k, _)| k != i));
                // what still names it is removed too ([`Sim::named_places`] counts the live): a dead soldier left inside it
                // would ride a seat the new one does not have (the monkey, GC_BonusRace)
                for e in self.vehicles.iter_mut().map(|v| &mut v.entry) {
                    if e.inside == Some(i) {
                        e.inside = None;
                    }
                    for r in [&mut e.return_to, &mut e.requested] {
                        if r.is_some_and(|r| r.0 == i) {
                            *r = None;
                        }
                    }
                    if e.exited.is_some_and(|r| r.0 == i) {
                        e.exited = None;
                    }
                }
                // (a new generation: what a bot's mind still names by the old handle finds nothing)
                let g = self.vehicles[i].place_gen.wrapping_add(1);
                self.vehicles[i] = v;
                self.vehicles[i].place_gen = g;
                i
            }
            None => {
                self.vehicles.push(v);
                self.vehicles.len() - 1
            }
        };
        self.vehicles[i].id = i;
        if !self.role.copy {
            self.journal.made += 1;
            self.vehicles[i].born = self.journal.made;
        }
        self.add_vehicle_depots(i);
        Some(i)
    }

    /// The first place of `vehicles` removed for [`REUSE_AFTER`] s that nothing names any more ([`Sim::named_places`]).
    /// None in a copy. A bot's mind may still name one: the place's generation goes up ([`mover::Vehicle::place_gen`]), so
    /// its old handle finds nothing, never the new object.
    fn free_place(&self) -> Option<usize> {
        if self.role.copy {
            return None;
        }
        let first = self.vehicles.iter().position(|v| self.past_reuse(v))?;
        let named = self.named_places();
        (first..self.vehicles.len()).find(|&k| self.past_reuse(&self.vehicles[k]) && named[k].is_none())
    }

    /// Removed for [`REUSE_AFTER`] s or more.
    pub fn past_reuse(&self, v: &mover::Vehicle) -> bool {
        v.removed && v.gone_since.is_some_and(|t| self.clock.time - t >= REUSE_AFTER)
    }

    /// By place of `vehicles`: who still names it (the first found), so [`Sim::spawn_vehicle`] does not take it: a player's
    /// soldier, a live object's seat or launcher, a spawner's object, a live projectile's owner, a carried kit, an
    /// explosives' pool.
    pub fn named_places(&self) -> Vec<Option<&'static str>> {
        let mut named = vec![None; self.vehicles.len()];
        let mut name = |k: usize, by: &'static str| {
            if let Some(n) = named.get_mut(k) {
                n.get_or_insert(by);
            }
        };
        for p in &self.players {
            p.soldier.map(|k| name(k, "player"));
        }
        for v in self.vehicles.iter().filter(|v| !v.removed) {
            let e = &v.entry;
            // (a soldier killed in a seat leaves it at once since `update_players`; `loadtest --every` lists any left)
            e.occupants.iter().flatten().for_each(|&k| name(k, "seat"));
            for k in [e.driver, e.inside, e.return_to.map(|r| r.0), e.exited.map(|r| r.0), e.requested.map(|r| r.0)].into_iter().flatten() {
                name(k, "entry");
            }
        }
        for s in &self.spawners {
            for m in s.slots.iter().flatten().chain(&s.last) {
                if let create::Made::Vehicle(k) = m {
                    name(*k, "spawner");
                }
            }
        }
        // (the live ones: a spent shell keeps its owner, a dead soldier's kit its carrier)
        for q in self.projectiles.iter().filter(|q| q.alive()) {
            q.owner.map(|k| name(k, "projectile"));
        }
        for t in self.items.iter().filter(|t| !t.removed) {
            t.carrier.map(|k| name(k, "kit"));
        }
        for p in &self.pools {
            name(p.soldier, "pool");
        }
        named
    }

    /// The logs, past `2 * keep` entries each, cut to the last `keep` (some 33 MB an hour of a round of 64
    /// clients in Berlin). A server's and, since a client's: what it cuts of the logs a client reads goes to
    /// [`Sim::log_base`], and their readers' cursors count from the round's start ([`log_since`]). Not for a test that
    /// reads them by length.
    pub fn trim_logs(&mut self, keep: usize) {
        fn cut<T>(v: &mut Vec<T>, keep: usize) -> usize {
            if v.len() <= 2 * keep {
                return 0;
            }
            let n = v.len() - keep;
            v.drain(..n);
            v.shrink_to(2 * keep);
            n
        }
        let b = &mut self.journal.log_base;
        b.damage += cut(&mut self.journal.damage_log, keep);
        b.explosions += cut(&mut self.journal.explosions, keep);
        b.impacts += cut(&mut self.journal.impacts, keep);
        cut(&mut self.journal.hit_log, keep);
        cut(&mut self.journal.pushes, keep);
        cut(&mut self.journal.spawn_log, keep);
        cut(&mut self.journal.kit_log, keep);
        b.fired += cut(&mut self.journal.fired, keep);
        b.player += cut(&mut self.journal.player_log, keep);
        b.radio += cut(&mut self.journal.radio_log, keep);
        cut(&mut self.journal.supply_log, keep);
    }

    /// Vehicle or soldier `i` out of the world for good (`removed`).
    pub fn retire(&mut self, i: usize) {
        self.vehicles[i].removed = true;
    }

    /// `fireBarrel` with a PlayerControlObject for a projectile (a guided missile: XPack2's
    /// Wasserfall, DC's Patriot, SA-3, AIM-54, Tomahawk): the object made at the barrel, turned as
    /// it and at rest (its `velocity` unused), and the one shooting from the launcher gets out of
    /// it (`exitVehicle`) and into the new one's driver's seat (`enterVehicle`, its toggle time
    /// now), remembering the launcher to go back to ([`crate::entry::Entry::return_to`]). Nobody in the seat (`getRootParent` of the player's object is not the launcher):
    /// nothing. The new vehicle's index.
    pub fn launch(&mut self, launcher: usize, seat: usize, template: &str, shot: &projectile::Shot) -> Option<usize> {
        let soldier = self.vehicles[launcher].entry.occupants.get(seat).copied().flatten()?;
        let Some(create::Made::Vehicle(k)) = self.instantiate(template, shot.pos, shot.rows) else { return None };
        self.exit(soldier, launcher);
        self.enter(soldier, k, 0);
        self.vehicles[soldier].entry.return_to = Some((launcher, seat));
        // (F3: no special rule for a vehicle born inside another; `enter` gives it `setForwardMod` m/s along its
        // forward, and with the Patriot's tubes level the frame after its birth hits `Patriot_Base`'s top faces, the
        // launcher's own passive collision mesh (its root has `hasCollisionPhysics` and no mobile physics), as the original's
        // does; raised, it clears the box and flies; the F-14A's Aim-54 never meets its plane: entry.rs `guided` tests)
        Some(k)
    }

    /// What the level's spawners make at the start of the round ([`Sim::instantiate`]: vehicles,
    /// and the kits and bundles that lie there, `items`), in the server's order; the vehicles
    /// empty (engines off), their indices.
    /// They are the spawners' first objects: their countdowns start at `calcSpawnDelay` (with
    /// no players, `maxSpawnDelay`), and run while the slot is empty. Their team is the
    /// spawner's `Object.setTeam` (in a round, [`Sim::start_conquest`] gives them their control
    /// points' first).
    pub fn spawn_level_vehicles(&mut self) -> Vec<usize> {
        self.spawn_level_vehicles_after(&[])
    }

    /// [`Sim::spawn_level_vehicles`] once the control points' `reset` (`restartMap`) has switched
    /// the spawners of their `objectSpawnerId` (`sw`): the server makes the first objects in the
    /// first the spawner's frame update handling, after `restartMap`, so a spawner with a control
    /// point makes that point's team's (a neutral one's, nothing) whatever its `setTeam` says
    fn spawn_level_vehicles_after(&mut self, sw: &[conquest::SpawnerSwitch]) -> Vec<usize> {
        let mut out = Vec::new();
        // the vehicles placed in the scripts (before the spawners make theirs), asleep: made while
        // the level loads, the original's mortar of DC_First_Light never moves,
        // where a spawner's settles
        for (t, pos, rot) in self.world.placed_vehicles() {
            let rows = ref2::set_rotation(rot.map(|a| a as f32));
            if let Some(create::Made::Vehicle(i)) = self.instantiate(&t, pos.map(|c| c as f32), rows) {
                self.vehicles[i].set_engines_on(false);
                self.vehicles[i].craft.body.sleepiness = 0;
                out.push(i);
            }
        }
        for s in self.world.level.spawners.iter().rev() {
            let parent = if s.in_static.is_some() { create::Parent::Static } else { create::Parent::Level };
            let mut st = damage::SpawnerState::of(s, parent);
            for w in sw.iter().filter(|w| s.os_id == Some(w.os_id)) {
                st.switch(w);
            }
            self.spawners.push(st);
            self.start_spawner(self.spawners.len() - 1, &mut out);
        }
        out
    }

    /// A spawner's first object as the round starts (the first `handleFrameUpdate`, unless
    /// `spawnDelayAtStart`), made by [`Sim::instantiate`]; the spawners in its tree (a ship's
    /// planes and boats) make theirs at once in turn, with their axes. The vehicles made
    /// go to `out`.
    fn start_spawner(&mut self, k: usize, out: &mut Vec<usize>) {
        let s = &self.spawners[k];
        if !s.active || s.countdown != -1.0 || s.template.is_empty() {
            return;
        }
        let (t, at, rows) = (s.template.clone(), s.at, s.rows.unwrap_or_else(|| craft::ypr(s.rotation)));
        if self.human_near(&t, at) {
            return;
        }
        let first = self.spawners.len();
        let Some(o) = self.instantiate(&t, at, rows) else { return };
        if let create::Made::Vehicle(i) = o {
            self.vehicles[i].set_engines_on(false);
            self.vehicles[i].entry.abandon = self.spawners[k].abandon();
            if self.spawners[k].team_on_vehicle {
                self.vehicles[i].team = self.spawners[k].team;
            }
            out.push(i);
        }
        let s = &mut self.spawners[k];
        s.slots[0] = Some(o);
        s.last = Some(o);
        s.countdown = s.delay;
        s.holding = s.hold;
        for c in first..self.spawners.len() {
            self.start_spawner(c, out);
        }
    }

    /// The round as the server starts it: `restartMap` (the tickets and the control
    /// points' `reset`, which hand their teams to their spawn groups and spawners, and switch off
    /// those of the neutral ones), then the spawners' first objects (`spawn_level_vehicles`, made
    /// in the first `handleFrameUpdate`: of the team their control point gave them).
    pub fn start_conquest(&mut self) -> Vec<usize> {
        let mut c = conquest::Conquest::from_db(&self.world.db);
        // the tickets run down in conquest, coop and objective mode only (`gameStatusPlaying`)
        let mode = self.world.mode.as_str();
        c.bleed = !matches!(mode, "GPM_CTF" | "GPM_TDM");
        self.modes.ctf = (mode == "GPM_CTF").then(|| ctf::Ctf::from_db(&self.world.db));
        self.modes.objectives = (mode == "GPM_OBJECTIVEMODE").then(|| objective::Objectives::from_db(&self.world.db));
        // (conquest's and coop's file; CTF and TDM have their own: the game's load)
        let file = match mode {
            "GPM_CTF" => "Bf1942/Game/ScoreManagerSettingsCTF.con",
            "GPM_TDM" => "Bf1942/Game/ScoreManagerSettingsTDM.con",
            _ => "Bf1942/Game/ScoreManagerSettings.con",
        };
        if let Ok(b) = self.world.vfs.read(file) {
            self.rules.scores = players::ScoreRules::parse(&String::from_utf8_lossy(&b));
        }
        if let (Some(t), Some(w)) = (c.rules.time_to_next_wave, self.round.waves.as_mut()) {
            w.add = t;
        }
        let mut sw = Vec::new();
        c.restart(&mut sw);
        // in objective mode the defenders' tickets are 100, not scaled (`gamaStatusFirstPreGame`,
        // mode 5: "rem defender tickets are set to 100" in the levels' `ObjectiveMode.con`)
        if let Some(o) = self.modes.objectives.as_ref().filter(|o| (1..=2).contains(&o.defender)) {
            c.tickets[o.defender as usize - 1] = 100;
        }
        if self.modes.objectives.is_some() {
            c.normal = Some(c.tickets);
        }
        let vs = self.spawn_level_vehicles_after(&sw);
        self.modes.conquest = Some(c);
        self.sync_carried_spawn_points();
        vs
    }

    /// The spawn points vehicles carry (a carrier's, a landing craft's; their groups are the
    /// global ones of `GlobalSpawnGroups.con`): a new vehicle's go in their groups, a gone or
    /// destroyed one's out, and the rest go where their vehicle is.
    pub fn sync_carried_spawn_points(&mut self) {
        let Some(c) = self.modes.conquest.as_mut() else { return };
        let vehicles = &self.vehicles;
        let lost = |k: usize| {
            let v = &vehicles[k];
            v.removed || !v.in_world() || v.armor.as_ref().is_some_and(|a| a.hp <= 0.0)
        };
        for g in c.groups.values_mut() {
            let n = g.spawns.len();
            g.spawns.retain(|s| s.on.is_none_or(|(k, _)| !lost(k)));
            g.points -= n - g.spawns.len();
            for s in g.spawns.iter_mut() {
                if let Some((k, lp)) = s.on {
                    let b = &vehicles[k].craft.body;
                    let p = [0, 1, 2].map(|j| lp[0] * b.rows[0][j] + lp[1] * b.rows[1][j] + lp[2] * b.rows[2][j]);
                    s.pos = [0, 1, 2].map(|j| b.pos[j] + p[j]);
                }
            }
        }
        for k in 0..vehicles.len() {
            if lost(k) || !c.carried.insert(k) {
                continue;
            }
            for (name, m) in bytefield_world::child_spawn_points(&self.world.db, &vehicles[k].template) {
                let Some(t) = self.world.db.object_template(&name) else { continue };
                let Some(g) = conquest::SpawnPoint::group_of(t) else { continue };
                let lp = [m.0[3][0] as f32, m.0[3][1] as f32, m.0[3][2] as f32];
                let mut s = conquest::SpawnPoint::of(t, [0.0; 3], [0.0; 3]);
                s.on = Some((k, lp));
                let b = &vehicles[k].craft.body;
                let p = [0, 1, 2].map(|j| lp[0] * b.rows[0][j] + lp[1] * b.rows[1][j] + lp[2] * b.rows[2][j]);
                s.pos = [0, 1, 2].map(|j| b.pos[j] + p[j]);
                let e = c.groups.entry(g).or_default();
                e.points += 1;
                e.spawns.push(s);
            }
        }
    }

    /// The spawner's setting of team + `setActive` from a control point (`CPEnable`/`CPDisable`): the
    /// next object is the new team's; switching it (on or off) restarts its countdown (unless it
    /// is −1, at the start).
    pub fn switch_spawners(&mut self, sw: &[conquest::SpawnerSwitch]) {
        for w in sw {
            for s in self.spawners.iter_mut().filter(|s| s.os_id == Some(w.os_id)) {
                s.team = w.team;
                s.template = s.templates.iter().rev().find(|(t, _)| *t == w.team).map_or(String::new(), |(_, n)| n.clone());
                s.active = w.active;
                if s.countdown != -1.0 {
                    s.countdown = s.delay;
                }
            }
        }
    }

    /// The players a control point sees: the soldiers with a team that are still there.
    pub fn players(&self) -> Vec<conquest::Player> {
        self.vehicles
            .iter()
            .filter(|v| v.team != 0 && !v.removed && v.has_soldier_contact() && v.armor.as_ref().is_none_or(|a| !a.destroyed))
            .map(|v| (v.team, v.craft.body.pos))
            .collect()
    }

    /// `gotControl`'s points (`handleFrameUpdate`): `Attack` to each alive player of the team
    /// that took a point (the log's entries from `seen` on) within its radius.
    fn score_captures(&mut self, seen: usize) {
        let Some(c) = self.modes.conquest.as_ref() else { return };
        let taken: Vec<(u32, [f32; 3], f32)> = c.log[seen..]
            .iter()
            .filter_map(|&(_, k, e)| match e {
                conquest::CpEvent::Taken(team) => Some((team, c.points[k].pos, c.points[k].template.radius)),
                _ => None,
            })
            .collect();
        for (team, at, radius) in taken {
            for k in 0..self.players.len() {
                let p = &self.players[k];
                let Some(v) = p.soldier.filter(|_| p.alive && p.team == team).and_then(|s| self.vehicles.get(s)) else { continue };
                let d = v.craft.body.pos.iter().zip(at).map(|(a, b)| (a - b) * (a - b)).sum::<f32>().sqrt();
                if d <= radius {
                    self.score(k, players::Score::Attack);
                }
            }
        }
    }

    /// One frame: the point bodies, then the vehicles as the server does it, each phase for
    /// every vehicle before the next (parts, integration, collision).
    pub fn step(&mut self) {
        self.step_ordered(false);
    }

    /// `parts_last`: integration, collision, then the parts of the next frame. The same cycle,
    /// cut elsewhere: what the original's reference runs record (the body at the entry of its update,
    /// with the parts already updated), so a sim started from a record lines up with the next.
    pub fn step_ordered(&mut self, parts_last: bool) {
        let dt = self.clock.dt;
        for v in self.vehicles.iter_mut() {
            v.gone_since = if v.removed { v.gone_since.or(Some(self.clock.time)) } else { None };
        }
        for (k, q) in self.projectiles.iter_mut().enumerate() {
            if !q.alive() && q.gone_since.is_none() {
                q.gone_since = Some(self.clock.time);
                self.spent.push_back(k);
            }
        }
        self.think_minds();
        self.think_better_bots();
        // the soldiers see the world time of the frame (`mWorldTime`)
        let wt = if self.round.playing { self.clock.time } else { 0.0 };
        for v in self.vehicles.iter_mut() {
            if let Some(m) = v.soldier_mut() {
                m.world_time = wt;
            }
        }
        self.clock.time += dt;
        self.clock.steps += 1;
        self.step_vehicles(dt as f32, parts_last);
        self.step_projectiles(dt as f32, parts_last);
        self.use_medpacks();
        self.use_repairpacks();
        self.use_detonators();
        self.supply();
        self.take_shots();
        self.update_pools();
        // then the game's part of the frame: the explosions' damage since the last
        // one (`gameStatusPlaying`), the armors and wrecks, the spawners
        self.apply_pending_damage();
        // (in the pre-game, `updateGameLogic` runs `gameStatusPreGame` instead: none of the rest)
        // (`updateGameLogic` counts the people first: once they are enough, a round already played starts again)
        self.count_players();
        let pre = self.pre_game(dt as f32);
        if !pre && self.modes.conquest.is_some() && self.round.round_end.is_none() {
            self.sync_carried_spawn_points();
            let players = self.players();
            let step = self.clock.steps;
            let seen = self.modes.conquest.as_ref().unwrap().log.len();
            self.objective_restore();
            let sw = self.modes.conquest.as_mut().unwrap().frame(step, dt as f32, &players);
            self.switch_spawners(&sw);
            self.score_captures(seen);
            self.ctf_frame(dt as f32);
            self.objective_frame(dt as f32);
            self.objective_tickets();
        }
        self.update_armors(dt as f32);
        if !pre && self.round.round_end.is_none() {
            self.update_players(dt as f32);
        }
        if !pre {
            self.end_of_round(dt as f32);
        }
        // the spawners' frame updates (the real clock's, when given: a reference run's)
        match self.spawner_frames.as_mut() {
            // (the world is cleared until the next round)
            _ if self.round.round_end.is_some() => {}
            Some(f) => {
                for dt in std::mem::take(f) {
                    self.update_spawners(dt);
                    self.abandoned(dt);
                }
            }
            None => {
                self.update_spawners(dt as f32);
                self.abandoned(dt as f32);
            }
        }
        let wind = [0.0; 3];
        for b in self.bodies.iter_mut().filter(|b| b.contact.is_none()) {
            // drag (in air; the under-water share comes with water contact for bodies that sink)
            if b.drag > 0.0 && b.mass > 1e-5 {
                let k = std::f64::consts::PI * b.radius * b.radius * b.drag / b.mass;
                for i in 0..3 {
                    b.acc[i] -= k * (b.vel[i] - wind[i]);
                }
            }
            // clamp, then 4 substeps of dt/4
            let a2 = b.acc.iter().map(|a| a * a).sum::<f64>();
            if a2 > MAX_ACCELERATION * MAX_ACCELERATION {
                let f = MAX_ACCELERATION / a2.sqrt();
                b.acc = b.acc.map(|a| a * f);
            }
            let prev = b.pos;
            let h = dt / 4.0;
            for _ in 0..4 {
                for i in 0..3 {
                    b.vel[i] += b.acc[i] * h;
                    b.pos[i] += b.vel[i] * h;
                }
            }
            b.acc = [0.0, -GRAVITY * b.gravity_modifier, 0.0];
            match self.world.surface_at(b.pos[0], b.pos[2]) {
                None => b.contact = Some((Contact::OutOfWorld, self.clock.time)),
                Some((h, water)) if b.pos[1] <= h => {
                    // place it on the surface where the step crossed it (linear in the step)
                    let h0 = self.world.surface_at(prev[0], prev[2]).map_or(h, |s| s.0);
                    let (d0, d1) = (prev[1] - h0, b.pos[1] - h);
                    let f = if d0 > d1 { (d0 / (d0 - d1)).clamp(0.0, 1.0) } else { 1.0 };
                    for i in 0..3 {
                        b.pos[i] = prev[i] + (b.pos[i] - prev[i]) * f;
                    }
                    b.pos[1] = self.world.surface_at(b.pos[0], b.pos[2]).map_or(h, |s| s.0);
                    b.contact = Some((if water { Contact::Water } else { Contact::Ground }, self.clock.time - dt * (1.0 - f)));
                }
                _ => {}
            }
        }
        // with debug assertions (`cargo build --profile checked`):
        // nothing in the world is NaN or infinite after a step, said with what and where
        #[cfg(debug_assertions)]
        self.assert_finite();
    }

    /// Panics, naming the object, the field and where it was, if a vehicle's body, armor, a projectile or a point body is
    /// not finite.
    #[cfg(debug_assertions)]
    fn assert_finite(&self) {
        let ok = |v: &[f32]| v.iter().all(|x| x.is_finite());
        let at = |p: &[f32; 3]| format!("[{:.1}, {:.1}, {:.1}]", p[0], p[1], p[2]);
        for (i, v) in self.vehicles.iter().enumerate().filter(|(_, v)| !v.removed) {
            let b = &v.craft.body;
            let bad = [("pos", ok(&b.pos)), ("vel", ok(&b.vel)), ("rotvel", ok(&b.rotvel)), ("rows", b.rows.iter().all(|r| ok(r)))];
            if let Some((field, _)) = bad.iter().find(|(_, good)| !good) {
                panic!("not finite after step {} (t {:.2} s): vehicle {i} {} {field} = pos {:?} vel {:?} rotvel {:?} (before at {})", self.clock.steps, self.clock.time, v.template, b.pos, b.vel, b.rotvel, at(&b.pos));
            }
            if let Some(a) = v.armor.as_ref().filter(|a| !a.hp.is_finite()) {
                panic!("not finite after step {} (t {:.2} s): vehicle {i} {} hp = {} at {}", self.clock.steps, self.clock.time, v.template, a.hp, at(&b.pos));
            }
        }
        for (i, p) in self.projectiles.iter().enumerate().filter(|(_, p)| p.alive()) {
            if !ok(&p.pos) || !ok(&p.vel) || !p.rows.iter().all(|r| ok(r)) {
                panic!("not finite after step {} (t {:.2} s): projectile {i} {} pos {:?} vel {:?} from {}", self.clock.steps, self.clock.time, p.template.name, p.pos, p.vel, at(&p.start));
            }
        }
        for (i, b) in self.bodies.iter().enumerate() {
            if !b.pos.iter().chain(&b.vel).all(|x| x.is_finite()) {
                panic!("not finite after step {} (t {:.2} s): point body {i} pos {:?} vel {:?}", self.clock.steps, self.clock.time, b.pos, b.vel);
            }
        }
    }

    fn step_vehicles(&mut self, dt: f32, parts_last: bool) {
        // (taken while `self` is borrowed below; put back at the end)
        let carried = std::mem::take(&mut self.carried);
        let env = self.world.surroundings(&carried);
        if !parts_last {
            self.update_entries();
            self.exited_messages(dt as f64);
            self.kit_rooms();
            for v in self.vehicles.iter_mut().filter(|v| v.in_world()) {
                v.update_parts(dt, env.ground);
            }
        }
        for v in self.vehicles.iter_mut().filter(|v| v.in_world()) {
            v.integrate(dt);
        }
        mover::check_all(&mut self.vehicles, &env, dt);
        self.crash_damage();
        mover::solve_all(&mut self.vehicles, dt);
        self.ride();
        if parts_last {
            self.update_entries();
            self.kit_rooms();
            for v in self.vehicles.iter_mut().filter(|v| v.in_world()) {
                v.update_parts(dt, env.ground);
            }
        }
        self.carried = carried;
    }

    /// What an object made in the middle of a frame gets of that frame (the original's recording harness creates it from inside another
    /// body's integration): the collision pass and its bundles' `handleUpdate`, but
    /// neither the parts' forces nor an integration. a helicopter on its wheels leaves the ground at 16.8 m/s in
    /// the original's first record, its rotor a step ahead; a plane in the air has no force of its own.
    pub fn creation_frame(&mut self) {
        let dt = self.clock.dt as f32;
        let carried = std::mem::take(&mut self.carried);
        let env = self.world.surroundings(&carried);
        mover::check_all(&mut self.vehicles, &env, dt);
        mover::solve_all(&mut self.vehicles, dt);
        for v in self.vehicles.iter_mut().filter(|v| v.in_world() && v.soldier().is_none()) {
            let g = env.ground;
            // The engine's update handling runs with the engine still off (set by `handleMessage` 4 or 0x13, entering):
            // rpm 0, the gear timer down a step, and the load the collision pass just left taken and zeroed (the
            // engines of 64 ground cases of the original equal the original's first record, the gear timer included; MLRS
            // on Bocage from 6.2 m to 0.0)
            let mut on = Vec::new();
            for p in v.craft.parts.iter_mut() {
                if let crate::craft::Kind::Engine(e) = &mut p.kind {
                    on.push(std::mem::replace(&mut e.on, false));
                }
            }
            v.craft.handle_parts(dt, &|x, z| Some(g.height_and_normal(x, z).0));
            let mut on = on.into_iter();
            for p in v.craft.parts.iter_mut() {
                if let crate::craft::Kind::Engine(e) = &mut p.kind {
                    e.on = on.next().unwrap_or(true);
                }
            }
        }
        self.carried = carried;
    }

    /// One step of vehicle `i` alone: its
    /// parts, its integration and its contacts with the level (not with the other objects);
    /// the clock and everything else stay. False if it cannot (gone, or riding in a vehicle).
    pub fn step_alone(&mut self, i: usize, k: u32) -> bool {
        self.step_alone_firing(i, k, false)
    }

    /// [`Sim::step_alone`], and with `fire` its shots taken (a client of the original server: its own
    /// shots as it presses, in a `remote` copy: for the eye).
    pub fn step_alone_firing(&mut self, i: usize, k: u32, fire: bool) -> bool {
        let dt = self.clock.dt as f32;
        let wt = if self.round.playing { self.clock.time + self.clock.dt * k as f64 } else { 0.0 };
        let env = self.world.surroundings(&self.carried);
        let v = &mut self.vehicles[i];
        if !v.in_world() || v.removed || v.entry.inside.is_some() {
            return false;
        }
        if let Some(m) = v.soldier_mut() {
            m.world_time = wt;
        }
        v.update_parts(dt, env.ground);
        v.integrate(dt);
        let one = std::slice::from_mut(v);
        mover::check_all(one, &env, dt);
        mover::solve_all(one, dt);
        if fire {
            self.take_shots();
            return true;
        }
        // its shots are the server's to make
        if let Some(m) = one[0].soldier_mut() {
            m.shots.clear();
            m.mark = None;
        }
        one[0].shots.clear();
        true
    }

    /// A copy of another's game (`remote`) a tick on: its clock, and its projectiles (the shots of [`Sim::fire_remote`]).
    pub fn step_remote(&mut self) {
        self.clock.time += self.clock.dt;
        self.clock.steps += 1;
        self.step_projectiles(self.clock.dt as f32, false);
    }

    /// Soldier `i` fires the weapon in its hand once, from its camera, as it would itself (for the eye: a client of
    /// the original server is not sent the others' shots, only that they fire). False with no weapon.
    pub fn fire_remote(&mut self, i: usize) -> bool {
        let v = &mut self.vehicles[i];
        let Some(m) = v.movement.as_mut().filter(|m| m.weapon.is_some()) else { return false };
        m.fire(&v.body.craft.body, rigid::GRAVITY_Y);
        self.take_shots();
        true
    }

    /// The soldier's use med pack for each soldier that pressed fire with the medic's pack: the first
    /// other soldier (whatever its side) within the medic's `healDistance` (the soldiers' data:
    /// 10 m), alive and hurt, gets `healFactor` (0.25) of health back (a negative damage); with
    /// none, the medic himself, if hurt, `selfHealFactor` (0.15). If it healed, the pack hears the fire message (it has no
    /// projectile: it only spends its rounds). (The server's order is its object manager's query;
    /// here, the soldiers' order.)
    fn use_medpacks(&mut self) {
        let hurt = |v: &mover::Vehicle| v.in_world() && v.soldier().is_some() && v.armor.as_ref().is_some_and(|a| !a.destroyed && a.hp < a.max_hp);
        let medics: Vec<usize> = self.vehicles.iter().enumerate().filter(|(_, v)| v.soldier().is_some_and(|m| m.medpack)).map(|(i, _)| i).collect();
        for i in medics {
            let p = self.vehicles[i].craft.body.pos;
            let [distance, factor, self_factor] = self.vehicles[i].soldier().unwrap().heal;
            let other = self.vehicles.iter().enumerate().find(|&(j, v)| {
                let q = v.craft.body.pos;
                j != i && hurt(v) && (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2) < distance * distance
            });
            let healed = match other.map(|(j, _)| j) {
                Some(j) => {
                    self.journal.heals_by[if self.vehicles[j].team == self.vehicles[i].team { 1 } else { 2 }] += 1;
                    self.give_damage(j, -factor, damage::Source::Heal);
                    true
                }
                None if self.vehicles[i].armor.as_ref().is_some_and(|a| !a.destroyed && a.hp < a.max_hp) => {
                    self.journal.heals_by[0] += 1;
                    self.give_damage(i, -self_factor, damage::Source::Heal);
                    true
                }
                None => false,
            };
            if healed {
                self.journal.heals += 1;
                let g = crate::rigid::GRAVITY_Y;
                let v = &mut self.vehicles[i];
                let body = v.craft.body.clone();
                v.soldier_mut().unwrap().medpack_fired(&body, g);
            }
        }
    }

    /// The soldier's use repair pack for each soldier that pressed fire with the wrench. Crouched,
    /// the first mine or charge (`LandmineProjectile`, `ExpPackProjectile`: anyone's) within its
    /// radius + `disarmMinesDistance` is gone (`resetProjectile`), and the soldier's mines (item
    /// 5) or charges (item 4) get a round back if they shoot that projectile; standing or lying,
    /// the nearest vehicle of its team or none (whose radius is no bigger than the nearest so
    /// far's), within its radius + `repairDistance`, not destroyed and hurt, gets `repairFactor`
    /// hit points, but a ship (`VCSea`) at or below its critical damage, which
    /// sinks; a field then says how whole it is above the critical damage. Either way the wrench
    /// hears the fire message (it spends its rounds). (The server's order is its object
    /// manager's query; here, the projectiles' and the vehicles'.)
    fn use_repairpacks(&mut self) {
        let engineers: Vec<usize> = self.vehicles.iter().enumerate().filter(|(_, v)| v.soldier().is_some_and(|m| m.repairpack)).map(|(i, _)| i).collect();
        let d2 = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
        for i in engineers {
            let p = self.vehicles[i].craft.body.pos;
            let m = self.vehicles[i].soldier().unwrap();
            let [reach, factor, disarm] = m.repair;
            let used = if m.pose() == 1 {
                let found = self.projectiles.iter().position(|q| {
                    let n = &q.template.name;
                    q.alive() && (n.eq_ignore_ascii_case("ExpPackProjectile") || n.eq_ignore_ascii_case("LandmineProjectile")) && d2(q.pos, p) <= (q.template.radius + disarm).powi(2)
                });
                if let Some(k) = found {
                    let name = self.projectiles[k].template.name.clone();
                    let slot = if name.eq_ignore_ascii_case("ExpPackProjectile") { 4 } else { 5 };
                    let m = self.vehicles[i].soldier_mut().unwrap();
                    if let Some(w) = m.weapon.iter_mut().chain(m.stowed.iter_mut()).find(|w| w.template.item_index == slot && w.template.projectile.eq_ignore_ascii_case(&name)) {
                        w.reload_ammo(1);
                    }
                    self.projectiles[k].end = Some(projectile::End::Removed);
                    self.journal.disarmed += 1;
                }
                found.is_some()
            } else {
                let team = self.vehicles[i].team;
                let mut best: Option<(usize, f32, f32)> = None;
                for (j, v) in self.vehicles.iter().enumerate() {
                    let Some(a) = v.armor.as_ref() else { continue };
                    if j == i || v.soldier().is_some() || !v.in_world() || !(v.team == 0 || v.team == team) {
                        continue;
                    }
                    let d = d2(v.craft.body.pos, p);
                    if d > (v.radius + reach).powi(2) || a.destroyed || !(a.hp < a.max_hp) {
                        continue;
                    }
                    if a.hp <= a.template.critical_damage && self.world.vehicle_category(&v.template) == 1 {
                        continue;
                    }
                    let take = match best {
                        Some((_, bd, br)) => d < bd && v.radius <= br,
                        None => true,
                    };
                    if take {
                        best = Some((j, d, v.radius));
                    }
                }
                if let Some((j, _, _)) = best {
                    self.give_damage(j, -factor, damage::Source::Repair);
                    let a = self.vehicles[j].armor.as_ref().unwrap();
                    let c = a.template.critical_damage;
                    let status = (a.hp - c) / (a.max_hp - c);
                    self.vehicles[i].soldier_mut().unwrap().repair_status = status;
                    self.journal.repairs += 1;
                }
                best.is_some()
            };
            if used {
                let g = crate::rigid::GRAVITY_Y;
                let v = &mut self.vehicles[i];
                let body = v.craft.body.clone();
                v.soldier_mut().unwrap().medpack_fired(&body, g);
            }
        }
    }

    /// The soldiers' shots of this frame become projectiles (logged in `fired`); they first move
    /// in the next frame (the server fires them inside the soldier's physics update, after the
    /// projectiles moved).
    fn take_shots(&mut self) {
        self.place_scout_marks();
        let mut shots = Vec::new();
        let mut from_vehicles = Vec::new();
        for (i, v) in self.vehicles.iter_mut().enumerate() {
            if let Some(m) = v.soldier_mut() {
                shots.extend(m.shots.drain(..).map(|s| (i, s)));
            }
            from_vehicles.extend(v.shots.drain(..).map(|s| (i, s)));
        }
        // a vehicle's (its driver's: the shot has an owner and a start); it does not hit the
        // vehicle that fired it
        for (i, (template, shot, seat)) in from_vehicles {
            if self.world.is_player_control_object(&template) {
                self.launch(i, seat, &template, &shot);
                self.journal.fired.push((self.clock.steps, i, shot, None));
                continue;
            }
            let p = self.add_projectile(&template, &shot);
            if let Some(k) = p {
                self.projectiles[k].owner = Some(i);
                self.projectiles[k].seat = seat;
            }
            self.journal.fired.push((self.clock.steps, i, shot, p));
        }
        for (i, (template, shot)) in shots {
            // a pack's "shot" (no projectile) is not one
            if template.is_empty() {
                continue;
            }
            let p = self.add_projectile(&template, &shot);
            // with a player, `fireBarrel` gives it its owner and where it starts; without (a
            // console-made soldier) neither: it can hit its soldier, and its start is still the
            // pool's (far: the damage falls to its minimum)
            if let Some(k) = p {
                // the mines and charges take their slot in the weapon's pool
                if let Some(w) = self.vehicles[i].soldier().and_then(|m| m.weapon.as_ref()) {
                    let (name, mag) = (w.template.name.clone(), w.template.mag_size);
                    self.pool_shot(i, &name, mag, k);
                }
                let player = self.vehicles[i].soldier().is_some_and(|m| m.has_player);
                if player {
                    self.projectiles[k].owner = Some(i);
                } else {
                    self.projectiles[k].start = [0.0; 3];
                }
                // a new object, its `handleUpdate` comes after the weapon's in this frame: its
                // time to live already runs (its physics, from the next)
                self.projectiles[k].update_timer(self.clock.dt as f32);
            }
            self.journal.fired.push((self.clock.steps, i, shot, p));
        }
    }

    /// One phase of the vehicles' frame on its own (the clock does not move), for lining up with
    /// the server's order inside a frame: its `physicsNodeManager` integrates the objects one by
    /// one, so a reference run's second object can be recorded already integrated.
    /// `only`: just that vehicle (the parts and the integration; the collision is always for all).
    pub fn phase(&mut self, phase: Phase, only: Option<usize>) {
        let dt = self.clock.dt as f32;
        let carried = std::mem::take(&mut self.carried);
        let env = self.world.surroundings(&carried);
        let pick = |i: usize| only.is_none_or(|k| k == i);
        match phase {
            Phase::Parts => {
                self.kit_rooms();
                self.vehicles.iter_mut().enumerate().filter(|(i, v)| pick(*i) && v.in_world()).for_each(|(_, v)| v.update_parts(dt, env.ground))
            }
            Phase::Integrate => self.vehicles.iter_mut().enumerate().filter(|(i, v)| pick(*i) && v.in_world()).for_each(|(_, v)| v.integrate(dt)),
            Phase::Collide => {
                mover::check_all(&mut self.vehicles, &env, dt);
                self.crash_damage();
                mover::solve_all(&mut self.vehicles, dt);
            }
        }
        self.carried = carried;
    }

    /// Step until every body has landed or `max_time` passes; the trajectory of body `track`.
    pub fn run(&mut self, max_time: f64, track: usize) -> Vec<Sample> {
        let mut out = vec![Sample { t: self.clock.time, pos: self.bodies[track].pos, vel: self.bodies[track].vel }];
        while self.clock.time < max_time && self.bodies.iter().any(|b| b.contact.is_none()) {
            self.step();
            let b = &self.bodies[track];
            out.push(Sample { t: self.clock.time, pos: b.pos, vel: b.vel });
        }
        out
    }
}

impl contact::Ground for Level {
    fn height_and_normal(&self, x: f32, z: f32) -> (f32, [f32; 3]) {
        match Level::height_and_normal(self, x as f64, z as f64, Orientation::IDENTITY) {
            Some((h, n)) => (h as f32, [n[0] as f32, n[1] as f32, n[2] as f32]),
            // off the map the terrain repeats, the height map's height lookup masks its indices (the server: a Humvee
            // dropped at x = -69 rests at 84 m); before, flat ground at 0, a fall for whoever crosses the edge
            None => Level::engine_height_and_normal(self, x, z),
        }
    }
    // TODO: off the map the material is 0 here; does the server's `Materialmap` wrap as well?
    fn material(&self, x: f32, z: f32) -> u32 {
        self.material_at(x as f64, z as f64)
    }
    fn water_level(&self) -> f32 {
        self.terrain.water_level as f32
    }
}

/// BYTEFIELD_CONTACT_ORDER=0: the contactors in the template's order, to compare.
fn contact_order_v2() -> bool {
    crate::debug::options().contact_order
}

#[cfg(test)]
mod log_tests {
    use super::log_since;

    /// A cursor from the round's start reads what is new, across a cut of the log's front and into another round.
    #[test]
    fn a_cursor_reads_what_is_new_across_a_cut() {
        let mut seen = 0;
        let mut log: Vec<u32> = (0..10).collect();
        assert_eq!(log_since(&log, 0, &mut seen), &log[..]);
        log.extend(10..14);
        log.drain(..8);
        // (8 cut: 8..14 left, 10..14 new)
        assert_eq!(log_since(&log, 8, &mut seen), [10, 11, 12, 13]);
        assert_eq!(seen, 14);
        assert!(log_since(&log, 8, &mut seen).is_empty());
        // behind the cut: what is left
        let mut late = 3;
        assert_eq!(log_since(&log, 8, &mut late).len(), 6);
        // another round: the whole log
        let new: Vec<u32> = vec![0, 1];
        assert_eq!(log_since(&new, 0, &mut seen), [0, 1]);
        assert_eq!(seen, 2);
    }
}

#[cfg(test)]
mod parse_scale_tests {
    use super::parse_scale;

    #[test]
    fn scale_one_number_or_three() {
        assert_eq!(parse_scale("0.3"), [0.3; 3]);
        assert_eq!(parse_scale("0.3/1/1"), [0.3, 1.0, 1.0]);
        assert_eq!(parse_scale("0/1/1"), [1.0; 3]);
        assert_eq!(parse_scale("x"), [1.0; 3]);
    }
}
