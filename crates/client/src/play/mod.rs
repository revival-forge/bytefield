//! Playing: the simulation (`bytefield-sim`) at 30 Hz, the player's keys and mouse as its
//! `c_PI*` axes, what moves drawn between two ticks, the camera.
//!
//! The simulation is the bench's, with nothing of Bevy in it: the client gathers the input,
//! steps it in whole ticks of 1/30 s as the server does, and draws each object at
//! `alpha` of the way from its previous tick to its last one (`Visual`).
//!
//! The keys are the original's (`Mods/bf1942/Settings/` defaults): W/S throttle, A/D strafe (or
//! steer), mouse look, left button fire, right button alt fire (zoom), space jump, E get in/out,
//! Ctrl crouch, Z lie, R reload, 1..9 weapon or seat, arrows pitch and roll (planes); C changes
//! the camera (first/third person), Esc frees the mouse. A `--script` stands for the keys: tick
//! ranges with what is held in them, for checks without a person in front of the screen.

use std::time::Instant;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use bytefield_sim::craft::Kind;
use bytefield_sim::replica::{LocalSession, Session};
use bytefield_sim::soldier::axis;
use bytefield_server::udp::UdpClient;
use bytefield_sim::{Sim, World};
use bytefield_world::scene::Mat4;

use crate::assets::{self, Cache};
use crate::{level, model, skin, weapon};
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};

mod camera;
mod input;
mod seats;
mod setup;
mod step;
mod sync;
mod visuals;
mod weapons;
#[cfg(test)]
mod tests;

pub use camera::*;
pub use setup::*;
pub(crate) use input::*;
pub(crate) use seats::*;
use step::*;
use sync::*;
use visuals::*;
use weapons::*;
#[cfg(test)]
pub(crate) use tests::with_data;

/// The layer of what the player sees of itself (arms, weapon): drawn by a second camera over
/// the world's, its depth its own (the arms do not go into walls), its near plane close and
/// its field of view the soldier's `set1pFov` (the server's `setFirstPersonFov` gives it to
/// each first-person part as an `IViewModifier`; taken as the vertical angle, as the world's:
/// so the No4's sight comes up from below the cross as in the original).
const FIRST_PERSON_LAYER: usize = 1;
/// A layer nothing is on.
const NOTHING_LAYER: usize = 2;
/// `renderer.fieldOfView 1` (`Settings/VideoDefault.con`): taken as the vertical angle, radians.
pub const WORLD_FOV: f32 = 1.0;
/// The same in degrees (the menu's)
pub const WORLD_FOV_DEG: f32 = 57.29578;

/// The simulation's step.
pub const TICK: f64 = 1.0 / 30.0;
/// At most this many ticks per frame (a slow frame does not snowball).
const MAX_TICKS: u32 = 5;
/// Seconds a script waits before its first tick.
pub const WARM_UP: f32 = 3.0;
/// Mouse units (`c_PIMouseLookX/Y` per tick: 3° of turn, 2.5° of aim) per pixel, at `--sens 1` and the soldier's
/// sensitivity by default ([`crate::controls::MOUSE_NEUTRAL`]). Measured against the original:
/// the same mouse gives a whole turn in the same distance, and half of it at 0.5 (within 3%). Only the turn:
/// the aim up and down is taken to go with it.
const MOUSE_UNITS: f32 = 0.02;
/// The soldier's vertical turn against his horizontal one, for the same mouse move.
const FOOT_PITCH_GAIN: f32 = 0.84;
/// The third-person camera: this far behind and above what it follows (soldier, vehicle).
/// In a vehicle the original's `Camera` places it by the vehicle's size instead ([`update_chase`]); this one is on
/// foot (the second, a vehicle's in a check's view from the side, `--chase-yaw`).
const CHASE: [(f32, f32); 2] = [(3.5, 1.0), (14.0, 4.0)];

/// The outside views in a vehicle (`CameraViewMode`: chase 0xc, front chase 0xd, fly-by 0xe).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outside {
    Chase,
    Front,
    FlyBy,
}

/// The outside view's state: where it is from the seat's `Camera` part (it goes there by a lag), and the fly-by's
/// fixed point.
#[derive(Clone, Copy, Debug)]
struct Chase {
    vehicle: usize,
    /// the vehicle's `getBoundingRadius` × 1.2
    reach: f32,
    off: Vec3,
    flyby: Option<Vec3>,
    eye: Vec3,
    look: Vec3,
}

