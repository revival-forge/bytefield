//! Effects: smoke, fire, flashes. An `EffectBundle` holds `Emitter`s; each sends out
//! particles of its `template`: a `SpriteParticle` (a textured square facing the camera) or a
//! `Particle` (a mesh). Read from the server, which compiles them:
//! - a value is a distribution, `CRD_NONE/a` (a), `CRD_UNIFORM/a/b` (between), `CRD_EXPONENTIAL/a`
//!   (−a·ln u), `CRD_NORMAL/a/b` (a + b·N); a third number 1 gives it a random sign;
//! - an emitter lives `timeToLive` after its `delay` and sends `intensity` particles a second
//!   (with `startProbability` of starting at all), each at `relativePositionIn{Right,Up,Dof}` of
//!   the emitter and with `positionalSpeedIn…` (plus the emitter's speed × `emitterSpeedScale`
//!   with `addEmitterSpeed`);
//! - a particle lives `timeToLive`; its size, gravity and alpha follow `…OverTime` curves
//!   (`t/v|t/v`, t in % of its life), its colour `colorRGBAOverTime` (`t/r/g/b/a`, 0..255);
//!   `destBlendMode BMOne` adds it to what is behind;
//! - a sprite with `numAnimationFrames` n > 1 takes one cell of its texture, a grid of c×c (c = ⌊√n⌋,
//!   one more if n is not a multiple of it): its phase starts at `initAnimationFrame` and grows by
//!   `animationSpeed` × `animationSpeedOverTime` / n a second, and the cell is ⌊c·phase⌋ mod c²
//!   (the client's own arithmetic, c and not n: `Frames`).
//! Not yet: `drag` over time, water surfaces, the server's `rand`.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bytefield_con::templates::{Template, TemplateDb};
use crate::assets::Cache;

/// A sprite's material (`shaders/particle.wesl`): unlit, and soft where it nears what is behind it.
pub type ParticleMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, Soft>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone)]
pub struct Soft {
    /// x: the fade's depth over the sprite's size (0: off, the setting `soft_particles`)
    #[uniform(100)]
    pub soft: Vec4,
}

impl bevy::pbr::MaterialExtension for Soft {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/particle.wesl".into()
    }
}

/// how deep a sprite fades into what is behind it, over its size
const SOFT: f32 = 0.5;

/// y: 1 for an additive sprite (`destBlendMode BMOne`)
fn soft(on: bool, additive: bool) -> Soft {
    Soft { soft: Vec4::new(if on { SOFT } else { 0.0 }, if additive { 1.0 } else { 0.0 }, 0.0, 0.0) }
}

/// A sprite's half side over its `size`: measured against the original's veil of smoke in Husky, the
/// corners at `size` from the centre. 0.5 left it in blotches.
// TODO: read it from the client's sprite vertices (only one effect measured)
const SPRITE_HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// A value drawn anew each time.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dist {
    kind: u8,
    a: f32,
    b: f32,
    signed: bool,
}

impl Dist {
    pub fn constant(a: f32) -> Dist {
        Dist { a, ..Default::default() }
    }

    /// `CRD_UNIFORM/0/180/1`
    pub fn parse(s: &str) -> Option<Dist> {
        let p: Vec<&str> = s.split('/').collect();
        let num = |i: usize| p.get(i).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
        let kind = match p.first()?.trim().to_ascii_uppercase().as_str() {
            "CRD_NONE" => 0,
            "CRD_UNIFORM" => 1,
            "CRD_EXPONENTIAL" => 2,
            "CRD_NORMAL" => 3,
            _ => return None,
        };
        Some(Dist { kind, a: num(1), b: num(2), signed: num(3) != 0.0 })
    }

    pub fn sample(&self, rng: &mut Rng) -> f32 {
        let v = match self.kind {
            1 => self.a + (self.b - self.a) * rng.unit(),
            2 => -self.a * rng.unit().max(1e-6).ln(),
            3 => self.a + self.b * rng.normal(),
            _ => self.a,
        };
        if self.signed && rng.unit() < 0.5 { -v } else { v }
    }
}

/// A small generator of our own (the server's `rand` has no seed we know).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn normal(&mut self) -> f32 {
        // Box–Muller
        let (u, v) = (self.unit().max(1e-6), self.unit());
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }
}

/// `t/v|t/v|…` (t in % of the life), or with several values per point (a colour).
#[derive(Debug, Clone, Default)]
pub struct Curve<const N: usize>(Vec<(f32, [f32; N])>);

