//! `import bytefield`: the headless test bench.
//!
//! ```python
//! import bytefield
//! w = bytefield.World("data/game", "DC_Final", "DC_Basrahs_Edge", mode="conquest")
//! w.world_size, w.water_level, w.height_at(1000, 1200)
//! w.instances()                 # [(template, kind, (x, y, z)), …]
//! w.vehicle("AH64")             # the component tree, as dicts
//! s = w.sim(dt=1/60)
//! i = s.spawn((1000, 300, 1200), (50, 0, 0), gravity_modifier=1.0, drag=0.0, mass=1.0, radius=0.1)
//! s.run(30.0, track=i)          # [(t, (x, y, z), (vx, vy, vz)), …]
//! s.contact(i)                  # ("Ground", t) or None
//! b = bytefield.RigidBody((x, y, z), mass=25000, drag=2, bbox=((-1.6, 0, -3.1), (1.6, 1.5, 3.1)))
//! b.set(vel=(15, 5, -10)); b.step(1/30); b.state()   # the engine's PhysicsNode, in f32
//! s = w.sim(dt=1/30)
//! t = s.add_vehicle("T34", (x, y, z), rotation=(90, 0, 0))   # a handle into the sim
//! t.set_input("c_PIThrottle", 1); s.step(30); t.body()["pos"]
//! w.spawners()                  # [(spawner, team, template, pos, rotation)]
//! vs = s.spawn_level_vehicles # what the level's spawners make at the start
//! ```
//!
//! A `Sim` borrows its `World`; on the Python side it owns a reference to it, so the world
//! stays alive while any sim does.

use std::path::PathBuf;

use bytefield_sim::rigid::RigidBody as CoreRigid;
use bytefield_sim::mover::Vehicle as Mover;
use bytefield_sim::{Sim as CoreSim, World as CoreWorld, vehicle};
use pyo3::exceptions::{PyIndexError, PyKeyError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};

#[pyclass(frozen)]
struct World {
    inner: CoreWorld,
}

#[pymethods]
impl World {
    #[new]
    #[pyo3(signature = (game, mod_name, level, mode = "conquest"))]
    fn new(game: PathBuf, mod_name: &str, level: &str, mode: &str) -> PyResult<World> {
        CoreWorld::load(&game, mod_name, level, mode).map(|inner| World { inner }).map_err(PyRuntimeError::new_err)
    }

    #[getter]
    fn world_size(&self) -> f64 {
        self.inner.level.terrain.world_size
    }

    #[getter]
    fn water_level(&self) -> f64 {
        self.inner.level.terrain.water_level
    }

    /// The soldiers' animation states, by id: (name, [(file, speed, looping)]),
    /// an empty file for an animation slot with none.
    fn animation_states(&self) -> Vec<(String, Vec<(String, f32, bool)>)> {
        self.inner.animations().states.iter().map(|s| (s.name.clone(), s.anims.clone())).collect()
    }

    /// (x, z, width, height) of `game.setActiveCombatArea`, or None
    #[getter]
    fn combat_area(&self) -> Option<(f64, f64, f64, f64)> {
        self.inner.level.combat_area.map(|a| (a[0], a[1], a[2], a[3]))
    }

    fn height_at(&self, x: f64, z: f64) -> Option<f64> {
        self.inner.height_at(x, z)
    }

    /// Terrain height and normal at (x, z), as the engine computes them (None outside).
    fn terrain_at(&self, x: f64, z: f64) -> Option<(f64, (f64, f64, f64))> {
        self.inner.terrain_at(x, z).map(|(h, n)| (h, (n[0], n[1], n[2])))
    }

    fn instances(&self) -> Vec<(String, Option<String>, (f64, f64, f64))> {
        self.inner.level.placed.iter().map(|p| (p.template.clone(), p.kind.clone(), (p.pos[0], p.pos[1], p.pos[2]))).collect()
    }

    /// The level's object spawners: [(spawner template, team, template for that team or None,
    /// position, rotation)], in script order.
    fn spawners(&self) -> Vec<(String, u32, Option<String>, Vec3, Vec3)> {
        self.inner
            .level
            .spawners
            .iter()
            .map(|s| (s.name.clone(), s.team, s.template().map(str::to_string), (s.pos[0], s.pos[1], s.pos[2]), (s.rotation[0], s.rotation[1], s.rotation[2])))
            .collect()
    }

    /// What the spawners make at the start of the round, in the server's order:
    /// [(template, position, rotation, mobile)].
    fn spawned_at_start(&self) -> Vec<(String, Vec3, Vec3, bool)> {
        self.inner
            .spawned_at_start()
            .into_iter()
            .map(|(t, p, r)| {
                let m = self.inner.is_mobile(&t);
                (t, (p[0], p[1], p[2]), (r[0], r[1], r[2]), m)
            })
            .collect()
    }

