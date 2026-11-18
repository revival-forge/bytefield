//! Contact with the terrain (`ResponsePhysics`), as read from the original game.
//!
//! Every frame, after the physics nodes have integrated, the collision pass runs for each object
//! whose template has response and mobile physics:
//! 1. `check_terrain` (`checkVsTerrain`): each vertex of the object's collision mesh (LOD 0)
//!    against the terrain; a vertex under it gives an impulse (`impulse_on`)
//! 2. then, object by object, `solve` (`solveImpulse`): the accumulated position adjust moves the
//!    root at once, and the accumulated velocity adjust becomes an acceleration at the mean
//!    contact point
//! 3. and `friction` (`addFriction`): resistance (an acceleration against the sliding speed)
//!    and friction (a "friction" acceleration, static or sliding, at the mean contact point)
//!
//! The engine's own quirks are kept: a vehicle's root and the bundle that holds its hull both
//! carry the hull's mesh, so the hull's contact counts twice.

use crate::vecmath::dot_zyx;
use std::collections::HashMap;
use std::sync::Arc;

use crate::obstacle::{FaceMesh, Obstacle, Obstacles};
use crate::rigid::RigidBody;

use crate::vecmath::V3;

/// Frames per second of the simulation (`g_simulationFps`).
pub const FPS: f32 = 30.0;
/// The quick test skips the vertices when both ends of the box are this many box-sizes above
/// the terrain (at most `QUICK_MAX` m).
pub const QUICK_FACTOR: f32 = 1.43;
pub const QUICK_MAX: f32 = 30.0;
/// Static and sliding friction limits: friction × these × 9.82 m/s² (per frame).
pub const STATIC_FRICTION: f32 = 2.25;
pub const SLIDING_FRICTION: f32 = 1.5;
pub const FRICTION_G: f32 = 9.82;
/// The spring force is d · gravity · this · strength: −1/9.82, so 1.5 · strength · d.
pub const SPRING_K: f32 = -0.101833;

/// `PhysicsGripFlags` (`ObjectTemplate.grip`).
pub mod grip {
    pub const NONE: u8 = 0;
    pub const CONTACT: u8 = 1;
    pub const ROLL: u8 = 2;
    pub const ENGINE: u8 = 4;
    pub const ROLL_WHEN_OCCUPIED: u8 = 8;
    pub const DUMMY: u8 = 0x20;
    pub const ENGINE_DUMMY: u8 = 0x24;
    /// set at run time while static friction holds
    /// The F-15C taking off rolls a step after the original (0.28 m
    /// at 4 s), level 3 since its wheels sit right at the static limit (0.664 against 0.6629 m/s at record 45)
    /// and the original gets there with 0.0008 m/s more, from its front spring's push in record 44 (z +0.024): the
    /// spring's d is a difference of world positions in f32 (`f32(pos + rel)` minus where the collision left the node),
    /// and there z + rel.z = 629.702178955078125 is a tie between two floats, rounded one way for one and the other way for
    /// the other (x87). 430 of the reference run's ~1430 spring records have such an ulp in x or z; ours never. Computing the node
    /// from the root in double changes nothing; from its parent's world frame (our chain), worse (30 cases over 1 m, 15
    /// before). A known failure
    pub const STATIC_FRICTION: u8 = 0x80;