/// One part of a script: from tick `from` to `to` (exclusive) these are held.
#[derive(Debug, Clone)]
pub struct Segment {
    pub from: u64,
    pub to: u64,
    pub keys: Vec<String>,
    pub mouse: Vec2,
}

/// `"0-60:w;60-90:w,mx=2;100:e"`: ranges of ticks (one tick alone: `100`), and in each the keys
/// held (names as in [`axes`]) and the mouse per tick (`mx=`, `my=`, in mouse units).
/// `BYTEFIELD_KEEP_LOGS=1`: the simulation's logs not cut.
fn keep_logs() -> bool {
    static KEEP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *KEEP.get_or_init(|| std::env::var_os("BYTEFIELD_KEEP_LOGS").is_some())
}

pub fn parse_script(s: &str) -> Result<Vec<Segment>, String> {
    let mut out = Vec::new();
    for part in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (range, keys) = part.split_once(':').ok_or(format!("script: no ':' in {part:?}"))?;
        let num = |x: &str| x.trim().parse::<u64>().map_err(|_| format!("script: bad tick {x:?}"));
        let (from, to) = match range.split_once('-') {
            Some((a, b)) => (num(a)?, num(b)?),
            None => (num(range)?, num(range)? + 1),
        };
        let mut seg = Segment { from, to, keys: Vec::new(), mouse: Vec2::ZERO };
        for k in keys.split(',').map(str::trim).filter(|k| !k.is_empty()) {
            if let Some(v) = k.strip_prefix("mx=") {
                seg.mouse.x = v.parse().map_err(|_| format!("script: bad {k:?}"))?;
            } else if let Some(v) = k.strip_prefix("my=") {
                seg.mouse.y = v.parse().map_err(|_| format!("script: bad {k:?}"))?;
            } else {
                seg.keys.push(k.to_ascii_lowercase());
            }
        }
        out.push(seg);
    }
    Ok(out)
}

/// What the player holds in one tick: key names, and the mouse in mouse units.
#[derive(Debug, Clone, Default)]
pub struct Held {
    pub keys: HashSet<String>,
    pub mouse: Vec2,
}

/// The axes of a tick (all of them mapped, as a player's are), from what is held now and what
/// was held the tick before: some only act on the press (E, Z, R, the numbers).
pub fn axes(now: &Held, before: &Held) -> Vec<(usize, f32)> {
    let held = |k: &str| now.keys.contains(k);
    let tap = |k: &str| held(k) && !before.keys.contains(k);
    let pair = |plus: bool, minus: bool| (plus as i32 - minus as i32) as f32;
    let mut a = vec![
        (axis::YAW, pair(held("d"), held("a"))),
        // (the first key of `addKeysToAxisMapping` is the plus, as W's: ↑ dives; it was once the other way)
        (1, pair(held("up"), held("down"))),
        (2, pair(held("right"), held("left"))),
        (axis::THROTTLE, pair(held("w"), held("s"))),
        (axis::MOUSE_X, now.mouse.x),
        (axis::MOUSE_Y, now.mouse.y),
        (axis::FIRE, held("lmb") as i32 as f32),
        (axis::ACTION, held("space") as i32 as f32),
        (bytefield_sim::entry::USE, tap("e") as i32 as f32),
        // (the original's is the left Shift; Alt, ours before, still does)
        (axis::WALK, (held("shift") || held("alt")) as i32 as f32),
        (axis::ALT_FIRE, held("rmb") as i32 as f32),
        (axis::RELOAD, tap("r") as i32 as f32),
        // (held: the server takes one kit every 2 s at most)
        (bytefield_sim::kit::DROP, held("g") as i32 as f32),
        (axis::LIE, tap("z") as i32 as f32),
        (axis::CROUCH, held("ctrl") as i32 as f32),
        // (`c_PIMouseLook`: Shift in a plane, `air_controls`)
        (MOUSE_LOOK, 0.0),
    ];
    for n in 1..=9 {
        a.push((axis::WEAPON1 + n - 1, tap(&n.to_string()) as i32 as f32));
    }
    a.sort_by_key(|x| x.0);
    // (as its PlayerAction leaves them, as the bots')
    bytefield_sim::input::through_actions(&a)
}