    /// The component tree of a template (a dict), as `vehicles --show` prints it.
    fn vehicle<'py>(&self, py: Python<'py>, template: &str) -> PyResult<Bound<'py, PyAny>> {
        let c = vehicle::model(&self.inner.db, template).ok_or_else(|| PyKeyError::new_err(template.to_string()))?;
        let json = serde_json::to_string(&c).unwrap();
        py.import("json")?.call_method1("loads", (json,))
    }

    /// The fire arms of a kit (its `addTemplate`s that are), in order: their template names.
    fn kit_weapons(&self, kit: &str) -> Vec<String> {
        self.inner.kit_weapons(kit).into_iter().map(|w| w.name).collect()
    }

    /// The properties of any template, in order: [(name, [args])].
    #[pyo3(signature = (name, namespace = "objecttemplate"))]
    fn template(&self, name: &str, namespace: &str) -> PyResult<(String, Vec<(String, Vec<String>)>)> {
        let t = self.inner.db.get(namespace, name).ok_or_else(|| PyKeyError::new_err(name.to_string()))?;
        Ok((t.kind.clone(), t.props.iter().map(|p| (p.name.clone(), p.args.clone())).collect()))
    }

    /// The rigid physics node of an object template (mass, drag, modifiers, geometry box).
    fn rigid_body(&self, template: &str) -> PyResult<RigidBody> {
        self.inner.rigid_body(template).map(|inner| RigidBody { inner }).ok_or_else(|| PyKeyError::new_err(template.to_string()))
    }

    /// The level's obstacles: [(template, position, radius, collision LODs, scale)].
    fn obstacles(&self) -> Vec<(String, F3, f32, usize, F3)> {
        self.inner.obstacles().iter().map(|o| (o.name.clone(), f3(o.pos), o.radius, o.lods.len(), f3(o.scale))).collect()
    }

    /// The segment test of obstacle `i` (index in the list `obstacles`), collision LOD `lod`:
    /// (normal, point, depth, along, material) or None.
    fn segment(&self, i: usize, lod: usize, start: F3, dir: F3) -> Option<(F3, F3, f32, f32, u32)> {
        let o = self.inner.obstacles().get(i)?;
        o.segment(lod, a3(start), a3(dir), false).map(|h| (f3(h.normal), f3(h.point), h.depth, h.along, h.material))
    }

    /// The geometry box (min, max) the physics uses for a template.
    /// A `ProjectileTemplate` as the sim reads it (dict).
    fn projectile_template<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyAny>> {
        let t = self.inner.projectile_template(name).ok_or_else(|| PyKeyError::new_err(name.to_string()))?;
        to_py(py, &t)
    }

    /// A `FireArms` template's firing data (dict).
    fn weapon<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyAny>> {
        let t = self.inner.weapon(name).ok_or_else(|| PyKeyError::new_err(name.to_string()))?;
        to_py(py, &t)
    }

    /// The segment start → start + dir against the terrain, as a projectile tests it:
    /// (point, normal) or None.
    fn terrain_segment(&self, start: F3, dir: F3) -> Option<(F3, F3)> {
        bytefield_sim::projectile::terrain_segment(&self.inner.level, a3(start), a3(dir)).map(|(p, n)| (f3(p), f3(n)))
    }

    /// The damage table: `getDamageMod(attacker, defender)` and `materialDamage`.
    fn damage_mod(&self, attacker: u32, defender: u32) -> f32 {
        self.inner.materials().damage_mod(attacker, defender)
    }

    fn material_damage(&self, material: u32) -> f32 {
        self.inner.materials().damage(material)
    }

    /// An object template's armor (dict), None without `hasArmor`.
    fn armor_template<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyAny>> {
        let t = self.inner.db.object_template(name).and_then(bytefield_sim::damage::ArmorTemplate::from_template);
        to_py(py, &t)
    }

    /// A projectile's hit on an object of template `target` (fresh armor): the damage, or None
    /// at 0.01 or under. `v`, `n`: the hit's relative velocity and the face's normal.
    #[pyo3(signature = (projectile, target, v, n, face_material, start = (0.0, 0.0, 0.0), point = (0.0, 0.0, 0.0)))]
    fn hit_damage(&self, projectile: &str, target: &str, v: F3, n: F3, face_material: u32, start: F3, point: F3) -> PyResult<Option<f32>> {
        let p = self.inner.projectile_template(projectile).ok_or_else(|| PyKeyError::new_err(projectile.to_string()))?;
        let a = self.armor(target)?;
        Ok(bytefield_sim::damage::hit_damage(&a, &p.damage, self.inner.materials(), a3(start), a3(point), a3(v), a3(n), face_material))
    }

    /// A crash on the terrain or into the water (`handleCollisionLandOrWater`): the damage or
    /// None. `fall`: a soldier's (last collision's height, its height, kit damping).
    #[pyo3(signature = (target, v, n, ground_material, own_material, fall = None))]
    fn land_damage(&self, target: &str, v: F3, n: F3, ground_material: u32, own_material: u32, fall: Option<F3>) -> PyResult<Option<f32>> {
        let a = self.armor(target)?;
        let fall = fall.map(|(last_height, y, damping)| bytefield_sim::damage::Fall { last_height, y, damping });
        Ok(bytefield_sim::damage::land_damage(&a, self.inner.materials(), a3(v), a3(n), ground_material, own_material, fall.as_ref()))
    }

    /// A crash into another object (`handleCollisionObjectVsObject`); `rammer`: its template
    /// (its `damageMod`), or None for an object without armor.
    /// A soldier victim: `fall` as in `land_damage` and `vels` (its velocity, the other's).
    #[pyo3(signature = (target, rammer, v, n, rammer_material, own_material, fall = None, vels = None))]
    #[allow(clippy::too_many_arguments)]
    fn ram_damage(&self, target: &str, rammer: Option<&str>, v: F3, n: F3, rammer_material: u32, own_material: u32, fall: Option<F3>, vels: Option<(F3, F3)>) -> PyResult<Option<f32>> {
        let a = self.armor(target)?;
        let m = match rammer {
            Some(r) => self.armor(r)?.template.damage_mod,
            None => 1.0,
        };
        let fall = fall.map(|(last_height, y, damping)| bytefield_sim::damage::Fall { last_height, y, damping });
        let (own, other) = vels.unwrap_or(((0.0, 0.0, 0.0), (0.0, 0.0, 0.0)));
        let speed = bytefield_sim::damage::soldier_ram_speed(a3(v), a3(own), a3(other));
        Ok(bytefield_sim::damage::ram_damage(&a, m, self.inner.materials(), a3(v), a3(n), rammer_material, own_material, fall.as_ref().map(|f| (f, speed))))
    }

    /// An explosion on an object of template `target` at `pos` with axes `rows` (mobile):
    /// (damage, (point, acceleration) or None) or None outside it / with no damage.
    #[allow(clippy::too_many_arguments)]
    fn blast(&self, target: &str, center: F3, radius: f32, material: u32, damage: f32, force: f32, ymod: f32, pos: F3, rows: (F3, F3, F3)) -> PyResult<Option<(f32, Option<(F3, F3)>)>> {
        let a = self.armor(target)?;
        let e = bytefield_sim::damage::Explosion { center: a3(center), radius, material, damage, force, ymod, by: None };
        let n = bytefield_sim::contact::Ground::height_and_normal(&self.inner.level, center.0, center.2).1;
        let r = [a3(rows.0), a3(rows.1), a3(rows.2)];
        let b = bytefield_sim::damage::blast(&e, &a, self.inner.materials(), a3(pos), &r, self.inner.bounding_radius(target), true, n, None);
        Ok(b.map(|b| (b.damage, b.push.map(|(p, acc)| (f3(p), f3(acc))))))
    }

    fn physics_box(&self, template: &str) -> Option<(F3, F3)> {
        self.inner.physics_box(template).map(|[a, b]| (f3(a), f3(b)))
    }

    #[pyo3(signature = (dt = 1.0 / 60.0))]
    fn sim(slf: Py<World>, dt: f64) -> Sim {
        Sim {
            world: slf,
            dt,
            time: 0.0,
            steps: 0,
            bodies: Vec::new(),
            vehicles: Vec::new(),
            projectiles: Vec::new(),
            fired: Vec::new(),
            spawner_frames: None,
            pending_damage: Vec::new(),
            damage_log: Vec::new(),
            explosions: Vec::new(),
            pushes: Vec::new(),
            spawn_log: Vec::new(),
            spawners: Vec::new(),
            items: Vec::new(),
            conquest: None,
            players: Vec::new(),
            player_log: Vec::new(),
            rng: bytefield_sim::players::Rng(0x9E37_79B9_7F4A_7C15),
            waves: Some(bytefield_sim::players::Waves::default()),
            playing: false,
            team_kills: Default::default(),
        }
    }
}

/// The state lives here (not in a borrowed core Sim) so Python can hold it freely; each call
/// rebuilds a core Sim over the world for the duration of the call.
impl World {
    fn armor(&self, template: &str) -> PyResult<bytefield_sim::damage::Armor> {
        let t = self.inner.db.object_template(template).ok_or_else(|| PyKeyError::new_err(template.to_string()))?;
        let a = bytefield_sim::damage::ArmorTemplate::from_template(t).ok_or_else(|| PyValueError::new_err(format!("{template} has no armor")))?;
        Ok(bytefield_sim::damage::Armor::new(a))
    }
}

#[pyclass]
struct Sim {
    world: Py<World>,
    dt: f64,
    time: f64,
    steps: u64,
    bodies: Vec<bytefield_sim::Body>,
    vehicles: Vec<Mover>,
    projectiles: Vec<bytefield_sim::projectile::Projectile>,
    fired: Vec<(u64, usize, bytefield_sim::projectile::Shot, Option<usize>)>,
    spawner_frames: Option<Vec<f32>>,
    pending_damage: Vec<(usize, f32, Option<usize>)>,
    damage_log: Vec<bytefield_sim::damage::DamageEvent>,
    explosions: Vec<(u64, bytefield_sim::damage::Explosion)>,
    pushes: Vec<(u64, usize, [f32; 3], [f32; 3])>,
    spawn_log: Vec<(u64, usize, usize)>,
    spawners: Vec<bytefield_sim::damage::SpawnerState>,
    items: Vec<bytefield_sim::create::Item>,
    conquest: Option<bytefield_sim::conquest::Conquest>,
    players: Vec<bytefield_sim::players::Player>,
    player_log: Vec<(u64, usize, bytefield_sim::players::PlayerEvent)>,
    rng: bytefield_sim::players::Rng,
    waves: Option<bytefield_sim::players::Waves>,
    playing: bool,
    team_kills: bytefield_sim::players::TeamKills,
}