    pub fn parse(s: &str) -> Option<u8> {
        Some(match s.to_ascii_lowercase().as_str() {
            "c_pgfnogrip" => NONE,
            "c_pgfcontactgrip" => CONTACT,
            "c_pgfrollgrip" => ROLL,
            "c_pgfenginegrip" => ENGINE,
            "c_pgfrollgripwhenoccupied" => ROLL_WHEN_OCCUPIED,
            "c_pgfdummygrip" => DUMMY,
            "c_pgfenginedummygrip" => ENGINE_DUMMY,
            "c_pgfstaticfriction" => STATIC_FRICTION,
            n => return n.parse().ok(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Material {
    pub friction: f32,
    pub elasticity: f32,
    pub resistance: f32,
    /// `materialDefGroup` (+0) and `materialAttGroup` (+4): the damage table's column and row
    /// (−1: none)
    pub def_group: i64,
    pub att_group: i64,
    /// `materialDamage` (+8): the damage of a projectile or explosion of this material
    pub damage: f32,
}

/// What the material's constructor sets: a material that is declared but given no values.
pub const DEFAULT_MATERIAL: Material = Material { friction: 1.0, elasticity: 0.0, resistance: 0.01, def_group: -1, att_group: -1, damage: 0.0 };

/// The `MaterialManager` table (`materialManagerdefine.con`) and its damage table
/// (`materialManagerSettings.con` and the `damage_system/*.con` it runs).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Materials {
    pub table: HashMap<u32, Material>,
    /// `attGroup a` + `defGroup d` + `damageMod x`: the cell (a, d)
    #[serde(skip)]
    pub cells: HashMap<(i64, i64), f32>,
    /// `setEffectTemplate` of a cell: the effect of that hit (the client's: a bullet's dust, an
    /// explosion's fire)
    #[serde(skip)]
    pub effects: HashMap<(i64, i64), String>,
    /// `defaultDamageMod` (0 in the constructor): a pair without a cell
    pub default_damage_mod: f32,
    /// `speedDamageMod` (0.1)
    pub speed_damage_mod: f32,
}

impl Default for Materials {
    fn default() -> Materials {
        Materials { table: HashMap::new(), cells: HashMap::new(), effects: HashMap::new(), default_damage_mod: 0.0, speed_damage_mod: 0.1 }
    }
}

impl Materials {
    /// From `MaterialManager.material <id>` and the `materialFriction/Elasticity/Resistance/
    /// AttGroup/DefGroup/Damage` lines that follow it, and the damage cells.
    pub fn from_commands<'a>(calls: impl IntoIterator<Item = (&'a str, &'a [String])>) -> Materials {
        let mut m = Materials::default();
        let mut cur: Option<u32> = None;
        let (mut att, mut def) = (-1i64, -1i64);
        for (cmd, args) in calls {
            let c = cmd.to_ascii_lowercase();
            let Some(method) = c.strip_prefix("materialmanager.") else { continue };
            let num = || args.first().and_then(|a| a.parse::<f32>().ok());
            let int = || args.first().and_then(|a| a.parse::<i64>().ok());
            match method {
                "material" => {
                    cur = args.first().and_then(|a| a.parse().ok());
                    if let Some(id) = cur {
                        m.table.entry(id).or_insert(DEFAULT_MATERIAL);
                    }
                }
                "materialfriction" | "materialelasticity" | "materialresistance" | "materialdamage" => {
                    let (Some(id), Some(v)) = (cur, num()) else { continue };
                    let e = m.table.entry(id).or_insert(DEFAULT_MATERIAL);
                    match method {
                        "materialfriction" => e.friction = v,
                        "materialelasticity" => e.elasticity = v,
                        "materialdamage" => e.damage = v,
                        _ => e.resistance = v,
                    }
                }
                "materialattgroup" | "materialdefgroup" => {
                    let (Some(id), Some(v)) = (cur, int()) else { continue };
                    let e = m.table.entry(id).or_insert(DEFAULT_MATERIAL);
                    if method == "materialattgroup" {
                        e.att_group = v;
                    } else {
                        e.def_group = v;
                    }
                }
                "attgroup" => att = int().unwrap_or(-1),
                "defgroup" => def = int().unwrap_or(-1),
                // `setCell def mod [effect]`: the cell (current attGroup, def) in one line, used by the
                // splash tables against infantry (`wespe.con`, `bombs.con`); none while attGroup is −1
                "setcell" => {
                    let def = int();
                    let v = args.get(1).and_then(|a| a.parse::<f32>().ok());
                    if let (Some(d), Some(v)) = (def, v)
                        && att != -1
                    {
                        m.cells.insert((att, d), v);
                        if let Some(e) = args.get(2) {
                            m.effects.insert((att, d), e.clone());
                        }
                    }
                }
                // the cell of the current groups (none while one is −1)
                "damagemod" => {
                    if let Some(v) = num()
                        && att != -1
                        && def != -1
                    {
                        m.cells.insert((att, def), v);
                    }
                }
                "seteffecttemplate" => {
                    if let Some(e) = args.first()
                        && att != -1
                        && def != -1
                    {
                        m.effects.insert((att, def), e.clone());
                    }
                }
                "defaultdamagemod" => m.default_damage_mod = num().unwrap_or(m.default_damage_mod),
                "speeddamagemod" => m.speed_damage_mod = num().unwrap_or(m.speed_damage_mod),
                _ => {}
            }
        }
        m
    }

    /// `getFriction/Elasticity/ResistanceForMaterial`: an unknown id falls back to material 0,
    /// and without that to 1.
    pub fn get(&self, id: u32) -> Material {
        self.table.get(&id).or_else(|| self.table.get(&0)).copied().unwrap_or(Material { friction: 1.0, elasticity: 1.0, resistance: 1.0, ..DEFAULT_MATERIAL })
    }

    /// `getDamageForMaterial`: its `materialDamage` (material 0's for an unknown id; 0 without).
    pub fn damage(&self, id: u32) -> f32 {
        self.table.get(&id).or_else(|| self.table.get(&0)).map_or(0.0, |m| m.damage)
    }

    /// `getDamageMod(attacker, defender)`: the cell (attacker's `attGroup`, defender's
    /// `defGroup`); unknown materials fall back to material 0, a missing cell to
    /// `defaultDamageMod`.
    pub fn damage_mod(&self, attacker: u32, defender: u32) -> f32 {
        let m = |id: u32| self.table.get(&id).or_else(|| self.table.get(&0));
        let (Some(a), Some(d)) = (m(attacker), m(defender)) else { return self.default_damage_mod };
        self.cells.get(&(a.att_group, d.def_group)).copied().unwrap_or(self.default_damage_mod)
    }
}

/// The terrain as the collision sees it.
pub trait Ground {
    /// height and normal at (x, z) (`getHeightAndNormal`; 0 and up outside the map)
    fn height_and_normal(&self, x: f32, z: f32) -> (f32, V3);
    /// the material id at (x, z) (`getMaterial`)
    fn material(&self, x: f32, z: f32) -> u32;
    fn water_level(&self) -> f32;
}

/// `setAdjust`: combine one axis of a correction: same sign, keep the larger; opposite signs,
/// add them; nothing yet, take it.
pub fn set_adjust(cur: f32, v: f32) -> f32 {
    if cur == 0.0 {
        v
    } else if cur > 0.0 {
        if v <= 0.0 { cur + v } else if v > cur { v } else { cur }
    } else if v >= 0.0 {
        cur + v
    } else if v < cur {
        v
    } else {
        cur
    }
}

fn project(v: V3, axis: V3) -> V3 {
    let l = dot_zyx(axis, axis);
    if l == 0.0 || l.is_nan() {
        return [0.0; 3];
    }
    let k = dot_zyx(v, axis) / l;
    [axis[0] * k, axis[1] * k, axis[2] * k]
}

fn nonzero(v: V3) -> bool {
    v.iter().any(|&x| x != 0.0)
}

/// One object's `ResponsePhysics`: what it accumulates during a frame's collision pass.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Response {
    /// position correction
    pub pos_adjust: V3,
    /// the root response's position correction when this one's impulse came
    pub root_adjust: V3,
    /// velocity correction
    pub vel_adjust: V3,
    /// speed of the surface it stands on
    pub surface_speed: V3,
    /// means over the frame's impulses: normal, velocity, point relative to the
    /// object
    pub normal: V3,
    pub vel: V3,
    pub point: V3,
    pub count: i32,
    /// means of the two materials' friction, elasticity, resistance
    pub friction: f32,
    pub elasticity: f32,
    pub resistance: f32,
    /// the grip of the last impulse, plus `grip::STATIC_FRICTION` while it holds
    pub flags: u8,
    /// a vertex was under the terrain this frame and their mean
    pub touching: bool,
    pub touch_point: V3,
    /// soldiers: the most upright normal of the frame's hits, written by
    /// the soldier's collision handling; their friction uses it instead of the mean
    pub ground_normal: V3,
    /// soldiers: the velocity their input asks for (the soldier's player input handling →
    /// `setSurfacePositionalSpeed`, every frame): the friction pulls the soldier towards it
    pub walk: V3,
    /// soldiers: the kept normal itself, which outlives the frame
    pub kept_normal: V3,
    /// soldiers: the terrain gave the kept normal
    pub kept_terrain: bool,
    /// soldiers: standing on something (a kept normal with y > 0.1), until
    /// the next `handlePlayerInput`
    pub on_ground: bool,
    /// soldiers: `handleCollision` was called (since the soldier's last update), and with an
    /// `Obstacle` (`slowDownMod` the next frame)
    pub hit: bool,
    pub slowed: bool,
    /// soldiers: the first ladder touched since the last `handlePlayerInput` (`ladder.rs`)
    pub ladder: Option<crate::ladder::Ladder>,
}

/// One impulse (as impulse on receives it), kept for inspection.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Impulse {
    pub r: V3,
    pub v: V3,
    pub n: V3,
    pub depth: f32,
    pub materials: (u32, u32),
    /// the obstacle hit (None: the terrain)
    pub with: Option<String>,
}

impl Response {
    /// `impulseOn(r, v, n, depth, mat1, mat2)`: r is the contact point relative to the object,
    /// v the velocity there, n the surface normal, depth ≤ 0 how far under it the point was.
    #[allow(clippy::too_many_arguments)]
    pub fn impulse_on(&mut self, r: V3, v: V3, n: V3, depth: f32, mats: (Material, Material), grip: u8, root_adjust: Option<V3>) {
        let vn = project([-v[0], -v[1], -v[2]], n);
        // On a very steep face a soldier that falls in is pushed along the normal by its depth and can jump tens of metres in a
        // frame. The original does too (GC_Podrace 1520/984, 41 m up against our 37).
        let d = -depth;
        let pa = [n[0] * d, n[1] * d, n[2] * d];
        for i in 0..3 {
            self.pos_adjust[i] = set_adjust(self.pos_adjust[i], pa[i]);
        }
        for i in 0..3 {
            self.vel_adjust[i] = set_adjust(self.vel_adjust[i], vn[i]);
        }
        let c = self.count as f32;
        let c1 = (self.count + 1) as f32;
        for i in 0..3 {
            self.normal[i] = (self.normal[i] * c + n[i]) / c1;
        }
        for i in 0..3 {
            self.point[i] = (self.point[i] * c + r[i]) / c1;
        }
        for i in 0..3 {
            self.vel[i] = (self.vel[i] * c + v[i]) / c1;
        }
        self.count += 1;
        self.flags = (self.flags & grip::STATIC_FRICTION) | grip;
        let (a, b) = mats;
        self.friction = (a.friction + b.friction) * 0.5;
        self.elasticity = (a.elasticity + b.elasticity) * 0.5;
        self.resistance = (a.resistance + b.resistance) * 0.5;
        if let Some(ra) = root_adjust {
            self.root_adjust = ra;
        }
    }
}

/// A wheel on a spring (`PhysicsSpring`): the collision pushes the wheel up along the ground's
/// normal, and the spring pushes the root back.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Spring {
    pub strength: f32,
    pub damping: f32,
    /// how far the collision moved the wheel this frame (world)
    pub moved: V3,
    /// the displacement of the previous frame
    pub d_prev: V3,
    /// the last push on the root: where (relative) and how much
    pub last_push: (V3, V3),
    /// its position in its parent: the original places the node from the parent's world position
    pub local: V3,
    /// its parent's links from the root down: each one's place in its parent and that parent's axes in the root
    /// (the F-15C's rear wheels: root → `F15CEngine_L` → `F15CRearGear_R`). The original places each link in the world in
    /// turn, rounding to f32 at each (checked to the bit: 478 of 478 `p50` of the F-15C's takeoff reference run, the sum in
    /// double); placed in one go from the root, one in 25 fell an ulp away, and the push's point with it
    pub chain: Vec<(V3, [V3; 3])>,
    /// each link's parent as a moving part would turn it (its part, the placement in that part's frame) and its axes as
    /// built: [`crate::mover::Vehicle::place_contacts`] gives a link the part's axes of now when they are not those it was built with
    /// (the Lambda's front wheel hangs from a `RotationalBundle` under a gear at -7,5 degrees; with the axes at rest its
    /// push fell 3,4 m to one side and the climb drifted 11,6 m from the original)
    pub chain_anchor: Vec<(Option<(usize, [V3; 3])>, [V3; 3])>,
    /// its own turn in its parent (`setRotation` under `addTemplate`), when it has one. The spring's node stays in its
    /// parent's axes (its contactor's rows leave this turn out, also as its anchor moves): GC's AT-AT hangs its back
    /// springs at 180/0/0, and offset by their own axes the rest point fell 2·|local| = 34,8 m away and the empty walker
    /// flew off at 0,8 s; DC's Urals hang the left front wheel at -179.999/0/0, and mirrored it touched the ground 1 mm off
    pub turn: Option<[V3; 3]>,
}