#[derive(Resource)]
pub struct Game {
    pub session: LocalSession<'static>,
    /// on a server: the copy follows its snapshots, and the input goes to it
    pub net: Option<UdpClient>,
    /// on a server: our soldier ahead of the copy with the actions not yet taken
    pub prediction: bytefield_server::Prediction,
    /// the player (an index in `sim.players`)
    pub player: usize,
    pub tick: u64,
    /// how far the frame is between the last two ticks
    pub alpha: f32,
    acc: f64,
    pub third_person: bool,
    /// `--third-person` (checks): the chase on foot too, which the original does not have
    pub third_on_foot: bool,
    /// P in a round of one's own (`c_GITogglePause`): the simulation stops
    pub paused: bool,
    /// which outside view in a vehicle (F10, F11, F12; F9 and C go back inside)
    pub outside: Outside,
    /// inside a vehicle without its cockpit, the nose camera (F9 inside, or C, when the
    /// server's `serverAllowNoseCam`, [`bytefield_sim::damage::ServerRules::nose_cam`]): the cockpit hidden, the cross shown
    pub nose: bool,
    /// the outside view turned about the vehicle by the numeric pad (`c_PICameraX/Y`): yaw and pitch, radians
    pub orbit: Vec2,
    /// the outside view's state in a vehicle (the original's `Camera`)
    chase: Option<Chase>,
    /// in an artillery seat, looking from a scout's mark: its projectile (the
    /// index kept at +8)
    pub scout_view: Option<usize>,
    /// the third-person camera turned this much (degrees) about what it follows
    chase_yaw: f32,
    /// `--at`/`--look` while playing: the camera stays there (a check's view of the round)
    fixed: Option<Transform>,
    pub(crate) script: Option<Vec<Segment>>,
    /// a script goes by the clock, not a tick a frame (`BYTEFIELD_REALTIME`)
    realtime: bool,
    /// milliseconds so far in the simulation's `step` and in placing what it moved (`sync`, `hands`): `BYTEFIELD_FRAMES`
    pub spent_ms: [f64; 2],
    /// the command line's factor on the map's sensitivity (`--sens`) and its inversion (`--invert-y`)
    mouse_factor: f32,
    mouse_flip: bool,
    /// gathered from the devices since the last tick
    mouse: Vec2,
    /// the head's own turn of the player's soldier on a seat (`seat_head_turn`). In the client: a seat's soldier is not simulated, and it does not go over the network
    seat_head: f32,
    tapped: HashSet<String>,
    /// the menu is over the round: the devices are its, none of them reaches the soldier
    menu_open: bool,
    pub before: Held,
    /// the camera at the last two ticks
    pub(crate) camera: (Transform, Transform),
    /// `center1pHands` of the soldier's template
    center_1p: [f32; 3],
    /// where the arms are from `center1pHands` and the first person's zoom factor, each frame nearer the weapon's (`arms_ease`)
    arms_eased: Option<([f32; 3], f32)>,
    /// the radio menu open (F1–F8), and the digit keys it took (kept from the weapons until let go)
    pub radio_menu: Option<u32>,
    /// F8: the row of the menus' headings shown (`Radio/ShowRadioIcons`), on at first (as the
    /// original's)
    pub radio_icons: bool,
    radio_taken: HashSet<String>,
    /// what the player chose for its next spawn (spawn group, None: the first of its team; kit),
    /// on the screen when dead; and the digit keys it took there
    pub spawn_choice: (Option<i64>, usize),
    spawn_taken: HashSet<String>,
    /// The chat line being typed (K to everyone, L to the team, as `c_GISayAll`/`c_GISayTeam` in the original's
    /// `Controls/Common.con`): whether to all, and the text; it takes the keys until Enter or Esc.
    pub chat_box: Option<(bool, String)>,
    /// the chat box is a note for this ticket (F11, `tickets.rs`), not for the server
    pub note_to: Option<std::path::PathBuf>,
    /// the chat box is the console's line (º)
    pub console: bool,
    /// everything held since the first tick, for a ticket's replay (`replay.rs`; a round of one's own)
    pub recording: crate::replay::Recording,
    /// `--replay`: the ticket's tick and hash to check
    pub replay: Option<crate::replay::Check>,
}