impl<const N: usize> Curve<N> {
    pub fn parse(s: &str) -> Option<Curve<N>> {
        let mut out = Vec::new();
        for point in s.split('|') {
            let v: Vec<f32> = point.split('/').filter_map(|x| x.trim().parse().ok()).collect();
            if v.len() < N + 1 {
                return None;
            }
            out.push((v[0] / 100.0, std::array::from_fn(|k| v[k + 1])));
        }
        (!out.is_empty()).then_some(Curve(out))
    }

    /// At `t` (0..1 of the life): between its points, the ends beyond them.
    pub fn at(&self, t: f32) -> [f32; N] {
        let p = &self.0;
        if t <= p[0].0 {
            return p[0].1;
        }
        for w in p.windows(2) {
            if t <= w[1].0 {
                let k = if w[1].0 > w[0].0 { (t - w[0].0) / (w[1].0 - w[0].0) } else { 1.0 };
                return std::array::from_fn(|i| w[0].1[i] + (w[1].1[i] - w[0].1[i]) * k);
            }
        }
        p[p.len() - 1].1
    }
}

/// What a particle is (`Particle` or `SpriteParticle`).
#[derive(Debug, Clone, Default)]
pub struct ParticleKind {
    pub ttl: Dist,
    pub size: Dist,
    pub size_over: Option<Curve<1>>,
    pub gravity: Dist,
    pub gravity_over: Option<Curve<1>>,
    pub drag: Dist,
    pub color: Option<Curve<4>>,
    pub alpha_over: Option<Curve<1>>,
    pub rotation_speed: Dist,
    pub init_rotation: Dist,
    pub additive: bool,
    /// a sprite's texture, or a mesh (`geometry`: kind and file)
    pub texture: Option<String>,
    pub mesh: Option<(String, String)>,
    /// `GeometryTemplate.scale` of that mesh
    pub mesh_scale: f32,
    pub frames: Option<Frames>,
}

/// A sprite's texture animation: `numAnimationFrames` (more than one), `initAnimationFrame`,
/// `animationSpeed`, `animationSpeedOverTime`.
#[derive(Debug, Clone)]
pub struct Frames {
    pub count: u32,
    pub init: Dist,
    pub speed: Dist,
    pub speed_over: Option<Curve<1>>,
}

impl Frames {
    /// The cells a side: the client takes ⌊√n⌋ and one more when n is not a multiple of it.
    pub fn side(&self) -> u32 {
        let c = (self.count as f32).sqrt() as u32;
        if c == 0 { 1 } else if self.count % c != 0 { c + 1 } else { c }
    }

    /// How much the phase grows in `dt` at `t` (0..1 of the life) with this particle's speed.
    pub fn step(&self, speed: f32, t: f32, dt: f32) -> f32 {
        speed * dt * self.speed_over.as_ref().map_or(1.0, |c| c.at(t)[0]) / self.count as f32
    }

    /// The cell of a phase, as the UV of its top left corner and its size: the client truncates
    /// side × phase (not count × phase) and wraps it at side².
    pub fn cell(&self, phase: f32) -> ([f32; 2], f32) {
        let c = self.side();
        let k = ((c as f32 * phase).max(0.0) as u64 % (c * c) as u64) as u32;
        let size = 1.0 / c as f32;
        ([(k % c) as f32 * size, (k / c) as f32 * size], size)
    }
}

/// One emitter of an effect.
#[derive(Debug, Clone, Default)]
pub struct EmitterKind {
    pub particle: Arc<ParticleKind>,
    pub ttl: Dist,
    pub intensity: Dist,
    pub delay: Dist,
    pub start_probability: f32,
    /// right, up, ahead (the emitter's x, y, z)
    pub position: [Dist; 3],
    pub speed: [Dist; 3],
    pub emitter_speed: f32,
    /// `showInFirstPerson` / `showInThirdPerson` (None: both)
    pub first_person: Option<bool>,
    /// `looping`: when its life ends it starts again
    pub looping: bool,
    /// where it is in its bundle (`setPosition`)
    pub at: [f32; 3],
}

/// An `EffectBundle`: its emitters.
#[derive(Debug, Clone, Default)]
pub struct Effect {
    pub emitters: Vec<EmitterKind>,
}

fn dist(t: &Template, k: &str) -> Option<Dist> {
    Dist::parse(t.get(k)?.first()?)
}