/// A driven wheel's friction and forward axis, and its engine's part if it is not the first.
pub type Feedback = (V3, V3, Option<usize>);

/// What the wheels need from the engine that drives them (`PhysicsEngine`).
#[derive(Debug, Clone, Copy)]
pub struct Drive {
    /// `getCurrentRatio`
    pub ratio: f32,
    /// rpm, steering differential, gear change timer
    pub rpm: f32,
    pub differential: f32,
    pub gear_timer: f32,
    /// the wheels are not driven
    pub disengaged: bool,
    /// engine type with bit 4 (tanks): left and right wheels turn at different speeds
    pub tracked: bool,
}

impl Drive {
    /// `getCurrentDifferentialRPM(side)`: side > 0 (right) slower when steering, < 0 faster.
    pub fn rpm_at(&self, side: f32) -> f32 {
        if !self.tracked || side == 0.0 {
            return self.rpm;
        }
        let k = if side > 0.0 { 1.0 - 1.5 * self.differential } else { 1.5 * self.differential + 1.0 };
        (k * self.rpm).clamp(-1.0, 1.0)
    }
}

/// A part of an object that touches the terrain: its collision mesh, where it sits on the root.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Contactor {
    pub name: String,
    /// its place in the template's walk (0 for the ones not made from a template): the order a vehicle with a driver goes in
    pub walked: usize,
    /// LOD 0 of the collision mesh: vertices and their materials
    pub vertices: Vec<V3>,
    pub materials: Vec<u32>,
    /// the box of the geometry (min, max), for the quick test
    pub bbox: Option<[V3; 2]>,
    /// placement on the root: position and axes in the root's frame
    pub pos: V3,
    pub rows: [V3; 3],
    pub grip: u8,
    /// the root object itself (its response moves the root directly)
    pub is_root: bool,
    pub soldier: bool,
    /// a `ProjectileTemplate`'s object (`CID_ProjectileTemplate`): a soldier does not collide with it
    pub projectile: bool,
    pub spring: Option<Spring>,
    pub response: Response,
    #[serde(skip)]
    pub last_impulses: Vec<Impulse>,
    /// this frame's `handleCollision` calls (|v|² over the threshold): what the damage reads
    #[serde(skip)]
    pub hits: Vec<Hit>,
    /// its collision LODs with faces (BSP), for the movers that run into it (`getFaceCollision`)
    #[serde(skip)]
    pub faces: Vec<Arc<FaceMesh>>,
    /// `GeometryTemplate.scale` of those faces
    pub scale: V3,
    /// this part's `getBoundingRadius` (the pair test between two movers' parts)
    pub radius: f32,
    /// the nearest moving ancestor (a craft part: a front wheel's steering bundle) and the
    /// placement in the frame its children hang from; `pos` and `rows` follow it
    /// ([`crate::mover::Vehicle::place_contacts`])
    pub anchor: Option<Anchor>,
    /// a vehicle of several engines: the one above this wheel (its part) and how it runs now (`addFriction` looks for it
    /// going up the parents); None: the first one, as the caller says
    #[serde(skip)]
    pub drive: Option<(usize, Drive)>,
    /// the z of its parent in its own axes: where an engine moves the ground under it (`addFriction` takes that axis from
    /// the parent, so the left wheel that DC's Ural tanker hangs at 179.999° from its steering pulls forward too)
    pub forward: V3,
    /// its x in its parent and the parent's x axis in the root: an engine's wheel takes its side from the x of its node in
    /// the parent (`addFriction`), where the collision moved a spring (DC's M1A1 hangs its wheels at
    /// ±0.01 from the track bundles)
    pub side: (f32, V3),
    /// an `Engine` among its parents: `addFriction` looks for the engine of a part with `grip::ENGINE` going up from it, and
    /// with none the part asks for no friction at all (GC's AT-AT hangs its feet, `c_PGFEngineGrip`, from the leg bundles
    /// next to `atatdedummyengine`, not under it: they never rub, and only the engine's four dummy springs drive it)
    pub engine_above: bool,
    /// the links from the root down to this part, each one's place in its parent and that parent's axes in the root (empty
    /// for a spring, which keeps its parent's: [`Spring::chain`]). The original places each link in the world in turn, rounding to f32 at
    /// each, and the part's vertices and the point its solve pushes at start from there: placed in one go from the root,
    /// the right foot of GC's AT-ST fell one and two ulps of the world away (0.06 and 0.12 mm) and its first touch turned
    /// the walker 2.6e-4 off
    #[serde(skip)]
    pub chain: Vec<(V3, [V3; 3])>,
    /// each link's moving part and its axes as built ([`Spring::chain_anchor`])
    #[serde(skip)]
    pub chain_anchor: Vec<(Option<(usize, [V3; 3])>, [V3; 3])>,
}