impl Game {
    pub fn on_orig(&self) -> bool {
        false
    }

    /// Something that reaches the simulation from outside the held keys (the spawn screen, the console): into the
    /// recording at this tick, for a ticket's replay (`replay::apply_events` does it again). Not on a server.
    pub fn record_event(&mut self, name: String) {
        if self.net.is_none() && !self.on_orig() {
            self.recording.event(self.tick, name);
        }
    }

    /// The player chose its next spawn: to the sim, or to the server in every packet.
    pub fn choose_spawn(&mut self, group: Option<i64>, kit: usize) {
        self.spawn_choice = (group, kit);
        match self.net.as_mut() {
            Some(net) => net.mirror.spawn = Some((group, kit as u8)),
            None => self.session.sim.choose_spawn(self.player, group, kit),
        }
    }

    /// The player is done choosing (or not): its first spawn waits for it.
    pub fn hold_spawn(&mut self, hold: bool) {
        match self.net.as_mut() {
            Some(net) => net.mirror.hold = hold,
            None => self.session.sim.hold_spawn(self.player, hold),
        }
    }

    /// Whether it still waits for the player to choose (its first spawn).
    pub fn holding(&self) -> bool {
        match self.net.as_ref() {
            Some(net) => net.mirror.hold,
            None => self.session.sim.players.get(self.player).is_some_and(|p| p.hold),
        }
    }

    /// Mouse units per pixel for what the player controls: its map's sensitivity (level 3: linear in it, with
    /// [`MOUSE_UNITS`] at the soldier's by default) and inversion. In a plane the inversion is the pilot's
    /// ([`air_controls`]): what looks around goes as it came.
    // TODO: `setAirMouseInvert` on the pilot's Shift look, against the original (the gunner's goes as it came, as here: the SBD's with
    // the Custom profile's 1).
    fn mouse_units(&self) -> Vec2 {
        let c = crate::controls::current();
        let map = control_context(self).map;
        let flip = self.mouse_flip ^ (map != crate::controls::Map::Air && c.invert(map));
        // On foot the original turns up and down less than sideways for the same mouse move: 25 px against 30 for 20 units (80 against
        // 93 for 60), a T34 9 m ahead filling the view.
        // TODO: the same in a vehicle's seat (only the soldier's was measured): the same KEYS in a seat with seat-shots.
        let pitch = if map == crate::controls::Map::Infantry { FOOT_PITCH_GAIN } else { 1.0 };
        Vec2::new(1.0, if flip { -pitch } else { pitch }) * MOUSE_UNITS * self.mouse_factor * c.mouse(map) / crate::controls::MOUSE_NEUTRAL
    }

    pub fn dead(&self) -> bool {
        self.session.sim.players.get(self.player).is_some_and(|p| !p.alive) && self.session.sim.round.round_end.is_none()
    }
}


/// What moves: one geometry of a simulated object, where it was at the last two ticks.
#[derive(Component)]
struct Visual {
    prev: Transform,
    cur: Transform,
}

/// The entities drawn for each simulated object (by index in `sim.vehicles`): its template and
/// the geometries, with their part and placement; and whether a cockpit is seen from inside.
/// And those of each object on the ground (by index in `sim.items`: a kit, a bundle), with where
/// they were drawn.
#[derive(Resource, Default)]
struct Shown(HashMap<usize, (String, Vec<Shape>)>, Vec<Entity>, HashMap<usize, (String, [f32; 3], Vec<Entity>)>, HashMap<String, Arc<Vec<model::SeatShow>>>, HashMap<usize, KitPose>);

/// A drawn soldier's skin frame in the world and its bones (in the skin's frame), after a tick: where its kit's parts hang
/// ([`kit_parts`]); and whether it is the player's own seen from its eyes.
struct KitPose {
    frame: Mat4,
    abs: Vec<bytefield_sim::skeleton::M4>,
    file: Arc<bytefield_formats::anim::Skeleton>,
    own_eyes: bool,
}