fn particle(db: &TemplateDb, name: &str) -> Option<ParticleKind> {
    let t = db.object_template(name)?;
    let curve = |k: &str| t.get(k).and_then(|a| a.first()).and_then(|v| Curve::<1>::parse(v));
    let mesh = t.get("geometry").and_then(|a| a.first()).and_then(|g| {
        let gt = db.get("geometrytemplate", g)?;
        Some((gt.kind.to_ascii_lowercase(), gt.get("file")?.first()?.replace('\\', "/")))
    });
    Some(ParticleKind {
        ttl: dist(t, "timetolive").unwrap_or(Dist::constant(1.0)),
        size: dist(t, "size").unwrap_or(Dist::constant(1.0)),
        size_over: curve("sizeovertime"),
        gravity: dist(t, "gravitymodifier").unwrap_or(Dist::constant(1.0)),
        gravity_over: curve("gravitymodifierovertime"),
        drag: dist(t, "drag").unwrap_or_default(),
        color: t.get("colorrgbaovertime").and_then(|a| a.first()).and_then(|v| Curve::<4>::parse(v)),
        alpha_over: curve("alphaovertime"),
        rotation_speed: dist(t, "rotationspeed").unwrap_or_default(),
        init_rotation: dist(t, "initrotation").unwrap_or_default(),
        additive: t.get("destblendmode").and_then(|a| a.first()).is_some_and(|v| v.eq_ignore_ascii_case("bmone")),
        texture: t.get("texture").and_then(|a| a.first()).map(|s| s.replace('\\', "/")),
        mesh_scale: t.get("geometry").and_then(|a| a.first()).and_then(|g| db.get("geometrytemplate", g)?.get("scale")?.first()?.trim().parse::<f32>().ok()).filter(|k| *k > 0.0).unwrap_or(1.0),
        mesh,
        frames: t.get("numanimationframes").and_then(|a| a.first()).and_then(|v| v.trim().parse::<u32>().ok()).filter(|n| *n > 1).map(|count| Frames {
            count,
            init: dist(t, "initanimationframe").unwrap_or_default(),
            speed: dist(t, "animationspeed").unwrap_or_default(),
            speed_over: curve("animationspeedovertime"),
        }),
    })
}

/// An effect template (`EffectBundle`), if it has emitters with particles (its own and those
/// of the effects it holds).
pub fn load(db: &TemplateDb, name: &str) -> Option<Effect> {
    let mut emitters = Vec::new();
    collect(db, name, [0.0; 3], &mut emitters, 0);
    (!emitters.is_empty()).then_some(Effect { emitters })
}

fn collect(db: &TemplateDb, name: &str, offset: [f32; 3], emitters: &mut Vec<EmitterKind>, depth: usize) {
    let Some(b) = db.object_template(name) else { return };
    if depth > 4 {
        return;
    }
    // a bundle inside: its emitters, moved by where it is
    let mut inner: Vec<(String, [f32; 3])> = Vec::new();
    let mut last_inner = false;
    let mut last: Option<usize> = None;
    let flag = |t: &Template, k: &str| t.get(k).and_then(|a| a.first()).map(|v| v.trim() == "1");
    for p in &b.props {
        let v = || p.args.first().and_then(|a| bytefield_world::parse_vec(a)).filter(|v| v.len() == 3);
        match p.name.as_str() {
            "addtemplate" => {
                last = None;
                last_inner = false;
                let Some(e) = p.args.first().and_then(|n| db.object_template(n)) else { continue };
                if e.kind.eq_ignore_ascii_case("effectbundle") {
                    inner.push((e.name.clone(), offset));
                    last_inner = true;
                    continue;
                }
                if !e.kind.eq_ignore_ascii_case("emitter") {
                    continue;
                }
                let Some(pk) = e.get("template").and_then(|a| a.first()).and_then(|n| particle(db, n)) else { continue };
                let first_person = match (flag(e, "showinfirstperson"), flag(e, "showinthirdperson")) {
                    (Some(true), _) => Some(true),
                    (_, Some(true)) => Some(false),
                    _ => None,
                };
                let pos = |k: &str| dist(e, k).unwrap_or_default();
                emitters.push(EmitterKind {
                    particle: Arc::new(pk),
                    // `EmitterTemplate`'s constructor: CRD_NONE/10 (4 or 5 emitters a mod leave it out)
                    ttl: dist(e, "timetolive").unwrap_or(Dist::constant(10.0)),
                    intensity: dist(e, "intensity").unwrap_or(Dist::constant(1.0)),
                    delay: dist(e, "delay").unwrap_or_default(),
                    start_probability: e.get("startprobability").and_then(|a| a.first()).and_then(|v| v.trim().parse().ok()).unwrap_or(1.0),
                    position: [pos("relativepositioninright"), pos("relativepositioninup"), pos("relativepositionindof")],
                    speed: [pos("positionalspeedinright"), pos("positionalspeedinup"), pos("positionalspeedindof")],
                    emitter_speed: if flag(e, "addemitterspeed") == Some(true) { e.get("emitterspeedscale").and_then(|a| a.first()).and_then(|v| v.trim().parse().ok()).unwrap_or(1.0) } else { 0.0 },
                    first_person,
                    looping: flag(e, "looping") == Some(true),
                    at: offset,
                });
                last = Some(emitters.len() - 1);
            }
            "setposition" => {
                let Some(v) = v() else { continue };
                let at = [offset[0] + v[0] as f32, offset[1] + v[1] as f32, offset[2] + v[2] as f32];
                if let Some(i) = last {
                    emitters[i].at = at;
                } else if last_inner && let Some(x) = inner.last_mut() {
                    x.1 = at;
                }
            }
            _ => {}
        }
    }
    for (n, at) in inner {
        collect(db, &n, at, emitters, depth + 1);
    }
}