/// Where a part hangs from a moving one ([`Contactor::anchor`]).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Anchor {
    /// the craft part ([`crate::craft::Craft::parts`])
    pub part: usize,
    /// the placement in the frame its children hang from
    pub pos: V3,
    pub rows: [V3; 3],
    /// the placement in the root as built (the moving parts unturned)
    pub rest: (V3, [V3; 3]),
}

fn place(pos: V3, rows: [V3; 3], body: &RigidBody) -> ([V3; 3], V3) {
    let r = &body.rows;
        let to_world = |v: V3| [r[2][0] * v[2] + r[0][0] * v[0] + r[1][0] * v[1], r[0][1] * v[0] + r[1][1] * v[1] + r[2][1] * v[2], r[0][2] * v[0] + v[1] * r[1][2] + v[2] * r[2][2]];
        let rows = [to_world(rows[0]), to_world(rows[1]), to_world(rows[2])];
        let p = to_world(pos);
        (rows, [body.pos[0] + p[0], body.pos[1] + p[1], body.pos[2] + p[2]])
    }

/// A part's position relative to the root, in the world's axes (`place`'s, before the root's position is added).
fn place_rel(pos: V3, body: &RigidBody) -> V3 {
    let r = &body.rows;
    [r[2][0] * pos[2] + r[0][0] * pos[0] + r[1][0] * pos[1], r[0][1] * pos[0] + r[1][1] * pos[1] + r[2][1] * pos[2], r[0][2] * pos[0] + pos[1] * r[1][2] + pos[2] * r[2][2]]
}

/// Where a chain of links ends in the world: link by link as the original's chain of matrices, the sum in double and
/// each link's place rounded to f32.
// TODO: this is walked for every contactor that has a contact, every step: about 3 % of a conquest round with 32 bots
// (`examples/speed`). A body's chains could be placed once a step and kept.
fn chain_pos(chain: &[(V3, [V3; 3])], body: &RigidBody) -> V3 {
    let mut parent = body.pos;
    for (l, axes) in chain {
        let w = [place_rel(axes[0], body), place_rel(axes[1], body), place_rel(axes[2], body)];
        parent = std::array::from_fn(|j| (parent[j] as f64 + w[0][j] as f64 * l[0] as f64 + w[1][j] as f64 * l[1] as f64 + w[2][j] as f64 * l[2] as f64) as f32);
    }
    parent
}

impl Contactor {
    /// Its axes and position in the world, given the root body's.
    pub fn frame(&self, body: &RigidBody) -> ([V3; 3], V3) {
        let (rows, p) = place(self.pos, self.rows, body);
        if self.chain.is_empty() {
            return (rows, p);
        }
        (rows, chain_pos(&self.chain, body))
    }


    /// `checkObjectVsObject` against static obstacles: a segment from the root's centre (set back
    /// by one step of its velocity) to each vertex, against the obstacle's faces (collision LOD
    /// `lod`); a face crossed gives an impulse with the end's depth behind it.
    #[allow(clippy::too_many_arguments)]
    pub fn check_obstacle(&mut self, body: &RigidBody, o: &Obstacle, lod: usize, materials: &Materials, root_adjust: Option<V3>, dt: f32) {
        let mut n = self.vertices.len();
        if n == 0 {
            return;
        }
        if n < 4 {
            n = 1;
        }
        let (rows, pos) = self.frame(body);
        let rv = [body.vel[0] * dt, body.vel[1] * dt, body.vel[2] * dt];
        let off = [pos[0] - body.pos[0] + rv[0], pos[1] - body.pos[1] + rv[1], pos[2] - body.pos[2] + rv[2]];
        let start = [pos[0] - off[0], pos[1] - off[1], pos[2] - off[2]];
        for i in 0..n.min(self.vertices.len()) {
            let v = self.vertices[i];
            let w = [
                rows[2][0] * v[2] + rows[0][0] * v[0] + rows[1][0] * v[1],
                rows[0][1] * v[0] + rows[1][1] * v[1] + rows[2][1] * v[2],
                v[2] * rows[2][2] + rows[0][2] * v[0] + v[1] * rows[1][2],
            ];
            let dir = [w[0] + off[0], w[1] + off[1], w[2] + off[2]];
            let Some(mut hit) = o.segment(lod, start, dir, false) else { continue };
            let vel = body.velocity_at(hit.point);
            // |v|² > 0.1 asks both objects' `handleCollision`; a vehicle's says no to an
            // `Obstacle`, and then there is no impulse
            if !self.soldier && dot_zyx(vel, vel) > crate::mover::HIT_SPEED2 {
                let mats = (self.materials.get(i).copied().unwrap_or(0), hit.material);
                self.hits.push(Hit { other: crate::damage::Other::Obstacle(o as *const Obstacle as usize), v: vel, n: hit.normal, materials: mats, height: pos[1], barrier: None });
                if o.obstacle_class {
                    continue;
                }
            }
            if self.soldier {
                // its `handleCollision` gets the face's normal, the impulse its own
                let face = hit.normal;
                if i != 0 {
                    crate::soldier::object_normal(&mut hit, start, pos);
                }
                if dot_zyx(vel, vel) > crate::soldier::HIT_SPEED2 {
                    // against an `Obstacle` it slows the soldier down and says no: no impulse;
                    // the obstacle's own damage instead of the crash's
                    if o.obstacle_class {
                        self.response.hit = true;
                        self.response.slowed = true;
                        self.hits.push(Hit { other: crate::damage::Other::Obstacle(o as *const Obstacle as usize), v: vel, n: face, materials: (0, hit.material), height: pos[1], barrier: Some(o.damage) });
                        continue;
                    }
                    let mats = (self.materials.get(i).copied().unwrap_or(0), hit.material);
                    self.hits.push(Hit { other: crate::damage::Other::Obstacle(o as *const Obstacle as usize), v: vel, n: face, materials: mats, height: pos[1], barrier: None });
                    if self.response.ladder.is_none() && crate::ladder::MATERIALS.contains(&hit.material) {
                        self.response.ladder = Some(crate::ladder::Ladder { pos: o.pos, rows: o.rows, bounds: o.bounds, material: hit.material });
                    }
                    crate::soldier::handle_collision(&mut self.response, face, false);
                }
            }
            let r = [hit.point[0] - pos[0], hit.point[1] - pos[1], hit.point[2] - pos[2]];
            let m = (self.materials.get(i).copied().unwrap_or(0), hit.material);
            let mats = (materials.get(m.0), materials.get(m.1));
            let ra = if self.is_root { None } else { root_adjust };
            self.response.impulse_on(r, vel, hit.normal, hit.depth, mats, self.grip, ra);
            self.last_impulses.push(Impulse { r, v: vel, n: hit.normal, depth: hit.depth, materials: m, with: Some(o.name.clone()) });
        }
    }