impl Sim {
    fn with<R>(&mut self, py: Python, f: impl FnOnce(&mut CoreSim) -> R) -> R {
        let w = self.world.bind(py).get();
        let mut s = CoreSim::new(&w.inner, self.dt);
        s.clock.time = self.time;
        s.clock.steps = self.steps;
        s.bodies = std::mem::take(&mut self.bodies);
        s.vehicles = std::mem::take(&mut self.vehicles);
        s.projectiles = std::mem::take(&mut self.projectiles);
        s.journal.fired = std::mem::take(&mut self.fired);
        s.spawner_frames = self.spawner_frames.take();
        s.damage.pending_damage = std::mem::take(&mut self.pending_damage);
        s.journal.damage_log = std::mem::take(&mut self.damage_log);
        s.journal.explosions = std::mem::take(&mut self.explosions);
        s.journal.pushes = std::mem::take(&mut self.pushes);
        s.journal.spawn_log = std::mem::take(&mut self.spawn_log);
        s.spawners = std::mem::take(&mut self.spawners);
        s.items = std::mem::take(&mut self.items);
        s.modes.conquest = self.conquest.take();
        s.players = std::mem::take(&mut self.players);
        s.journal.player_log = std::mem::take(&mut self.player_log);
        s.rng = self.rng.clone();
        s.round.waves = self.waves.take();
        s.round.playing = self.playing;
        s.rules.team_kills = self.team_kills.clone();
        let r = f(&mut s);
        self.waves = s.round.waves.take();
        self.playing = s.round.playing;
        self.team_kills = s.rules.team_kills.clone();
        self.players = std::mem::take(&mut s.players);
        self.player_log = std::mem::take(&mut s.journal.player_log);
        self.rng = s.rng.clone();
        self.conquest = s.modes.conquest.take();
        self.pending_damage = s.damage.pending_damage;
        self.fired = std::mem::take(&mut s.journal.fired);
        self.spawner_frames = s.spawner_frames.take();
        self.damage_log = s.journal.damage_log;
        self.explosions = s.journal.explosions;
        self.pushes = s.journal.pushes;
        self.spawn_log = s.journal.spawn_log;
        self.spawners = s.spawners;
        self.items = s.items;
        self.time = s.clock.time;
        self.steps = s.clock.steps;
        self.bodies = s.bodies;
        self.vehicles = s.vehicles;
        self.projectiles = s.projectiles;
        r
    }
}

type Vec3 = (f64, f64, f64);

#[pymethods]
impl Sim {
    #[getter]
    fn time(&self) -> f64 {
        self.time
    }

    #[getter]
    fn steps(&self) -> u64 {
        self.steps
    }

    #[pyo3(signature = (pos, vel, gravity_modifier = 1.0, drag = 0.0, mass = 1.0, radius = 0.1))]
    #[allow(clippy::too_many_arguments)]
    fn spawn(&mut self, py: Python, pos: Vec3, vel: Vec3, gravity_modifier: f64, drag: f64, mass: f64, radius: f64) -> usize {
        self.with(py, |s| s.spawn([pos.0, pos.1, pos.2], [vel.0, vel.1, vel.2], gravity_modifier, drag, mass, radius))
    }

    /// `n` frames. `parts_last`: integrate, collide, then the parts of the next frame (to line
    /// up with an original reference run's records: `Sim::step_ordered`).
    #[pyo3(signature = (n = 1, parts_last = false))]
    fn step(&mut self, py: Python, n: u64, parts_last: bool) {
        self.with(py, |s| {
            for _ in 0..n {
                s.step_ordered(parts_last);
            }
        })
    }

    /// One phase of the vehicles' frame alone: "parts", "integrate" or "collide" (`vehicle`: only
    /// that one, for the first two). The clock does not move. For lining up with a reference run whose
    /// objects the server integrated in another order (`Sim::phase`).
    #[pyo3(signature = (name, vehicle = None))]
    fn phase(&mut self, py: Python, name: &str, vehicle: Option<usize>) -> PyResult<()> {
        use bytefield_sim::Phase;
        let p = match name {
            "parts" => Phase::Parts,
            "integrate" => Phase::Integrate,
            "collide" => Phase::Collide,
            _ => return Err(PyKeyError::new_err(name.to_string())),
        };
        if vehicle.is_some_and(|i| i >= self.vehicles.len()) {
            return Err(PyKeyError::new_err(vehicle));
        }
        self.with(py, |s| s.phase(p, vehicle));
        Ok(())
    }

    /// Add a vehicle (at rest; `rotation`: yaw, pitch, roll in degrees); its handle.
    #[pyo3(signature = (template, pos = (0.0, 0.0, 0.0), rotation = (0.0, 0.0, 0.0)))]
    fn add_vehicle(slf: Py<Sim>, py: Python, template: &str, pos: F3, rotation: F3) -> PyResult<Vehicle> {
        let index = slf
            .borrow_mut(py)
            .with(py, |s| s.spawn_vehicle(template, a3(pos), a3(rotation)))
            .ok_or_else(|| PyKeyError::new_err(template.to_string()))?;
        Ok(Vehicle { sim: slf, index })
    }

    /// Add the vehicles the level's spawners make at the start (`Sim::spawn_level_vehicles`).
    fn spawn_level_vehicles(slf: Py<Sim>, py: Python) -> Vec<Vehicle> {
        let ix = slf.borrow_mut(py).with(py, |s| s.spawn_level_vehicles());
        ix.into_iter().map(|index| Vehicle { sim: slf.clone_ref(py), index }).collect()
    }

    fn vehicle(slf: Py<Sim>, py: Python, index: usize) -> PyResult<Vehicle> {
        if index >= slf.borrow(py).vehicles.len() {
            return Err(PyKeyError::new_err(index));
        }
        Ok(Vehicle { sim: slf, index })
    }

    /// A weapon (`FireArms` template) fires: barrel axes `rows` (z: the direction) at `pos`;
    /// `root_vel`: the speed of what carries it (None: nothing with a physics node). The shell.
    #[pyo3(signature = (weapon, rows, pos, root_vel = None))]
    fn fire(slf: Py<Sim>, py: Python, weapon: &str, rows: (F3, F3, F3), pos: F3, root_vel: Option<F3>) -> PyResult<Projectile> {
        let rows = [a3(rows.0), a3(rows.1), a3(rows.2)];
        let index = slf
            .borrow_mut(py)
            .with(py, |s| s.fire(weapon, rows, a3(pos), root_vel.map(a3)))
            .ok_or_else(|| PyKeyError::new_err(weapon.to_string()))?;
        Ok(Projectile { sim: slf, index })
    }