/// One drawn geometry of an object: its entity, the part it hangs from and where, whether it
/// is the parachute, and its skin (with its own mesh) when its skeleton moves it.
struct Shape {
    entity: Entity,
    part: Option<usize>,
    local: Mat4,
    parachute: bool,
    skin: Option<(Arc<skin::MaterialSkin>, Handle<Mesh>)>,
    first_person: bool,
    /// its template and which of its kind (its contactor: a wheel on a spring moves with it)
    spring: (String, usize),
    /// a track: its skeleton's bones as the `.ske` stands, and the wheel each follows
    track: Option<Arc<(Vec<bytefield_sim::skeleton::M4>, Vec<(String, usize)>)>>,
    /// the parachute, skinned on its own skeleton
    chute: Option<skin::Chute>,
    /// a texture that runs with the engine (`setAnimatedTextureSpeed`): its own material
    scroll: Option<Scroll>,
    /// a wheel: its radius, how far it has turned (radians)
    spin: Option<(Wheel, f32)>,
    /// a side of a cockpit: seen from inside or from outside
    cockpit: Option<model::Cockpit>,
    /// a side of a `CompareSelector`, see [`model::Draw::compare`]
    compare: Option<(f32, bool)>,
    /// a weapon's dummy projectile (`model::Draw::dummy`)
    dummy: bool,
    /// whole only or destroyed only (`model::Draw::wreck`)
    wreck: Option<bool>,
    /// it fades with the wreck ([`bytefield_sim::damage::Armor::wreck_alpha`]): its own material
    fade: Option<Handle<StandardMaterial>>,
    /// its geometry's file (`standardmesh/<file>.sm`, a skin's too), for the terrain's shadow ([`shade_objects`])
    file: Option<String>,
}

/// On every visual update the texture moves by the engine's
/// ratio × its rpm on the bundle's side × the speed, with no time step. Each tick sets the rate;
/// every frame moves it (at 30 Hz a fast tread would jump almost a whole link).
struct Scroll {
    speed: [f32; 2],
    /// the bundle's x on the vehicle: which side of a tank's differential it is on
    side: f32,
    /// UV per second, and where it is
    rate: [f32; 2],
    offset: [f32; 2],
    material: Handle<StandardMaterial>,
}

/// The original's visual updates per second for `Scroll`. Level 2: the formula is the
/// original's, the rate is ours. Ratio × rpm comes out as the speed in m/s, and the treads do not
/// slip at 134 (Tiger) to 240 (Sherman) updates a second; 150 is in the middle of them.
const SCROLL_UPDATES_PER_SECOND: f32 = 150.0;

/// A wheel's size from its meshes: half its height and its axle (the middle of its box, in the
/// object's frame: some meshes, the Panzer IV's road wheels, are not made about their origin).
#[derive(Clone, Copy)]
struct Wheel {
    radius: f32,
    axle: [f64; 3],
}


/// The moving things' parts after each tick: where, shown or not, placed from the camera, posed.
type Visuals<'w, 's> = Query<'w, 's, (&'static mut Visual, &'static mut Visibility, Option<&'static mut FirstPerson>, Option<&'static mut SkinFrames>)>;

/// How far a posed skin may stray from its bind pose's box, in metres (a soldier prone, turned).
const SKIN_REACH: f32 = 2.5;

/// The skinned meshes written so far (`BYTEFIELD_FRAMES`).
pub static SKIN_WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A skinned mesh's vertices at the last two ticks: drawn between them every frame.
/// TODO: skin on the GPU: the vertices of what is seen are still worked out and packed here, on the main thread.
#[derive(Component)]
pub(crate) struct SkinFrames {
    /// the player's own arms in first person: their light changes with the camera, written every frame
    arms: bool,
    /// lit by the mesh's own normals, as the skinning shader does (`skin::write_arms`)
    rest_normals: bool,
    skin: Arc<skin::MaterialSkin>,
    mesh: Handle<Mesh>,
    prev: Vec<[f32; 3]>,
    cur: Vec<[f32; 3]>,
    /// both the same and already in the mesh (a vehicle asleep: nothing to send)
    written: bool,
}

impl SkinFrames {
    fn push(&mut self, pos: Vec<[f32; 3]>) {
        self.prev = if self.cur.is_empty() { pos.clone() } else { std::mem::take(&mut self.cur) };
        self.cur = pos;
        self.written = false;
    }
}

/// What the player sees of itself from its own eyes (the arms, the weapon): placed from the
/// camera every frame (`local` in its frame), not between two ticks, while `on`.
#[derive(Component)]
struct FirstPerson {
    local: Transform,
    on: bool,
}