    /// `checkVsTerrain`. Returns how far under water its lowest vertex is (0 above).
    pub fn check_terrain(&mut self, body: &RigidBody, ground: &dyn Ground, materials: &Materials, root_adjust: Option<V3>) -> f32 {
        let mut n = self.vertices.len();
        if n == 0 {
            return 0.0;
        }
        if n < 4 {
            n = 1;
        }
        let (rows, pos) = self.frame(body);
        let mut check = true;
        if n > 10
            && let Some([lo, hi]) = self.bbox
        {
            let bx = lo[0].abs().max(hi[0].abs());
            let by = lo[1].abs().max(hi[1].abs());
            let margin = (by.max(bx) * QUICK_FACTOR).min(QUICK_MAX);
            let end = |z: f32| [rows[2][0] * z + pos[0], rows[2][1] * z + pos[1], rows[2][2] * z + pos[2]];
            let (a, b) = (end(hi[2]), end(lo[2]));
            let above = |p: V3| (p[1] - margin) - ground.height_and_normal(p[0], p[2]).0 > 0.0;
            if above(a) && above(b) {
                check = false;
            }
        }
        if self.soldier {
            n = 5;
        }
        let mut lowest = 9999.0f32;
        let mut touch = 0;
        let mut touch_sum = [0.0f32; 3];
        for i in 0..n.min(self.vertices.len()) {
            let v = self.vertices[i];
            let w = [
                rows[2][0] * v[2] + rows[0][0] * v[0] + rows[1][0] * v[1] + pos[0],
                rows[0][1] * v[0] + rows[1][1] * v[1] + rows[2][1] * v[2] + pos[1],
                rows[0][2] * v[0] + v[1] * rows[1][2] + v[2] * rows[2][2] + pos[2],
            ];
            if check {
                let (h, nrm) = ground.height_and_normal(w[0], w[2]);
                let depth = w[1] - h;
                // TODO: camel2 born flush with the ground (depth exactly 0 at four vertices) has no contact in the original's
                // first record and four here; not the x87's precision (tried). How: compare the first record with the original.
                if depth <= 0.0 {
                    let point = [w[0], h, w[2]];
                    let r = [w[0] - pos[0], h - pos[1], w[2] - pos[2]];
                    let vel = body.velocity_at(point);
                    if self.soldier && dot_zyx(vel, vel) >= crate::soldier::HIT_SPEED2 {
                        crate::soldier::handle_collision(&mut self.response, nrm, true);
                    }
                    let m = (self.materials.get(i).copied().unwrap_or(0), ground.material(w[0], w[2]));
                    if dot_zyx(vel, vel) >= TERRAIN_HIT_SPEED2 {
                        self.hits.push(Hit { other: crate::damage::Other::Terrain, v: vel, n: nrm, materials: m, height: pos[1], barrier: None });
                    }
                    let mats = (materials.get(m.0), materials.get(m.1));
                    let ra = if self.is_root { None } else { root_adjust };
                    self.response.impulse_on(r, vel, nrm, depth, mats, self.grip, ra);
                    self.last_impulses.push(Impulse { r, v: vel, n: nrm, depth, materials: m, with: None });
                    let c = touch as f32;
                    touch += 1;
                    for k in 0..3 {
                        touch_sum[k] = (touch_sum[k] * c + point[k]) / touch as f32;
                    }
                }
            }
            lowest = lowest.min(w[1]);
        }
        self.response.touching = touch > 0;
        self.response.touch_point = touch_sum;
        let d = lowest - ground.water_level();
        // under water the object's `handleCollision` is called with the water (up, material 1)
        // and the root's velocity, whatever it is
        if d <= 0.0 {
            if self.soldier {
                crate::soldier::water_collision(&mut self.response);
            }
            let m = (self.materials.first().copied().unwrap_or(0), crate::projectile::WATER_MATERIAL);
            self.hits.push(Hit { other: crate::damage::Other::Terrain, v: body.vel, n: [0.0, 1.0, 0.0], materials: m, height: pos[1], barrier: None });
        }
        if d > 0.0 { 0.0 } else { -d }
    }

    /// `solveImpulse` for a part that is not a spring: the position correction moves the root
    /// now; the velocity correction, × fps × (1 + elasticity) / 2, is an acceleration at the
    /// mean contact point.
    pub fn solve(&mut self, body: &mut RigidBody) {
        let r = &mut self.response;
        if let Some(sp) = self.spring.as_mut() {
            // the wheel moves along the mean normal by its correction less the root's, 0..1 m
            let n = r.normal;
            let a = r.pos_adjust;
            let b = r.root_adjust;
            let f = (a[2] * n[2] + a[0] * n[0] + a[1] * n[1]) - (b[2] * n[2] + b[1] * n[1] + n[0] * b[0]);
            let f = if f >= 0.0 { f.min(1.0) } else { 0.0 };
            sp.moved = [n[0] * f, n[1] * f, n[2] * f];
        } else if nonzero(r.pos_adjust) {
            for i in 0..3 {
                body.pos[i] += r.pos_adjust[i];
            }
            let e = r.elasticity + 1.0;
            let a = [r.vel_adjust[0] * FPS * e * 0.5, r.vel_adjust[1] * FPS * e * 0.5, r.vel_adjust[2] * FPS * e * 0.5];
            let p = if self.chain.is_empty() { place(self.pos, self.rows, body).1 } else { chain_pos(&self.chain, body) };
            // a soldier's mean point is zeroed first: the push goes through its centre
            let at = if self.soldier { p } else { [r.point[0] + p[0], r.point[1] + p[1], r.point[2] + p[2]] };
            body.add_acc_at(at, a);
        }
        r.pos_adjust = [0.0; 3];
        r.root_adjust = [0.0; 3];
        r.vel_adjust = [0.0; 3];
    }