    /// A projectile of `template` at `pos` with speed `vel` (axes `rows`, default the world's).
    #[pyo3(signature = (template, pos, vel, rows = None, rotvel = None))]
    fn add_projectile(slf: Py<Sim>, py: Python, template: &str, pos: F3, vel: F3, rows: Option<(F3, F3, F3)>, rotvel: Option<F3>) -> PyResult<Projectile> {
        let rows = rows.map_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], |r| [a3(r.0), a3(r.1), a3(r.2)]);
        let shot = bytefield_sim::projectile::Shot { pos: a3(pos), rows, vel: a3(vel), rotvel: rotvel.map_or([0.0; 3], a3) };
        let index = slf
            .borrow_mut(py)
            .with(py, |s| s.add_projectile(template, &shot))
            .ok_or_else(|| PyKeyError::new_err(template.to_string()))?;
        Ok(Projectile { sim: slf, index })
    }

    /// The projectiles' handles, in the order they were fired.
    fn projectiles(slf: Py<Sim>, py: Python) -> Vec<Projectile> {
        let n = slf.borrow(py).projectiles.len();
        (0..n).map(|index| Projectile { sim: slf.clone_ref(py), index }).collect()
    }

    /// Every damage given so far: dicts with step, vehicle, amount, source
    /// ("Hit", "Explosion", "Crash", "Burning", "UpsideDown"), status and hp after.
    fn damage_log<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.damage_log)
    }

    /// The spawners' frame updates (holding objects, the
    /// countdowns) to run at the end of the next step, with these dts (a reference run's
    /// `spawner_frame`s); from the first call on, no others run.
    fn spawner_frames(&mut self, dts: Vec<f32>) {
        self.spawner_frames.get_or_insert_with(Vec::new).extend(dts);
    }

    /// The soldiers' shots so far: (step, vehicle, {pos, rows, vel, rotvel}, projectile index).
    fn fired<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.fired)
    }

    /// Every explosion so far: (step, {center, radius, material, damage, force, ymod}).
    fn explosions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.explosions)
    }

    /// Every explosion's push: (step, vehicle, point, acceleration).
    fn pushes(&self) -> Vec<(u64, usize, F3, F3)> {
        self.pushes.iter().map(|&(st, v, p, a)| (st, v, f3(p), f3(a))).collect()
    }

    /// The level's vehicle spawners (after `spawn_level_vehicles`): name, template, slot,
    /// last, countdown...
    fn spawners<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.spawners)
    }

    /// The game playing (`setGameStatus(1)`): the world time runs (in pre-game it stays 0, and a
    /// soldier faster than 8 m/s flies as if just blown up).
    fn set_playing(&mut self, py: Python, playing: bool) {
        self.with(py, |s| s.round.playing = playing);
    }

    /// Start a conquest round: the level's vehicles, the tickets, the control points.
    fn start_conquest(slf: Py<Sim>, py: Python) -> Vec<Vehicle> {
        let ix = slf.borrow_mut(py).with(py, |s| s.start_conquest());
        ix.into_iter().map(|index| Vehicle { sim: slf.clone_ref(py), index }).collect()
    }

    /// Only the control points' frame with these players ((team, position) each) and dt (a
    /// reference run's real-clock one); their spawners follow.
    fn control_points_frame(&mut self, py: Python, dt: f32, players: Vec<(u32, F3)>) {
        let ps: Vec<(u32, [f32; 3])> = players.into_iter().map(|(t, p)| (t, a3(p))).collect();
        self.with(py, |s| {
            let step = s.clock.steps;
            if let Some(c) = s.modes.conquest.as_mut() {
                let sw = c.points_frame(step, dt, &ps);
                s.switch_spawners(&sw);
            }
        });
    }

    /// The conquest state (dict: points, groups, rules, tickets, timers, log) or None.
    fn conquest<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.conquest)
    }

    /// Every object a spawner made after the start: (step, spawner index, vehicle index).
    fn spawn_log(&self) -> Vec<(u64, usize, usize)> {
        self.spawn_log.clone()
    }

    /// `handleExplosion` at `center`: every armored vehicle (but `skip`) pushed now, damaged
    /// the next frame.
    #[pyo3(signature = (center, radius, material, damage, force, ymod = 1.0, skip = None))]
    #[allow(clippy::too_many_arguments)]
    fn explode(&mut self, py: Python, center: F3, radius: f32, material: u32, damage: f32, force: f32, ymod: f32, skip: Option<usize>) {
        let e = bytefield_sim::damage::Explosion { center: a3(center), radius, material, damage, force, ymod, by: None };
        self.with(py, |s| s.explode(e, skip));
    }

    /// A player of `team` that spawns (as `template`) by itself in conquest; its index.
    #[pyo3(signature = (team, template, ai = true))]
    fn add_player(&mut self, py: Python, team: u32, template: &str, ai: bool) -> usize {
        self.with(py, |s| s.add_player(team, template, ai))
    }

    /// The players (list of dicts: team, template, ai, soldier, alive, spawn_time, group, deaths).
    fn players<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.players)
    }

    /// Spawn waves: (wait, to_next, period, add), or None for
    /// `serverSpawnDelay` alone.
    #[setter]
    fn set_waves(&mut self, w: Option<(f32, f32, f32, f32)>) {
        self.waves = w.map(|(wait, to_next, period, add)| bytefield_sim::players::Waves { wait, to_next, period, add });
    }

    #[getter]
    fn waves(&self) -> Option<(f32, f32, f32, f32)> {
        self.waves.as_ref().map(|w| (w.wait, w.to_next, w.period, w.add))
    }

    /// Choose a player's spawn group.
    fn set_player_group(&mut self, index: usize, group: Option<i64>) -> PyResult<()> {
        self.players.get_mut(index).ok_or_else(|| PyIndexError::new_err("no such player"))?.group = group;
        Ok(())
    }

    /// What happened to the players: (step, player, {"Died": vehicle} / {"Spawned": [vehicle, group, point]}).
    fn player_log<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.player_log)
    }

    /// `giveDamage` to a vehicle (negative heals), with what follows; `by`: the vehicle that
    /// gives it (its player's, for team kills).
    #[pyo3(signature = (vehicle, amount, by = None))]
    fn give_damage(&mut self, py: Python, vehicle: usize, amount: f32, by: Option<usize>) {
        self.with(py, |s| {
            s.damage.attacker = by;
            s.give_damage(vehicle, amount, bytefield_sim::damage::Source::Hit);
            s.damage.attacker = None;
        });
    }

    /// Set a spawner's countdown (seconds).
    fn set_spawner_countdown(&mut self, index: usize, countdown: f32) -> PyResult<()> {
        let s = self.spawners.get_mut(index).ok_or_else(|| PyIndexError::new_err("no such spawner"))?;
        s.countdown = countdown;
        Ok(())
    }

    /// The vehicles' handles, in the order they were added.
    fn vehicles(slf: Py<Sim>, py: Python) -> Vec<Vehicle> {
        let n = slf.borrow(py).vehicles.len();
        (0..n).map(|index| Vehicle { sim: slf.clone_ref(py), index }).collect()
    }

    #[pyo3(signature = (max_time, track = 0))]
    fn run(&mut self, py: Python, max_time: f64, track: usize) -> PyResult<Vec<(f64, Vec3, Vec3)>> {
        if track >= self.bodies.len() {
            return Err(PyKeyError::new_err(track));
        }
        Ok(self.with(py, |s| s.run(max_time, track)).into_iter().map(|x| (x.t, (x.pos[0], x.pos[1], x.pos[2]), (x.vel[0], x.vel[1], x.vel[2]))).collect())
    }

    fn state(&self, i: usize) -> PyResult<(Vec3, Vec3)> {
        let b = self.bodies.get(i).ok_or_else(|| PyKeyError::new_err(i))?;
        Ok(((b.pos[0], b.pos[1], b.pos[2]), (b.vel[0], b.vel[1], b.vel[2])))
    }

    fn contact(&self, i: usize) -> PyResult<Option<(String, f64)>> {
        let b = self.bodies.get(i).ok_or_else(|| PyKeyError::new_err(i))?;
        Ok(b.contact.map(|(c, t)| (format!("{c:?}"), t)))
    }
}