/// An emitter at work.
#[derive(Debug, Clone)]
struct Running {
    kind: usize,
    /// its life (s; negative: for ever), the delay before it, the time it has run, the
    /// particles owed
    ttl: f32,
    delay: f32,
    time: f32,
    owed: f32,
}

/// A particle in flight (Bevy's frame).
#[derive(Debug, Clone)]
struct Flying {
    kind: Arc<ParticleKind>,
    pos: Vec3,
    vel: Vec3,
    age: f32,
    ttl: f32,
    size: f32,
    gravity: f32,
    drag: f32,
    rot: f32,
    spin: f32,
    /// a mesh particle's turn: its effect's (its own spin about the effect's ahead)
    turn: Quat,
    /// a sprite's texture animation: its phase and speed (`Frames`)
    phase: f32,
    frame_speed: f32,
    layer: usize,
    /// a mesh particle's entities
    entities: Vec<Entity>,
}

/// What an effect follows: a vehicle (by index in `sim.vehicles`), with an offset in its frame
/// (Refractor's); a projectile in flight (its template and where it left from: they have no
/// lasting index).
#[derive(Debug, Clone)]
pub enum Follow {
    Vehicle(usize, [f32; 3]),
    Projectile(String, [u32; 3]),
}

/// One effect playing: where (Bevy's frame), its emitters, on which layer (the first person's
/// or the world's), what it follows.
struct Playing {
    id: u64,
    effect: Arc<Effect>,
    at: Transform,
    vel: Vec3,
    running: Vec<Running>,
    layer: usize,
    follow: Option<Follow>,
}

/// The effects playing and their particles; the textures' materials, the sprites' meshes (one
/// per texture, blending and layer, made again every frame).
#[derive(Resource, Default)]
pub struct Effects {
    templates: HashMap<String, Option<Arc<Effect>>>,
    playing: Vec<Playing>,
    flying: Vec<Flying>,
    rng: Option<Rng>,
    sprites: HashMap<(String, bool, usize), (Entity, Handle<Mesh>, bool, Handle<ParticleMaterial>)>,
    /// the setting the sprites' materials have
    soft: bool,
    next_id: u64,
    /// the effects started since the sound last looked (their bundles' scripts play there):
    /// (template, where, the vehicle it follows, id)
    pub started: Vec<(String, Vec3, Option<usize>, u64)>,
    /// and those stopped
    pub stopped: Vec<u64>,
    /// the level's own (smoke over the ruins) are going ([`level_effects`])
    placed: bool,
}

/// Where the world's gravity pulls (Bevy's frame; the server's −14.73).
const GRAVITY: f32 = -14.73;

impl Effects {
    /// Starts the effect `name` at `at` (moving at `vel`), on the first person's layer or the
    /// world's: its emitters for that view only.
    pub fn start(&mut self, db: &TemplateDb, name: &str, at: Transform, vel: Vec3, first_person: bool, layer: usize) -> bool {
        self.play(db, name, at, vel, first_person, layer, None).is_some()
    }