    /// Where its physics node is: the placement, plus what the collision moved a wheel.
    fn node_pos(&self, p: V3) -> V3 {
        match &self.spring {
            Some(s) => [p[0] + s.moved[0], p[1] + s.moved[1], p[2] + s.moved[2]],
            None => p,
        }
    }

    /// `addFriction`. With `grip::ENGINE` and a `drive`, the wheel's ground speed follows the
    /// engine; returns the friction (per frame) and the forward axis for its `feedbackLoop`.
    pub fn friction(&mut self, body: &mut RigidBody, gravity: f32, drive: Option<&Drive>) -> Option<(V3, V3)> {
        let own = self.drive;
        let drive = if self.engine_above { own.as_ref().map(|d| &d.1).or(drive) } else { None };
        let g = self.grip;
        if g & 0x20 != 0 && g & 4 != 0 {
            return None; // engine dummy wheels only spin (SpinWheel); their means are never emptied
        }
        // (nothing placed for a contactor with no contact: the chain of links costs every step)
        if self.response.count == 0 || g == 0 {
            self.response.flags = 0;
            return None;
        }
        let (rows, placed) = self.frame(body);
        let node = self.node_pos(placed);
        let r = &mut self.response;
        // soldiers: the ground normal of `handleCollision`, and limits × n.y⁵
        let n = if self.soldier { r.ground_normal } else { r.normal };
        let (static_limit, sliding_limit) = if self.soldier {
            crate::soldier::friction_limits(r.friction, n[1])
        } else {
            (((r.friction * STATIC_FRICTION * FRICTION_G) / FPS) * n[1], n[1] * ((r.friction * SLIDING_FRICTION * FRICTION_G) / FPS))
        };
        let mut vrel = [r.vel[0] - r.surface_speed[0], r.vel[1] - r.surface_speed[1], r.vel[2] - r.surface_speed[2]];
        vrel[1] += gravity / FPS;
        let p = project(vrel, n);
        let t = [vrel[0] - p[0], vrel[1] - p[1], vrel[2] - p[2]];
        if r.resistance > 0.0 {
            let k = r.resistance;
            body.add_acc_at_rel([0.0; 3], [-t[0] * k, -t[1] * k, -t[2] * k]);
        }
        let z = self.forward;
        let forward = [0, 1, 2].map(|i| rows[0][i] * z[0] + rows[1][i] * z[1] + rows[2][i] * z[2]);
        let mut feedback_axis = None;
        let want = if r.flags & grip::ENGINE != 0 {
            // the ground under the wheel should move at the engine's speed along the wheel
            let mut drive_v = [0.0f32; 3];
            if let Some(d) = drive {
                feedback_axis = Some(forward);
                if !d.disengaged {
                    // the side is the x of the wheel's node in its parent, where the collision just moved it (a spring's
                    // `moved`, along the ground's normal): on a slope a right wheel of DC's M1A1 (x 0.01 in its bundle) pushed
                    // 0.19 m up a tilted normal turns at the left track's speed in the original
                    let side = self.side.0
                        + self.spring.as_ref().map_or(0.0, |s| {
                            let ax = [0, 1, 2].map(|j| self.side.1[0] * body.rows[0][j] + self.side.1[1] * body.rows[1][j] + self.side.1[2] * body.rows[2][j]);
                            dot_zyx(s.moved, ax)
                        });
                    let w = d.ratio * d.rpm_at(side);
                    let target = [forward[0] * w, forward[1] * w, forward[2] * w];
                    let k = 1.0 - 0.5 * d.gear_timer;
                    let tf = project(t, forward);
                    let k2 = 0.5 * d.gear_timer;
                    let sum = [target[0] * k + tf[0] * k2, target[1] * k + tf[1] * k2, target[2] * k + tf[2] * k2];
                    let sn = project(sum, n);
                    drive_v = [sum[0] - sn[0], sum[1] - sn[1], sum[2] - sn[2]];
                }
                [drive_v[0] - t[0], drive_v[1] - t[1], drive_v[2] - t[2]]
            } else {
                [0.0; 3]
            }
        } else if r.flags & grip::ROLL != 0 {
            // only sideways: the part of t along the axle (laid on the ground)
            let axle = rows[0];
            let an = project(axle, n);
            let flat = [axle[0] - an[0], axle[1] - an[1], axle[2] - an[2]];
            let side = project(t, flat);
            [0.0 - side[0], 0.0 - side[1], 0.0 - side[2]]
        } else {
            [0.0 - t[0], 0.0 - t[1], 0.0 - t[2]]
        };
        let l2 = dot_zyx(want, want);
        let clamp = |v: V3, lim: f32| -> V3 {
            let l2 = dot_zyx(v, v);
            if l2 > lim * lim && l2 > 1e-6 {
                let k = lim.abs() / l2.sqrt();
                [v[0] * k, v[1] * k, v[2] * k]
            } else {
                v
            }
        };
        let f = if r.flags & grip::STATIC_FRICTION != 0 {
            if l2 > static_limit * static_limit && l2 > 1e-6 {
                r.flags &= !grip::STATIC_FRICTION;
                clamp(clamp(want, static_limit), sliding_limit)
            } else {
                want
            }
        } else {
            let out = clamp(want, sliding_limit);
            if !(l2 > sliding_limit * sliding_limit && l2 > 1e-6) {
                r.flags |= grip::STATIC_FRICTION;
            }
            out
        };
        let at = if self.soldier { node } else { [r.point[0] + node[0], r.point[1] + node[1], r.point[2] + node[2]] };
        body.add_friction_at(at, [f[0] * FPS, f[1] * FPS, f[2] * FPS]);
        r.surface_speed = [0.0; 3];
        r.count = 0;
        r.normal = [0.0; 3];
        r.ground_normal = [0.0; 3];
        r.vel = [0.0; 3];
        r.point = [0.0; 3];
        feedback_axis.map(|a| (f, a))
    }