type F3 = (f32, f32, f32);

fn f3(v: [f32; 3]) -> F3 {
    (v[0], v[1], v[2])
}

fn a3(v: F3) -> [f32; 3] {
    [v.0, v.1, v.2]
}

/// One rigid physics node (`bytefield_sim::rigid`), stepped by hand: for comparing with the original.
#[pyclass]
struct RigidBody {
    inner: CoreRigid,
}

#[pymethods]
impl RigidBody {
    #[new]
    #[pyo3(signature = (pos, mass = 1.0, drag = 0.0, bbox = None))]
    fn new(pos: F3, mass: f32, drag: f32, bbox: Option<(F3, F3)>) -> RigidBody {
        RigidBody { inner: CoreRigid::new(a3(pos), mass, drag, bbox.map(|(a, b)| [a3(a), a3(b)])) }
    }

    /// Every field, as a dict (vectors as tuples; `rows` as three tuples).
    fn state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let b = &self.inner;
        let d = PyDict::new(py);
        for (k, v) in [("pos", b.pos), ("vel", b.vel), ("rotvel", b.rotvel), ("acc", b.acc), ("rotacc", b.rotacc), ("friction", b.friction), ("rot_friction", b.rot_friction),
                       ("inertia_modifier", b.inertia_modifier), ("wind", b.wind)] {
            d.set_item(k, f3(v))?;
        }
        for (k, v) in [("mass", b.mass), ("drag", b.drag), ("gravity_modifier", b.gravity_modifier), ("underwater", b.underwater)] {
            d.set_item(k, v)?;
        }
        d.set_item("rows", (f3(b.rows[0]), f3(b.rows[1]), f3(b.rows[2])))?;
        d.set_item("sleepiness", b.sleepiness)?;
        Ok(d)
    }

    /// Set fields by name: `b.set(vel=(1, 2, 3), rows=(...), mass=25000)`.
    #[pyo3(signature = (**fields))]
    fn set(&mut self, fields: Option<&Bound<'_, PyDict>>) -> PyResult<()> {
        let Some(fields) = fields else { return Ok(()) };
        let b = &mut self.inner;
        for (k, v) in fields.iter() {
            let k: String = k.extract()?;
            match k.as_str() {
                "pos" => b.pos = a3(v.extract()?),
                "vel" => b.vel = a3(v.extract()?),
                "rotvel" => b.rotvel = a3(v.extract()?),
                "acc" => b.acc = a3(v.extract()?),
                "rotacc" => b.rotacc = a3(v.extract()?),
                "friction" => b.friction = a3(v.extract()?),
                "rot_friction" => b.rot_friction = a3(v.extract()?),
                "inertia_modifier" => b.inertia_modifier = a3(v.extract()?),
                "center_of_mass" => b.center_of_mass = a3(v.extract()?),
                "wind" => b.wind = a3(v.extract()?),
                "mass" => b.mass = v.extract()?,
                "drag" => b.drag = v.extract()?,
                "gravity_modifier" => b.gravity_modifier = v.extract()?,
                "underwater" => b.underwater = v.extract()?,
                "sleepiness" => b.sleepiness = v.extract()?,
                "rows" => {
                    let r: (F3, F3, F3) = v.extract()?;
                    b.rows = [a3(r.0), a3(r.1), a3(r.2)];
                }
                _ => return Err(PyKeyError::new_err(k)),
            }
        }
        Ok(())
    }

    /// The geometry box (min, max), or None.
    #[getter]
    fn bbox(&self) -> Option<(F3, F3)> {
        self.inner.bbox.map(|[a, b]| (f3(a), f3(b)))
    }

    /// Add an acceleration `a` at the world point `pos` (or at the offset `pos`, world axes,
    /// with `relative=True`), as the engine's components do.
    #[pyo3(signature = (pos, a, relative = false))]
    fn add_acc_at(&mut self, pos: F3, a: F3, relative: bool) {
        if relative {
            self.inner.add_acc_at_rel(a3(pos), a3(a));
        } else {
            self.inner.add_acc_at(a3(pos), a3(a));
        }
    }

    /// World velocity of the body's point `pos`.
    fn velocity_at(&self, pos: F3) -> F3 {
        f3(self.inner.velocity_at(a3(pos)))
    }

    /// The drag cross sections (xy, zx, yz), m², or None without geometry.
    fn sections(&self) -> Option<F3> {
        self.inner.sections().map(f3)
    }

    #[pyo3(signature = (dt = 1.0 / 30.0, n = 1))]
    fn step(&mut self, dt: f32, n: u32) {
        for _ in 0..n {
            self.inner.step(dt);
        }
    }
}

/// A vehicle in a `Sim` (`bytefield_sim::mover`): a handle, the state lives in the sim. Stepped by
/// `Sim.step`, with every other vehicle.
#[pyclass]
struct Vehicle {
    sim: Py<Sim>,
    index: usize,
}

impl Vehicle {
    fn get<R>(&self, py: Python, f: impl FnOnce(&Mover) -> R) -> R {
        f(&self.sim.borrow(py).vehicles[self.index])
    }

    fn get_mut<R>(&self, py: Python, f: impl FnOnce(&mut Mover) -> R) -> R {
        f(&mut self.sim.borrow_mut(py).vehicles[self.index])
    }
}

fn to_py<'py>(py: Python<'py>, v: &impl serde::Serialize) -> PyResult<Bound<'py, PyAny>> {
    let s = serde_json::to_string(v).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    py.import("json")?.call_method1("loads", (s,))
}

#[pymethods]
impl Vehicle {
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    #[getter]
    fn template(&self, py: Python) -> String {
        self.get(py, |v| v.template.clone())
    }

    /// A fixed gun: no mobile physics (it is not integrated nor collides).
    #[getter]
    fn fixed(&self, py: Python) -> bool {
        self.get(py, |v| v.fixed)
    }

    /// A control input; it also turns the engines on, as a driver would (an empty vehicle's are off, rpm 0 and free
    /// wheels), the way the reference harness and `phys_diff` drive one.
    fn set_input(&self, py: Python, name: &str, value: f32) -> PyResult<()> {
        if self.get_mut(py, |v| {
            let known = v.craft.set_input(name, value);
            if known {
                v.set_engines_on(true);
            }
            known
        }) {
            Ok(())
        } else {
            Err(PyKeyError::new_err(name.to_string()))
        }
    }