    /// Starts `name` on what it follows, until stopped (or that is gone).
    pub fn start_on(&mut self, db: &TemplateDb, name: &str, follow: Follow) -> Option<u64> {
        self.play(db, name, Transform::IDENTITY, Vec3::ZERO, false, 0, Some(follow))
    }

    /// Its emitters stop (its particles live on).
    pub fn stop(&mut self, id: u64) {
        self.playing.retain(|p| p.id != id);
        self.stopped.push(id);
    }

    #[allow(clippy::too_many_arguments)]
    fn play(&mut self, db: &TemplateDb, name: &str, at: Transform, vel: Vec3, first_person: bool, layer: usize, follow: Option<Follow>) -> Option<u64> {
        let effect = self.templates.entry(name.to_ascii_lowercase()).or_insert_with(|| load(db, name).map(Arc::new)).clone()?;
        let rng = self.rng.get_or_insert(Rng(0x9e37_79b9_7f4a_7c15));
        let mut running = Vec::new();
        for (k, e) in effect.emitters.iter().enumerate() {
            if e.first_person.is_some_and(|f| f != first_person) || rng.unit() > e.start_probability {
                continue;
            }
            running.push(Running { kind: k, ttl: e.ttl.sample(rng), delay: e.delay.sample(rng).max(0.0), time: 0.0, owed: 1.0 });
        }
        self.next_id += 1;
        let on = match follow {
            Some(Follow::Vehicle(i, _)) => Some(i),
            _ => None,
        };
        if !first_person {
            self.started.push((name.to_string(), at.translation, on, self.next_id));
        }
        self.playing.push(Playing { id: self.next_id, effect, at, vel, running, layer, follow });
        Some(self.next_id)
    }
}

/// The level in the viewer (`--fly`, no round): its placed effects run there too, as the original's before the spawn
/// (the smoke over Husky's ruins).
/// Where [`update`] runs (in a round and in the viewer), for others to go after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectsUpdate;

/// The viewer's camera, for the sprites to face it.
#[derive(Component)]
pub struct ViewerCamera;