    /// The spring's physics update (the next frame, before the root integrates): the wheel
    /// went back to rest, so its displacement is the rest point less where the collision left it; the root
    /// gets −(1.5·strength·d + damping·(d − d_prev)/dt) at the rest point.
    pub fn spring_push(&mut self, body: &mut RigidBody, gravity: f32, dt: f32) {
        let Some(sp) = self.spring.as_mut() else { return };
        // the chain of `updatePhysics`, each step in f32 (checked to the bit against the reference run's `p50`/`m50`,
        // the reference harness's spring hook): the parent's world position, the offset by the parent's world axes,
        // rest = root + ((parent − root) + off), the node where the collision left it f32(parent + off) + moved. The two
        // roundings differ by an ulp of the world, not d = −moved.
        // the node is offset by its parent's axes, which are the spring's own rows: the contactor leaves its turn out
        // ([`Spring::turn`]; GC's AT-AT hangs its back springs at 180/0/0)
        let l = sp.local;
        let prow: [V3; 3] = self.rows;
        let pr: V3 = std::array::from_fn(|j| prow[0][j] * l[0] + prow[1][j] * l[1] + prow[2][j] * l[2]);
        let pw = place_rel([self.pos[0] - pr[0], self.pos[1] - pr[1], self.pos[2] - pr[2]], body);
        let mut parent: V3 = std::array::from_fn(|i| body.pos[i] + pw[i]);
        if !sp.chain.is_empty() {
            // link by link, as the original's chain of matrices: the sum in double, each link's place rounded to f32
            // (the links' axes follow the moving parts: [`Spring::chain_anchor`])
            parent = chain_pos(&sp.chain, body);
        }
        let rw = [place_rel(prow[0], body), place_rel(prow[1], body), place_rel(prow[2], body)];
        // the offset in double, as the chain's links: in f32 the node fell an ulp off (the F-15C's record 3, d one ulp
        // of y long)
        let off: [f64; 3] = std::array::from_fn(|j| rw[0][j] as f64 * l[0] as f64 + rw[1][j] as f64 * l[1] as f64 + rw[2][j] as f64 * l[2] as f64);
        // (the push's point, relative to the root, is this sum: `local_12c`, not rest − root)
        let r: V3 = std::array::from_fn(|i| ((parent[i] - body.pos[i]) as f64 + off[i]) as f32);
        let rest: V3 = std::array::from_fn(|i| body.pos[i] + r[i]);
        let node: V3 = std::array::from_fn(|i| (parent[i] as f64 + off[i]) as f32 + sp.moved[i]);
        let d = [rest[0] - node[0], rest[1] - node[1], rest[2] - node[2]];

        let k = SPRING_K as f64 * gravity as f64;
        let inv = 1.0 / dt as f64;
        let mut a = [0.0f32; 3];
        for i in 0..3 {
            let spring = d[i] as f64 * k * sp.strength as f64;
            let damp = (d[i] as f64 - sp.d_prev[i] as f64) * inv * sp.damping as f64;
            a[i] = -(spring + damp) as f32;
        }
        sp.d_prev = d;
        sp.moved = [0.0; 3];
        sp.last_push = (r, a);
        body.add_acc_at_rel(r, a);
    }
}

/// The whole collision pass of one object against the terrain: every part checks, then every
/// part solves and adds its friction, in order. Sets the body's depth under water.
pub fn terrain_pass(parts: &mut [Contactor], body: &mut RigidBody, ground: &dyn Ground, materials: &Materials, gravity: f32) {
    terrain_pass_driven(parts, body, ground, materials, gravity, 1.0 / FPS, None);
}

/// The same, with the engine that drives the `grip::ENGINE` wheels; returns each driven wheel's
/// friction and forward axis, for the engine's `feedbackLoop`. Then the springs push.
pub fn terrain_pass_driven(parts: &mut [Contactor], body: &mut RigidBody, ground: &dyn Ground, materials: &Materials, gravity: f32, dt: f32, drive: Option<&Drive>) -> Vec<Feedback> {
    collision_pass(parts, body, ground, Obstacles::default(), reach(parts), materials, gravity, dt, drive)
}

/// How far the object's parts reach from its root (for the broad phase and the choice of the
/// obstacles' collision LOD).
pub fn reach(parts: &[Contactor]) -> f32 {
    let mut r = 0.0f32;
    for c in parts {
        for v in &c.vertices {
            let p = [c.pos[0] + v[0], c.pos[1] + v[1], c.pos[2] + v[2]];
            r = r.max((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt());
        }
    }
    r
}

/// Objects smaller than this are tested against the obstacles' finer collision LOD (1).
pub const SMALL_OBJECT: f32 = 4.0;

/// The collision LOD of the faces an object of bounding radius `radius` is tested against.
pub fn faces_lod(radius: f32) -> usize {
    if radius < SMALL_OBJECT { 1 } else { 0 }
}

/// The level's obstacles near a moving object (a superset of `getCollisionObjects`).
pub fn near_obstacles<'o>(body: &RigidBody, obstacles: Obstacles<'o>, radius: f32, dt: f32) -> Vec<&'o Obstacle> {
    let step = [body.vel[0] * dt, body.vel[1] * dt, body.vel[2] * dt];
    let probe = [body.pos[0] - step[0] * 0.5, body.pos[1] - step[1] * 0.5, body.pos[2] - step[2] * 0.5];
    let probe_r = radius + 1.0 + 0.25 * (step[0] * step[0] + step[1] * step[1] + step[2] * step[2]) + 2.0;
    obstacles
        .iter()
        .filter(|o| {
            let d = [o.center[0] - probe[0], o.center[1] - probe[1], o.center[2] - probe[2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < probe_r + o.radius
        })
        .collect()
}

/// The root part's position correction so far: what an impulse on another part copies
/// (`impulseOn`).
pub fn root_adjust(parts: &[Contactor]) -> Option<V3> {
    parts.iter().find(|c| c.is_root).map(|c| c.response.pos_adjust)
}

/// Start of an object's collision pass: forget the last frame's impulses and hits.
pub fn begin_pass(parts: &mut [Contactor]) {
    for c in parts.iter_mut() {
        c.last_impulses.clear();
        c.hits.clear();
    }
}

/// A `handleCollision` call on a part: who with, the contact point's velocity
/// (relative to the other for a mover), the normal, the materials (its own vertex's, the
/// other's), and the part's height.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Hit {
    pub other: crate::damage::Other,
    pub v: V3,
    pub n: V3,
    pub materials: (u32, u32),
    pub height: f32,
    /// a soldier against an `Obstacle`: its template's `damage`
    pub barrier: Option<f32>,
}

/// `checkVsTerrain` calls `handleCollision` from |v|² = 0.1 on (vehicles; soldiers have their own).
pub const TERRAIN_HIT_SPEED2: f32 = 0.1;