    /// The root's rigid body state (as the `state` of a `RigidBody`).
    fn body<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        RigidBody { inner: self.get(py, |v| v.craft.body.clone()) }.state(py)
    }

    /// Set root body fields (as `RigidBody.set`).
    #[pyo3(signature = (**fields))]
    fn set_body(&self, py: Python, fields: Option<&Bound<'_, PyDict>>) -> PyResult<()> {
        let mut b = RigidBody { inner: self.get(py, |v| v.craft.body.clone()) };
        b.set(fields)?;
        self.get_mut(py, |v| v.craft.body = b.inner);
        Ok(())
    }

    /// The parts with their state, as JSON-like dicts.
    fn parts<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let parts = self.get(py, |v| serde_json::to_value(&v.craft.parts)).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        to_py(py, &parts)
    }

    /// What each part added in the last update: [(part, (x, y, z), (ax, ay, az))].
    fn last_adds(&self, py: Python) -> Vec<(String, F3, F3)> {
        self.get(py, |v| v.craft.last_adds.iter().map(|(n, p, a)| (n.clone(), f3(*p), f3(*a))).collect())
    }

    /// Set a part's state: angle/speed (yaw, pitch, roll), rpm, load, regulator.
    #[pyo3(signature = (name, angle = None, speed = None, rpm = None, load = None, regulator = None))]
    #[allow(clippy::too_many_arguments)]
    fn set_part(&self, py: Python, name: &str, angle: Option<F3>, speed: Option<F3>, rpm: Option<f32>, load: Option<f32>, regulator: Option<f32>) -> PyResult<()> {
        let mut sim = self.sim.borrow_mut(py);
        set_craft_part(&mut sim.vehicles[self.index].craft, name, angle, speed, rpm, load, regulator)
    }

    /// Turn every engine on or off (an engine runs only with a driver).
    fn set_engines_on(&self, py: Python, on: bool) {
        self.get_mut(py, |v| v.set_engines_on(on));
    }

    /// The parts that touch things, with their response state and last impulses.
    fn contacts<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let v: Vec<serde_json::Value> = self.get(py, |v| {
            v.contacts
                .iter()
                .map(|c| {
                    let mut j = serde_json::to_value(c).unwrap_or_default();
                    j["impulses"] = serde_json::to_value(&c.last_impulses).unwrap_or_default();
                    j.as_object_mut().map(|o| o.remove("vertices"));
                    j
                })
                .collect()
        });
        to_py(py, &v)
    }

    /// Its armor: hp, max_hp, destroyed, critical, accum, dead, death_timer... and
    /// the template's numbers; None without `hasArmor`.
    fn armor<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let a = self.get(py, |v| v.armor.clone());
        to_py(py, &a)
    }

    /// Set armor fields: hp, accum (the seconds since its last tick), death_timer.
    #[pyo3(signature = (hp=None, accum=None, death_timer=None))]
    fn set_armor(&self, py: Python, hp: Option<f32>, accum: Option<f32>, death_timer: Option<f32>) -> PyResult<()> {
        self.get_mut(py, |v| {
            let a = v.armor.as_mut().ok_or_else(|| PyValueError::new_err("no armor"))?;
            if let Some(x) = hp {
                a.hp = x;
            }
            if let Some(x) = accum {
                a.accum = x;
            }
            if let Some(x) = death_timer {
                a.death_timer = x;
            }
            Ok(())
        })
    }

    /// A soldier's team (its player's; 0: none): control points count soldiers with a team.
    #[getter]
    fn team(&self, py: Python) -> u32 {
        self.get(py, |v| v.team)
    }

    #[setter]
    fn set_team(&self, py: Python, team: u32) {
        self.get_mut(py, |v| v.team = team)
    }

    /// Gone from the world (the wreck's time ran out, or a spawner cleared it).
    #[getter]
    fn removed(&self, py: Python) -> bool {
        self.get(py, |v| v.removed)
    }

    /// The bounding radius used for the collision (`getBoundingRadius`).
    #[getter]
    fn radius(&self, py: Python) -> f32 {
        self.get(py, |v| v.radius)
    }

    /// Whether it is a soldier.
    #[getter]
    fn is_soldier(&self, py: Python) -> bool {
        self.get(py, |v| v.is_soldier())
    }

    /// A soldier's input as the velocity it asks for (world, m/s), kept until changed: the
    /// friction pulls it towards it while it stands on something.
    fn set_walk(&self, py: Python, v: F3) {
        self.get_mut(py, |s| s.set_walk(a3(v)));
    }

    /// A soldier's input as `handlePlayerInput` reads it: forward and strafe in −1..1 along its
    /// own axes, pose 0 stand / 1 crouch / 2 lie, `walking` (the slow walk); kept until changed,
    /// the speed ramps towards it frame by frame (`soldier::Movement`).
    #[pyo3(signature = (forward, strafe = 0.0, pose = 0, walking = false))]
    fn set_move(&self, py: Python, forward: f32, strafe: f32, pose: usize, walking: bool) {
        self.get_mut(py, |s| s.set_move(forward, strafe, pose, walking));
    }

    /// A soldier's input from now on, as `handlePlayerInput` reads it: {c_PI index: value}; the
    /// axes given are mapped, the rest not (as a recorded run's soldier input).
    fn set_soldier_input(&self, py: Python, axes: std::collections::HashMap<usize, f32>) {
        let mut a: Vec<(usize, f32)> = axes.into_iter().collect();
        a.sort_by_key(|x| x.0);
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                m.set_input(&a);
            }
        });
    }

    /// A soldier's movement factors (forward, strafe: −127..127), as a reference run's record has them
    /// (its `factors` are strafe, forward).
    fn set_factors(&self, py: Python, forward: i32, strafe: i32) {
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                m.factor = [forward, strafe];
            }
        });
    }

    /// The animations' frame updates to run before the soldier's next input (a reference run's
    /// `soldier_frame` dts); from the first call on, no other frame updates run.
    fn anim_frames(&self, py: Python, dts: Vec<f32>) {
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                m.frames.get_or_insert_with(Vec::new).extend(dts);
            }
        });
    }

    /// A soldier's movement state: the lower body's animation state (name, time), the movement
    /// factors (forward, strafe), the head's yaw, the pose; None for a vehicle.
    fn soldier<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        let v = self.get(py, |s| {
            s.movement.as_ref().map(|m| {
                let name = |h: &bytefield_sim::anim::Instance| h.state.map(|i| m.machine.states[i].name.clone());
                serde_json::json!({"state": name(&m.lower), "time": m.lower.time, "morph": m.lower.morph,
                    "upper": name(&m.upper), "upper_time": m.upper.time, "upper_morph": m.upper.morph, "item": m.item,
                    "idle": [m.lower.idle, m.upper.idle],
                    "factors": m.factor, "head_yaw": m.head_yaw, "pitch": m.pitch, "head": m.head, "lean": m.lean,
                    "bits": m.bits, "pose": m.pose()})
            })
        });
        v.map(|v| to_py(py, &v)).transpose()
    }

    /// A soldier's collision skeleton: each bone's absolute matrix (16 floats, the
    /// server's layout, in the soldier's frame), its collision bones (bone, radius, shift,
    /// material) and the countdown to the next sample; None for a vehicle.
    fn skeleton<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        let v = self.get(py, |s| {
            s.movement.as_ref().and_then(|m| {
                m.skeleton.as_ref().map(|k| {
                    let names: Vec<&str> = k.file.bones.iter().map(|b| b.name.as_str()).collect();
                    serde_json::json!({"abs": k.abs, "names": names, "collision": k.collision, "sample_in": m.sample_in})
                })
            })
        });
        v.map(|v| to_py(py, &v)).transpose()
    }

    /// The distance to the geometry of the segment start → start + dir against
    /// the soldier where it is: the bone hit, its material, point and normal, or None.
    fn skeleton_segment<'py>(&self, py: Python<'py>, start: F3, dir: F3) -> PyResult<Option<Bound<'py, PyAny>>> {
        let h = self.get(py, |s| {
            let b = &s.craft.body;
            s.movement.as_ref().and_then(|m| m.skeleton.as_ref()).and_then(|k| k.segment(&b.rows, b.pos, a3(start), a3(dir)))
        });
        h.map(|h| to_py(py, &h)).transpose()
    }

    /// Pose the collision skeleton now with the lower body's state (or this one, by name) at
    /// this time and morph (a reference run's, at the server's sample), and set the countdown to the
    /// next sample. `upper`: the upper body's (state, time, morph); `aim`: the soldier's pitch,
    /// head yaw off the body, the head's own pitch and yaw; `lean`: the lean
    /// (and no more leaning); `dt`: the update's (the lean moves by it).
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (time, morph = 1.0, sample_in = None, state = None, upper = None, aim = None, lean = None, dt = 0.0))]
    fn sample_skeleton(
        &self,
        py: Python,
        time: f32,
        morph: f32,
        sample_in: Option<f32>,
        state: Option<&str>,
        upper: Option<(String, f32, f32)>,
        aim: Option<[f32; 4]>,
        lean: Option<f32>,
        dt: f32,
    ) -> PyResult<()> {
        let mut missing = None;
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                let machine = m.machine.clone();
                let mut put = |h: &mut bytefield_sim::anim::Instance, name: Option<&str>, time: f32, morph: f32| {
                    if let Some(name) = name {
                        match machine.find(name) {
                            Some(i) => h.set(i),
                            None => missing = Some(name.to_string()),
                        }
                    }
                    if h.state.is_some() && h.prev != h.state {
                        h.prev = h.state;
                        h.anim = h.state.and_then(|i| machine.states[i].anim());
                    }
                    h.time = time;
                    h.morph = morph;
                };
                put(&mut m.lower, state, time, morph);
                if let Some((name, t, w)) = &upper {
                    put(&mut m.upper, Some(name), *t, *w);
                }
                if let Some([p, y, h0, h1]) = aim {
                    (m.pitch, m.head_yaw, m.head) = (p, y, [h0, h1]);
                }
                if let Some(l) = lean {
                    m.lean = [l, l];
                    m.bits = 0;
                }
                m.pose_skeleton(dt);
                if let Some(t) = sample_in {
                    m.sample_in = t;
                }
            }
        });
        match missing {
            None => Ok(()),
            Some(n) => Err(PyValueError::new_err(format!("no animation state {n:?}"))),
        }
    }

    /// Put a soldier's body halves in these states (by name) at this time and morph, set up
    /// (as a reference run's record has them); the upper body's is optional.
    #[pyo3(signature = (state, time, morph, upper = None))]
    fn set_animation(&self, py: Python, state: &str, time: f32, morph: f32, upper: Option<(String, f32, f32)>) -> PyResult<()> {
        let mut missing = None;
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                let machine = m.machine.clone();
                let mut put = |h: &mut bytefield_sim::anim::Instance, name: &str, time: f32, morph: f32| match machine.find(name) {
                    Some(i) => {
                        h.set(i);
                        h.prev = h.state;
                        h.anim = machine.states[i].anim();
                        (h.time, h.morph) = (time, morph);
                    }
                    None => missing = Some(name.to_string()),
                };
                put(&mut m.lower, state, time, morph);
                if let Some((name, t, w)) = &upper {
                    put(&mut m.upper, name, *t, *w);
                }
            }
        });
        match missing {
            None => Ok(()),
            Some(n) => Err(PyValueError::new_err(format!("no animation state {n:?}"))),
        }
    }

    /// What the server's `rand` decided for a body half (0 lower, 1 upper), which we cannot
    /// know: its idle timer now, and the idle it goes to next (by name).
    #[pyo3(signature = (half, timer = None, pick = None))]
    fn set_idle(&self, py: Python, half: usize, timer: Option<f32>, pick: Option<&str>) {
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                let p = pick.and_then(|n| m.machine.find(n));
                let h = if half == 0 { &mut m.lower } else { &mut m.upper };
                if let Some(t) = timer {
                    h.idle = t;
                }
                if p.is_some() {
                    h.pick = p;
                }
            }
        });
    }

    /// The soldier's active item, by its template name (the upper body's states are named
    /// after it: `Ub_StandAimDP`...): `enableItem`, which raises it.
    fn set_item(&self, py: Python, name: &str) {
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                m.enable_item(name);
            }
        });
    }

    /// The soldier's weapon: a `HandFireArms`/`FireArms` template, raised
    /// (`set_item`) and loaded (full magazines, a round in the chamber).
    fn set_weapon(&self, py: Python, name: &str) -> PyResult<()> {
        let w = {
            let sim = self.sim.borrow(py);
            let world = sim.world.bind(py).get();
            world.inner.weapon(name).ok_or_else(|| PyKeyError::new_err(name.to_string()))?
        };
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                m.enable_item(name);
                m.weapon = Some(bytefield_sim::firearms::FireArms::new(std::sync::Arc::new(w)));
            }
        });
        Ok(())
    }

    /// The soldier's kit: every fire arm of a `Kit` template, loaded, by slot; the
    /// one named `active` (or the first) in its hand, raised.
    #[pyo3(signature = (kit, active = None))]
    fn set_kit(&self, py: Python, kit: &str, active: Option<&str>) -> PyResult<()> {
        let ws = {
            let sim = self.sim.borrow(py);
            let world = sim.world.bind(py).get();
            world.inner.kit_weapons(kit)
        };
        if ws.is_empty() {
            return Err(PyKeyError::new_err(kit.to_string()));
        }
        self.get_mut(py, |s| {
            s.give_kit(ws, active);
        });
        Ok(())
    }

    /// The weapon's state (magazines, chamber, timers, deviation, seed…) with the soldier's
    /// recoil and camera; None without one.
    fn weapon<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        let v = self.get(py, |s| {
            s.movement.as_ref().and_then(|m| {
                m.weapon.as_ref().map(|w| {
                    let mut v = serde_json::to_value(w).unwrap();
                    v["recoil"] = serde_json::json!(m.recoil);
                    v["camera"] = serde_json::json!(m.camera);
                    v["camera_rows"] = serde_json::json!(m.camera_rows);
                    v["animation_ready"] = serde_json::json!(w.animation_ready());
                    v["template"] = serde_json::json!(w.template.name);
                    v
                })
            })
        });
        v.map(|v| to_py(py, &v)).transpose()
    }

    /// The game's input index the soldier's shots and recoil are seeded with
    /// (`getCurrentInputIndex`; a reference run's `input_index`).
    fn set_input_index(&self, py: Python, index: i32) {
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                m.input_index = index;
            }
        });
    }

    /// Whether the soldier has a player (`simulatePlayerUpdate`'s order: triggers, input, then
    /// its and its weapon's `handleUpdate`; its shots have an owner and a start) or is a
    /// console-made one (the default: what most reference runs follow).
    fn set_player(&self, py: Python, player: bool) {
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                m.has_player = player;
            }
        });
    }

    /// The soldier gets into that seat of `vehicle` at once (as `enterVehicle`).
    fn board(&self, py: Python, vehicle: usize, seat: usize) {
        let index = self.index;
        self.sim.borrow_mut(py).with(py, |s| s.enter(index, vehicle, seat));
    }

    /// Whether the soldier's player is a human (not a bot): its weapon's messages and shots say
    /// so (the clip's eject)
    fn set_human(&self, py: Python, human: bool) {
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                m.human = human;
            }
        });
    }

    /// As if a spawner at `spawner` had made it: left beyond `distance` with nobody
    /// near, `ttl` seconds, then `damage` a second.
    fn set_abandon(&self, py: Python, spawner: F3, distance: f32, ttl: f32, damage: f32) {
        self.get_mut(py, |v| {
            v.entry.abandon = Some(bytefield_sim::entry::Abandon { spawner: a3(spawner), distance, ttl, damage, timer: ttl });
        });
    }

    /// A vehicle's weapons: [{"name", "part", "mags", "fire", "reload",...}].
    fn weapons<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let w: Vec<serde_json::Value> = self.get(py, |v| {
            v.weapons
                .iter()
                .map(|(part, w)| {
                    let mut j = serde_json::to_value(w).unwrap_or_default();
                    j["name"] = w.template.name.clone().into();
                    j["part"] = (*part).into();
                    j
                })
                .collect()
        });
        to_py(py, &w)
    }

    /// Its seat and driver (a vehicle) or the vehicle it is in (a soldier):
    /// {"inside", "position", "driver", "occupants", "last_toggle", "seats": [{"entries", "exit",
    /// "exit_speed_mod", "ride"}]}.
    fn entry<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let e = self.get(py, |v| v.entry.clone());
        to_py(py, &e)
    }

    /// The countdown to the soldier's next collision sample (the server's first one
    /// comes from its own `rand`), its skeleton left as it is.
    fn set_sample_in(&self, py: Python, t: f32) {
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                m.sample_in = t;
            }
        });
    }

    /// The soldier's camera child's position off it (it moves halfway to the pose's each update).
    fn set_camera(&self, py: Python, pos: F3) {
        self.get_mut(py, |s| {
            if let Some(m) = s.movement.as_mut() {
                m.camera = a3(pos);
            }
        });
    }

    /// The material table as {id: (friction, elasticity, resistance)}.
    fn materials(&self, py: Python) -> std::collections::HashMap<u32, (f32, f32, f32)> {
        let sim = self.sim.borrow(py);
        let w = sim.world.bind(py).get();
        w.inner.materials().table.iter().map(|(k, m)| (*k, (m.friction, m.elasticity, m.resistance))).collect()
    }
}
/// A projectile in a `Sim`.
/// `Vehicle.set_part` / `Projectile.set_part`: overwrite a part's bundle angles and speeds, an
/// engine's rpm and load, a wing's regulator.
#[allow(clippy::too_many_arguments)]
fn set_craft_part(craft: &mut bytefield_sim::craft::Craft, name: &str, angle: Option<F3>, speed: Option<F3>, rpm: Option<f32>, load: Option<f32>, regulator: Option<f32>) -> PyResult<()> {
    use bytefield_sim::craft::Kind;
    let p = craft.parts.iter_mut().find(|p| p.name.eq_ignore_ascii_case(name)).ok_or_else(|| PyKeyError::new_err(name.to_string()))?;
    if let Some(b) = p.bundle.as_mut() {
        if let Some(a) = angle {
            b.angle = a3(a);
        }
        if let Some(s) = speed {
            b.speed = a3(s);
        }
    }
    match &mut p.kind {
        Kind::Engine(e) => {
            if let Some(r) = rpm {
                e.rpm = r;
            }
            if let Some(l) = load {
                e.load = l;
            }
        }
        Kind::Wing(w) => {
            if let Some(r) = regulator {
                w.regulator = r;
            }
        }
        Kind::Floating(_) | Kind::Weapon(_) | Kind::LandingGear(_) | Kind::Camera | Kind::Other => {}
    }
    Ok(())
}