/// The skins by template (its skeleton), mesh and skin file: loaded once.
#[derive(Resource, Default)]
struct Skins(HashMap<(String, String, String), Option<Vec<Arc<skin::MaterialSkin>>>>);

impl Skins {
    fn get(&mut self, vfs: &bytefield_vfs::Vfs, template: &str, mesh: &str, file: &str, skeleton: &bytefield_formats::anim::Skeleton, partial: bool) -> Option<Vec<Arc<skin::MaterialSkin>>> {
        let key = (template.to_lowercase(), mesh.to_lowercase(), file.to_lowercase());
        self.0
            .entry(key)
            .or_insert_with(|| {
                let s = skin::load(vfs, mesh, file, skeleton, partial);
                if s.is_none() {
                    warn!("{template}: skin {file} does not fit {mesh}");
                }
                s
            })
            .clone()
    }
}

/// The hand weapons' models, by template (loaded once).
#[derive(Resource, Default)]
struct Weapons(HashMap<String, Option<Arc<weapon::Model>>>);

/// The weapon each soldier (by index in `sim.vehicles`) holds: its template and its meshes
/// with the piece of the model each is. Then the parts of each soldier's kit ([`kit_parts`]): the kit's template and
/// each mesh with its bone; and each kit template's parts (loaded once).
#[derive(Resource, Default)]
struct InHand(HashMap<usize, (String, Vec<(Entity, usize)>)>, HashMap<usize, (String, Vec<(Entity, usize)>)>, HashMap<String, Arc<Vec<KitPart>>>);

/// A kit's part (`KitPart`: a helmet, a pack): its geometry and the soldier's bone it hangs from (`setBoneName`).
#[derive(Clone, Debug)]
struct KitPart {
    kind: String,
    file: String,
    bone: String,
}


/// How lit each drawn object stands in the terrain's baked shadow: the sampler, each mesh's box, each object's
/// intensity as it fades, and the tag each entity has.
#[derive(Resource, Default)]
struct Shade {
    sampler: Option<Option<bytefield_world::shadow::ShadowSampler>>,
    boxes: level::MeshBoxes,
    intensity: HashMap<usize, f32>,
    tags: HashMap<Entity, u32>,
}


/// The projectiles in flight: their meshes, by template and starting point.
#[derive(Resource, Default)]
#[allow(clippy::type_complexity)]
struct Shells(HashMap<(String, [u32; 3]), Vec<(Entity, Mat4)>>);

#[derive(Component)]
pub struct PlayCamera;

/// The camera of the first-person layer, and its field of view unzoomed (`set1pFov`).
#[derive(Component)]
pub struct ArmsCamera {
    fov: f32,
}

/// The sniper's scope over the screen (`setScopeIcon`), shown zoomed with a weapon with
/// `useScope` or `setSniperSight`.
#[derive(Component)]
struct Scope;

/// Black on both sides while a cockpit (not a plane's) is seen from inside: its inside is made
/// for a 4:3 screen, and wider the outside shows past its walls. Each side's bar goes as far in
/// as the walls stop covering the screen's edge (rays against the cockpit's triangles, at a
/// few heights), and never into the 4:3 strip. (Ours: the original draws 4:3.)
#[derive(Component)]
struct CockpitBars;

/// The cockpit's walls as last measured: what for (the entities, the aspect, the field of
/// view) and where its bars end (normalised x of the left and right edges, −1 to 1).
#[derive(Default)]
struct BarsMeasure {
    key: (Vec<Entity>, u32, u32),
    edges: [f32; 2],
}



pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::effects::Effects>()
            .init_resource::<crate::lights::Flashes>()
            .init_resource::<Burning>()
            .add_systems(Update, (gather, chat_box, step, crate::replay::check, triggers, armor_fx, crate::effects::level_effects, (interpolate, scroll_textures, camera, scope, cockpit_bars), crate::effects::update.in_set(crate::effects::EffectsUpdate), crate::lights::update).chain().distributive_run_if(resource_exists::<Game>));
    }
}





impl Game {
    /// Whether a script stands for the player (a tick a frame).
    pub fn scripted(&self) -> bool {
        self.script.is_some()
    }
}






#[derive(Component)]
struct CockpitBar(usize);