/// Part `k` against the level's obstacles in `near` (`checkObjectVsObjects`, the static ones).
pub fn check_part_obstacles(parts: &mut [Contactor], k: usize, body: &RigidBody, near: &[&Obstacle], radius: f32, materials: &Materials, dt: f32) {
    let lod = faces_lod(radius);
    for o in near {
        let ra = root_adjust(parts);
        let c = &mut parts[k];
        let ra = if c.is_root { None } else { ra };
        c.check_obstacle(body, o, lod, materials, ra, dt);
    }
}

/// Part `k` against the terrain (`checkVsTerrain`); the root's sets the body's depth under water.
pub fn check_part_terrain(parts: &mut [Contactor], k: usize, body: &mut RigidBody, ground: &dyn Ground, materials: &Materials) {
    let ra = root_adjust(parts);
    let c = &mut parts[k];
    let d = c.check_terrain(body, ground, materials, if c.is_root { None } else { ra });
    if c.is_root {
        body.underwater = d;
    }
}

/// The second half of the pass (`solveImpulse` and `addFriction` of every part, in order),
/// then the springs push. It runs for a sleeping object too (the impulses another one gave it);
/// the springs of a sleeping one do not push.
pub fn solve_pass(parts: &mut [Contactor], body: &mut RigidBody, gravity: f32, dt: f32, drive: Option<&Drive>) -> Vec<Feedback> {
    let mut feedback = Vec::new();
    for c in parts.iter_mut() {
        feedback.extend(solve_part(c, body, gravity, drive));
    }
    spring_pass(parts, body, gravity, dt);
    feedback
}

/// One part's solve and friction (what its feedback to the engine is, if any).
pub fn solve_part(c: &mut Contactor, body: &mut RigidBody, gravity: f32, drive: Option<&Drive>) -> Option<Feedback> {
    c.solve(body);
    let engine = c.drive.map(|d| d.0);
    c.friction(body, gravity, drive).map(|(f, axis)| (f, axis, engine))
}

/// The springs' push, after every part solved (not for a sleeping body).
pub fn spring_pass(parts: &mut [Contactor], body: &mut RigidBody, gravity: f32, dt: f32) {
    if !body.is_sleeping() {
        for c in parts.iter_mut() {
            c.spring_push(body, gravity, dt);
        }
    }
}

/// The full collision pass of one object alone: each part against the obstacles near it and
/// against the terrain; then every part solves and adds its friction; then the springs push.
/// `radius`: the object's bounding radius (`World::bounding_radius`). A sleeping object is not
/// checked.
#[allow(clippy::too_many_arguments)]
pub fn collision_pass(parts: &mut [Contactor], body: &mut RigidBody, ground: &dyn Ground, obstacles: Obstacles<'_>, radius: f32, materials: &Materials, gravity: f32, dt: f32, drive: Option<&Drive>) -> Vec<Feedback> {
    begin_pass(parts);
    if !body.is_sleeping() {
        body.underwater = 0.0;
        let near = near_obstacles(body, obstacles, radius, dt);
        for k in 0..parts.len() {
            check_part_obstacles(parts, k, body, &near, radius, materials, dt);
            check_part_terrain(parts, k, body, ground, materials);
        }
    }
    solve_pass(parts, body, gravity, dt, drive)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_cell_fills_the_current_att_group() {
        let calls: Vec<(String, Vec<String>)> = [
            ("MaterialManager.setCell", vec!["40", "5"]),
            ("MaterialManager.attGroup", vec!["201"]),
            ("MaterialManager.setCell", vec!["40", "20"]),
            ("MaterialManager.setCell", vec!["41", "15", "e_Splash"]),
            ("MaterialManager.defGroup", vec!["43"]),
            ("MaterialManager.damageMod", vec!["15"]),
        ]
        .into_iter()
        .map(|(c, a)| (c.to_string(), a.into_iter().map(String::from).collect()))
        .collect();
        let m = Materials::from_commands(calls.iter().map(|(c, a)| (c.as_str(), a.as_slice())));
        assert_eq!(m.cells.get(&(201, 40)), Some(&20.0));
        assert_eq!(m.cells.get(&(201, 41)), Some(&15.0));
        assert_eq!(m.effects.get(&(201, 41)).map(String::as_str), Some("e_Splash"));
        assert_eq!(m.cells.get(&(201, 43)), Some(&15.0));
        assert_eq!(m.cells.len(), 3);
    }

    #[test]
    fn adjust_combines_like_the_engine() {
        assert_eq!(set_adjust(0.0, 2.0), 2.0);
        assert_eq!(set_adjust(1.0, 2.0), 2.0);
        assert_eq!(set_adjust(3.0, 2.0), 3.0);
        assert_eq!(set_adjust(3.0, -2.0), 1.0);
        assert_eq!(set_adjust(-3.0, -2.0), -3.0);
        assert_eq!(set_adjust(-1.0, -2.0), -2.0);
        assert_eq!(set_adjust(-1.0, 2.0), 1.0);
    }

    struct Flat;
    impl Ground for Flat {
        fn height_and_normal(&self, _: f32, _: f32) -> (f32, V3) {
            (0.0, [0.0, 1.0, 0.0])
        }
        fn material(&self, _: f32, _: f32) -> u32 {
            0
        }
        fn water_level(&self) -> f32 {
            -100.0
        }
    }

    #[test]
    fn a_box_comes_to_rest_on_flat_ground() {
        let bbox = [[-1.0, -0.5, -2.0], [1.0, 0.5, 2.0]];
        let mut body = RigidBody::new([0.0, 3.0, 0.0], 1000.0, 0.0, Some(bbox));
        let mut verts = Vec::new();
        for x in [-1.0, 1.0] {
            for y in [-0.5, 0.5] {
                for z in [-2.0, 2.0] {
                    verts.push([x, y, z]);
                }
            }
        }
        let mut parts = vec![Contactor {
            walked: 0,
            name: "box".into(),
            materials: vec![0; verts.len()],
            vertices: verts,
            bbox: Some(bbox),
            pos: [0.0; 3],
            rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            grip: grip::CONTACT,
            is_root: true,
            soldier: false,
            projectile: false,
            spring: None,
            response: Response::default(),
            last_impulses: Vec::new(),
            hits: Vec::new(),
            faces: Vec::new(),
            scale: [1.0; 3],
            radius: 2.3, anchor: None, drive: None, forward: [0.0, 0.0, 1.0], side: (0.0, [1.0, 0.0, 0.0]), engine_above: false, chain: Vec::new(), chain_anchor: Vec::new(),
        }];
        let mats = Materials::default();
        for _ in 0..300 {
            body.step(1.0 / FPS);
            terrain_pass(&mut parts, &mut body, &Flat, &mats, crate::rigid::GRAVITY_Y);
        }
        assert!((body.pos[1] - 0.5).abs() < 0.05, "{:?}", body.pos);
        assert!(body.vel[1].abs() < 1.0, "{:?}", body.vel);
    }
}