#[pyclass]
struct Projectile {
    sim: Py<Sim>,
    index: usize,
}

#[pymethods]
impl Projectile {
    #[getter]
    fn index(&self) -> usize {
        self.index
    }

    #[getter]
    fn template(&self, py: Python) -> String {
        self.sim.borrow(py).projectiles[self.index].template.name.clone()
    }

    #[getter]
    fn alive(&self, py: Python) -> bool {
        self.sim.borrow(py).projectiles[self.index].alive()
    }

    /// pos, rows, vel, acc, underwater, age, ttl, end, impacts (and the rigid body, if any)
    fn state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let p = self.sim.borrow(py).projectiles[self.index].clone();
        let mut j = serde_json::to_value(&p).unwrap_or_default();
        j["template"] = p.template.name.clone().into();
        to_py(py, &j)
    }

    /// Its age (seconds lived: a reference run may start some time after it was made).
    fn set_age(&self, py: Python, age: f64) {
        self.sim.borrow_mut(py).projectiles[self.index].age = age;
    }

    /// A rigid projectile's part (as `Vehicle.set_part`).
    #[pyo3(signature = (name, angle = None, speed = None, rpm = None, load = None, regulator = None))]
    #[allow(clippy::too_many_arguments)]
    fn set_part(&self, py: Python, name: &str, angle: Option<F3>, speed: Option<F3>, rpm: Option<f32>, load: Option<f32>, regulator: Option<f32>) -> PyResult<()> {
        let mut s = self.sim.borrow_mut(py);
        let v = s.projectiles[self.index].rigid.as_mut().ok_or_else(|| PyKeyError::new_err("not a rigid projectile"))?;
        set_craft_part(&mut v.craft, name, angle, speed, rpm, load, regulator)
    }

    /// A rigid projectile's parts' accelerations of its last update (as `Vehicle.last_adds`).
    fn last_adds(&self, py: Python) -> Vec<(String, F3, F3)> {
        let s = self.sim.borrow(py);
        s.projectiles[self.index].rigid.as_ref().map_or(Vec::new(), |v| v.craft.last_adds.iter().map(|(n, p, a)| (n.clone(), f3(*p), f3(*a))).collect())
    }

    /// A rigid projectile's body (as `Vehicle.body`), or None for a point one.
    fn body<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyDict>>> {
        let b = self.sim.borrow(py).projectiles[self.index].rigid.as_ref().map(|v| v.craft.body.clone());
        b.map(|inner| RigidBody { inner }.state(py)).transpose()
    }

    /// Overwrite a rigid projectile's body (fields as `RigidBody.set`).
    #[pyo3(signature = (**fields))]
    fn set_body(&self, py: Python, fields: Option<&Bound<'_, PyDict>>) -> PyResult<()> {
        let mut s = self.sim.borrow_mut(py);
        let Some(v) = s.projectiles[self.index].rigid.as_mut() else { return Err(PyKeyError::new_err("not a rigid projectile")) };
        let mut rb = RigidBody { inner: v.craft.body.clone() };
        rb.set(fields)?;
        v.craft.body = rb.inner;
        Ok(())
    }

    /// Overwrite pos, vel, acc (point projectiles).
    #[pyo3(signature = (pos = None, vel = None, acc = None))]
    fn set(&self, py: Python, pos: Option<F3>, vel: Option<F3>, acc: Option<F3>) {
        let mut s = self.sim.borrow_mut(py);
        let p = &mut s.projectiles[self.index];
        if let Some(v) = pos {
            p.pos = a3(v);
        }
        if let Some(v) = vel {
            p.vel = a3(v);
        }
        if let Some(v) = acc {
            p.acc = a3(v);
        }
    }
}

#[pymodule]
fn bytefield(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<World>()?;
    m.add_class::<Sim>()?;
    m.add_class::<RigidBody>()?;
    m.add_class::<Vehicle>()?;
    m.add_class::<Projectile>()?;
    m.add("GRAVITY", bytefield_sim::GRAVITY)?;
    Ok(())
}