#[derive(Resource)]
pub struct ViewerWorld(pub &'static bytefield_sim::World);

/// The effects the level places as objects: started once, for the whole round.
// TODO: the emitters' `lodDistance` (600 m on these): they all run wherever the camera is
pub fn level_effects(game: Option<Res<crate::play::Game>>, viewer: Option<Res<ViewerWorld>>, mut fx: ResMut<Effects>) {
    if fx.placed {
        return;
    }
    let Some(world) = game.as_ref().map(|g| &*g.session.sim.world).or(viewer.as_ref().map(|v| v.0)) else { return };
    fx.placed = true;
    for (name, at, i) in bytefield_world::scene::expand_level(&world.db).effects {
        if !world.is_mobile(&world.db.instances[i].template) {
            fx.start(&world.db, &name, crate::assets::to_transform(&at), Vec3::ZERO, false, 0);
        }
    }
    // BYTEFIELD_EFFECT=e_MustangFire@x,y,z[;…]: effects of our choosing at those points (the game's frame), to look at
    // one without the object that carries it
    for one in std::env::var("BYTEFIELD_EFFECT").unwrap_or_default().split(';').filter(|s| !s.is_empty()) {
        let Some((name, at)) = one.split_once('@') else { continue };
        let p: Vec<f32> = at.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if p.len() == 3 && !fx.start(&world.db, name, Transform::from_translation(crate::assets::to_bevy([p[0], p[1], p[2]])), Vec3::ZERO, false, 0) {
            eprintln!("BYTEFIELD_EFFECT: no effect {name}");
        }
    }
}

/// Every frame: the emitters send particles out, the particles move, fade and go; the sprites
/// are drawn facing the camera.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut fx: ResMut<Effects>,
    time: Res<Time>,
    mut commands: Commands,
    camera: Query<&GlobalTransform, Or<(With<crate::play::PlayCamera>, With<ViewerCamera>)>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut particle_materials: ResMut<Assets<ParticleMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<Cache>,
    game: Option<Res<crate::play::Game>>,
    viewer: Option<Res<ViewerWorld>>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(world) = game.as_ref().map(|g| &*g.session.sim.world).or(viewer.as_ref().map(|v| v.0)) else { return };
    let vfs = &world.vfs;
    // with a script, a tick a frame (as the simulation)
    let dt = if game.as_ref().is_some_and(|g| g.scripted()) { crate::play::TICK as f32 } else { time.delta_secs().min(0.1) };
    let fx = &mut *fx;
    let rng = fx.rng.get_or_insert(Rng(0x9e37_79b9_7f4a_7c15));
    // the emitters (those on something where it is now; gone with it; in the viewer, nothing moves)
    let sim = game.as_ref().map(|g| &g.session.sim);
    fx.playing.retain_mut(|p| {
        let Some(sim) = sim else { return p.follow.is_none() };
        let (rows, pos, vel) = match &p.follow {
            None => return true,
            Some(Follow::Vehicle(v, off)) => {
                let Some(v) = sim.vehicles.get(*v).filter(|v| !v.removed) else { return false };
                let b = &v.craft.body;
                (b.rows, std::array::from_fn(|k| b.pos[k] + off[0] * b.rows[0][k] + off[1] * b.rows[1][k] + off[2] * b.rows[2][k]), b.vel)
            }
            Some(Follow::Projectile(name, start)) => {
                let Some(q) = sim.projectiles.iter().find(|q| q.alive() && q.template.name == *name && q.start.map(f32::to_bits) == *start) else { return false };
                (q.rows, q.pos, q.vel)
            }
        };
        p.at = crate::assets::to_transform(&crate::assets::frame_matrix(&rows, pos));
        p.vel = crate::assets::to_bevy(vel);
        true
    });
    let mut born = Vec::new();
    fx.playing.retain_mut(|p| {
        for r in p.running.iter_mut() {
            let e = &p.effect.emitters[r.kind];
            if r.delay > 0.0 {
                r.delay -= dt;
                continue;
            }
            r.time += dt;
            r.owed += e.intensity.sample(rng).max(0.0) * dt;
            while r.owed >= 1.0 {
                r.owed -= 1.0;
                let at = p.at.transform_point(Vec3::new(e.at[0], e.at[1], -e.at[2]));
                let (right, up, ahead) = (p.at.rotation * Vec3::X, p.at.rotation * Vec3::Y, p.at.rotation * Vec3::NEG_Z);
                let off = |d: &[Dist; 3], rng: &mut Rng| right * d[0].sample(rng) + up * d[1].sample(rng) + ahead * d[2].sample(rng);
                let k = &e.particle;
                born.push(Flying {
                    kind: k.clone(),
                    pos: at + off(&e.position, rng),
                    vel: off(&e.speed, rng) + p.vel * e.emitter_speed,
                    age: 0.0,
                    ttl: k.ttl.sample(rng).max(0.01),
                    size: k.size.sample(rng),
                    gravity: k.gravity.sample(rng),
                    drag: k.drag.sample(rng).max(0.0),
                    rot: k.init_rotation.sample(rng).to_radians(),
                    spin: k.rotation_speed.sample(rng).to_radians(),
                    turn: p.at.rotation,
                    phase: k.frames.as_ref().map_or(0.0, |a| a.init.sample(rng)),
                    frame_speed: k.frames.as_ref().map_or(0.0, |a| a.speed.sample(rng)),
                    layer: p.layer,
                    entities: Vec::new(),
                });
            }
        }
        for r in p.running.iter_mut() {
            if p.effect.emitters[r.kind].looping && r.ttl > 0.0 && r.time >= r.ttl {
                r.time -= r.ttl;
            }
        }
        p.running.retain(|r| r.delay > 0.0 || r.ttl < 0.0 || r.time < r.ttl);
        !p.running.is_empty()
    });
    // mesh particles get their entities
    for mut f in born {
        if let Some((kind, file)) = f.kind.mesh.clone() {
            for (mesh, mat) in cache.get(vfs, &kind, &file, &mut meshes, &mut materials, &mut images) {
                let t = Transform::from_translation(f.pos).with_rotation(f.turn * Quat::from_rotation_z(f.rot)).with_scale(Vec3::splat(f.size.max(0.001) * f.kind.mesh_scale));
                f.entities.push(commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), t, RenderLayers::layer(f.layer))).id());
            }
        }
        fx.flying.push(f);
    }
    // the particles move and age
    fx.flying.retain_mut(|f| {
        f.age += dt;
        if f.age >= f.ttl {
            for e in &f.entities {
                commands.entity(*e).despawn();
            }
            return false;
        }
        let t = f.age / f.ttl;
        let g = f.gravity * f.kind.gravity_over.as_ref().map_or(1.0, |c| c.at(t)[0]);
        f.vel.y += GRAVITY * g * dt;
        f.vel *= (1.0 - f.drag * dt).max(0.0);
        f.pos += f.vel * dt;
        f.rot += f.spin * dt;
        // (the client moves it only while the sprite is drawn; here, always)
        if let Some(a) = &f.kind.frames {
            f.phase += a.step(f.frame_speed, t, dt);
        }
        let size = f.size * f.kind.size_over.as_ref().map_or(1.0, |c| c.at(t)[0]);
        for e in &f.entities {
            if let Ok(mut tr) = transforms.get_mut(*e) {
                *tr = Transform::from_translation(f.pos).with_rotation(f.turn * Quat::from_rotation_z(f.rot)).with_scale(Vec3::splat(size.max(0.001) * f.kind.mesh_scale));
            }
        }
        true
    });
    // the sprites: one mesh for each texture, blending and layer, facing the camera
    let Ok(cam) = camera.single() else { return };
    let (cam_right, cam_up) = (cam.right().as_vec3(), cam.up().as_vec3());
    let on = crate::assets::graphics().soft_particles;
    if fx.soft != on {
        fx.soft = on;
        for ((_, additive, _), (.., m)) in fx.sprites.iter() {
            if let Some(mut m) = particle_materials.get_mut(m) {
                m.extension = soft(on, *additive);
            }
        }
    }
    // (positions, colours, uvs, sizes)
    let mut quads: HashMap<(String, bool, usize), (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<[f32; 2]>, Vec<[f32; 2]>)> = HashMap::new();
    // (the oldest on top: smoke takes the colour of its middle age, Husky)
    for f in fx.flying.iter().rev().filter(|f| f.kind.mesh.is_none()) {
        let Some(tex) = &f.kind.texture else { continue };
        let t = f.age / f.ttl;
        let size = f.size * f.kind.size_over.as_ref().map_or(1.0, |c| c.at(t)[0]);
        let mut c = f.kind.color.as_ref().map_or([255.0; 4], |c| c.at(t));
        if let Some(a) = &f.kind.alpha_over {
            c[3] = a.at(t)[0];
        }
        // (the template's colour is in the screen's space, as in D3D; Bevy takes a vertex's as linear: 167 came out as 212)
        let rgb = LinearRgba::from(Srgba::rgb(c[0] / 255.0, c[1] / 255.0, c[2] / 255.0));
        let col = [rgb.red, rgb.green, rgb.blue, (c[3] / 255.0).clamp(0.0, 1.0)];
        let (s, co) = f.rot.sin_cos();
        let (r, u) = ((cam_right * co + cam_up * s) * size * SPRITE_HALF, (cam_up * co - cam_right * s) * size * SPRITE_HALF);
        let q = quads.entry((tex.to_ascii_lowercase(), f.kind.additive, f.layer)).or_default();
        let (o, k) = f.kind.frames.as_ref().map_or(([0.0; 2], 1.0), |a| a.cell(f.phase));
        for (corner, uv) in [(-r - u, [0.0, 1.0]), (r - u, [1.0, 1.0]), (r + u, [1.0, 0.0]), (-r + u, [0.0, 0.0])] {
            q.0.push((f.pos + corner).to_array());
            q.1.push(col);
            q.2.push([o[0] + uv[0] * k, o[1] + uv[1] * k]);
            q.3.push([size, 0.0]);
        }
    }
    let quad_mesh = |pos: Vec<[f32; 3]>, col: Vec<[f32; 4]>, uv: Vec<[f32; 2]>, sizes: Vec<[f32; 2]>| {
        let n = pos.len() as u32 / 4;
        let idx: Vec<u32> = (0..n).flat_map(|i| [4 * i, 4 * i + 1, 4 * i + 2, 4 * i, 4 * i + 2, 4 * i + 3]).collect();
        let normals = vec![[0.0, 1.0, 0.0]; pos.len()];
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, sizes)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
            .with_inserted_indices(Indices::U32(idx))
    };
    for (key, (e, _, shown, _)) in fx.sprites.iter_mut() {
        if !quads.contains_key(key) && *shown {
            // nothing of it this frame
            commands.entity(*e).insert(Visibility::Hidden);
            *shown = false;
        }
    }
    for (key, (pos, col, uv, sizes)) in quads {
        let mesh = quad_mesh(pos, col, uv, sizes);
        match fx.sprites.get_mut(&key) {
            Some((e, h, shown, _)) => {
                if let Some(mut m) = meshes.get_mut(h) {
                    *m = mesh;
                }
                if !*shown {
                    commands.entity(*e).insert(Visibility::Inherited);
                    *shown = true;
                }
            }
            None => {
                let texture = cache.texture(vfs, &key.0, &mut images);
                let base = StandardMaterial {
                    base_color_texture: texture,
                    // additive in the original (`BMOne`): premultiplied, with `particle.wesl` giving colour × alpha and
                    // alpha 0, so it adds to what is behind (Bevy's [`AlphaMode::Add`] drew nothing on the first person's
                    // camera). Blended by alpha, DC's magenta HEDP flash came out a solid pink veil instead of a pale glow
                    alpha_mode: if key.1 { AlphaMode::Premultiplied } else { AlphaMode::Blend },
                    unlit: true,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                };
                let mat = particle_materials.add(ParticleMaterial { base, extension: soft(fx.soft, key.1) });
                let h = meshes.add(mesh);
                let e = commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, NoFrustumCulling, RenderLayers::layer(key.2))).id();
                fx.sprites.insert(key, (e, h, true, mat));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributions_and_curves() {
        let mut r = Rng(1);
        assert_eq!(Dist::parse("CRD_NONE/0.07/0/0").unwrap().sample(&mut r), 0.07);
        let u = Dist::parse("CRD_UNIFORM/2/4/0").unwrap();
        assert!((0..100).all(|_| (2.0..=4.0).contains(&u.sample(&mut r))));
        let s = Dist::parse("CRD_UNIFORM/0/180/1").unwrap();
        assert!((0..100).any(|_| s.sample(&mut r) < 0.0));
        let c = Curve::<4>::parse("0/255/255/128/255|77/255/157/29/172|100/255/128/0/65").unwrap();
        assert_eq!(c.at(0.0), [255.0, 255.0, 128.0, 255.0]);
        assert_eq!(c.at(1.0), [255.0, 128.0, 0.0, 65.0]);
        let h = c.at(0.385);
        assert!((h[1] - 206.0).abs() < 1.0, "{h:?}");
    }

    #[test]
    fn animation_frames() {
        let f = |n| Frames { count: n, init: Dist::default(), speed: Dist::default(), speed_over: Curve::parse("0/1|100/0.2") };
        assert_eq!((f(16).side(), f(4).side(), f(5).side(), f(8).side()), (4, 2, 3, 2));
        // `initAnimationFrame 1` on 16 frames: the client's phase 1 is the second row, not the second cell
        assert_eq!(f(16).cell(1.0), ([0.0, 0.25], 0.25));
        assert_eq!(f(16).cell(0.5), ([0.5, 0.0], 0.25));
        assert_eq!(f(16).cell(8.0), ([0.0, 0.0], 0.25));
        assert_eq!(f(4).cell(1.5), ([0.5, 0.5], 0.5));
        // at speed 95 and half its life: 95 · 0.6 / 16 a second
        assert!((f(16).step(95.0, 0.5, 1.0) - 95.0 * 0.6 / 16.0).abs() < 1e-5);
    }

    #[test]
    fn the_muzzle_flash_loads() {
        let game = bytefield_testdata::require!(Base);
        let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let e = load(&w.db, "e_MuzzGun").expect("e_MuzzGun");
        assert_eq!(e.emitters.len(), 3);
        assert_eq!(e.emitters[1].first_person, Some(true));
        assert!(e.emitters[2].particle.additive && e.emitters[2].particle.texture.as_deref() == Some("e_fire4"));
        // a tank's gun
        assert!(load(&w.db, "e_MuzzPanz").is_some());
        // a bundle of bundles
        let c = load(&w.db, "BazookaCascadesStone").expect("BazookaCascadesStone");
        let b = load(&w.db, "e_ExplBazooka").expect("e_ExplBazooka");
        assert!(c.emitters.len() > b.emitters.len());
        for e in &b.emitters {
            let p = &e.particle;
            eprintln!("ttl {:?} intensity {:?} | particle ttl {:?} size {:?} texture {:?} mesh {:?} pos {:?} speed {:?}", e.ttl, e.intensity, p.ttl, p.size, p.texture, p.mesh, e.position, e.speed);
        }
    }
}
