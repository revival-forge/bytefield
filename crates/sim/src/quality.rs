//! What can be seen of a side from the outside, one card at a time from the B01..B18 list: the `bots` dashboard.
//! Like [`crate::dumb::Detector`], the core ([`Meter::sample`]) only sees neutral snapshots (`P`, `V`, `F`) once per
//! second plus shots and heals as they happen, so the same number holds for any mind and can be tested without a match;
//! [`Meter::step`] extracts them from a [`crate::Sim`] (never from a bot's internal state). B16 is a [`crate::dumb::Watcher`] inside.
//!
//! B04 is measured without knowing about squads: a spawn (new life) counts as "with its own" if it is born within [`SPAWN_M`] of
//! at least two teammates who were already alive in the previous sample. B01 "fraction at an own flag" only counts those alive for
//! more than [`SETTLED_S`]: on spawning everybody is born at their base, and that is the spawn, not a flag choice.
//! What cannot be measured today comes out as an absent field, not as 0:
//! B10 (already in `bot_caps`), B17 (still-person recipe). B01 "first capture" is missing if the side captures none; the
//! percentages, if there were no cases.
//!
//! B05: `B05_pct_covered` counts those who stop firing *or* move away (the card asks for both: it is loose, kept for
//! comparison); `B05_pct_breaks_contact` is the card: stops firing *and* ends up hidden ([`P::hidden`], via `Sim::hidden`)
//! or farther than 60 m. B14, its visible part: whoever loses health on foot with no enemy in sight within 60 m, in 2 s has it moved
//! more than 3 m or looked (± [`B14_ANGLE`]) at the nearest visible enemy? Full B14 adds hearing: an enemy shot within
//! [`B14_HEARING_M`] in the [`B14_HEARING_S`] s before the damage is a heard enemy, and looking toward it counts as reacting.
//!
//! B15 (the radio, from the outside: only `Sim::radio_log` is read, never a mind's state): messages per life, % with a cause
//! ([`cause_b15`]: a table message -> visible state of the speaker at that moment) and, after a "medic!", whether someone heals them in
//! [`B15_MEDIC_S`] s. With bots that do not speak it gives 0 messages (and the rest, absent).
use crate::bot_api::VehicleClass;
use crate::dumb::Watcher;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// A flag is contested with bots of both sides within this distance (B01), m.
pub const CONTEST_M: f32 = 30.0;
/// "No enemy" in B01, B02 and B13, m.
pub const ENEMY_M: f32 = 60.0;
/// A tank is escorted by a teammate not inside it within this distance (B02/B03), m.
pub const ESCORT_M: f32 = 40.0;
/// Together in B02, m.
pub const TOGETHER_M: f32 = 15.0;
/// A trip counts once it gets this far from where it started (B03), m.
pub const TRIP_M: f32 = 80.0;
/// B05: low health, enemy near, window, and "far", m and s.
pub const LOW_HP: f32 = 0.3;
pub const NEAR_M: f32 = 20.0;
pub const WINDOW_S: f64 = 10.0;
pub const FAR_M: f32 = 60.0;
/// B14: window, displacement and angle, s, m and degrees; how far to look for the visible enemy to face, m.
pub const B14_WINDOW_S: f64 = 2.0;
pub const B14_MOVE_M: f32 = 3.0;
pub const B14_ANGLE: f32 = 30.0;
pub const B14_SIGHT_M: f32 = 150.0;
/// Full B14: an enemy shot is heard within this distance, m, and up to this long before the damage, s.
pub const B14_HEARING_M: f32 = 100.0;
pub const B14_HEARING_S: f64 = 3.0;
/// B15: a "seen" cause (enemy of that class, flag) within this distance, m; health (of a person or vehicle) "low" below
/// this; a "medic!" is attended (someone heals them) within this, s.
pub const B15_SEEN_M: f32 = 150.0;
pub const B15_FLAG_M: f32 = 200.0;
pub const B15_LOW_HP: f32 = 0.5;
pub const B15_MEDIC_S: f64 = 20.0;
/// B25: a "medic!" has an own medic "at hand" when one is alive within this of the caller, m
pub const B25_MEDIC_M: f32 = 150.0;
/// B06: recovered.
pub const HP_RECOVERED: f32 = 0.6;
/// B07: a crewed vehicle goes from fewer to more.
pub const VEH_LOW: f32 = 0.4;
pub const VEH_HIGH: f32 = 0.7;
/// B09: cluster.
pub const CLUSTER_M: f32 = 5.0;
pub const CLUSTER_S: f64 = 20.0;
/// B12: keep following on foot after getting off, s and m; maximum height of a helicopter that "lands".
pub const FOLLOW_S: f64 = 30.0;
pub const FOLLOW_M: f32 = 15.0;
pub const HELI_HEIGHT: f32 = 15.0;
/// B04: born with its own, m; B01: alive at least this long to count at a flag, s.
pub const SPAWN_M: f32 = 40.0;
pub const SETTLED_S: f64 = 20.0;
/// B18: rush to help, m and s.
pub const RUSH_M: f32 = 150.0;
pub const RUSH_S: f64 = 40.0;
/// B08: pieces of the name (lowercase) of an anti-tank projectile or rocket.
pub const AT_NAMES: [&str; 6] = ["rocket", "bazooka", "panzerschreck", "rpg", "law", "at_"];
/// B08: an enemy armoured vehicle within this of an on-foot anti-tank soldier opens a case; checks whether it gets a rocket in the window, m and s.
pub const B08_NEAR_M: f32 = 40.0;
pub const B08_WINDOW_S: f64 = 10.0;
/// B08: a rocket's target is the enemy nearest to its line, within this, degrees.
pub const B08_LINE_DEGREES: f32 = 10.0;
/// B08: the names (lowercase) of the anti-tank weapons a soldier can carry (to know who is anti-tank).
pub const AT_WEAPONS: [&str; 12] = ["bazooka", "panzershreck", "panzerschreck", "piat", "rocket", "rpg", "at4", "smaw", "carl", "law", "plx", "flechette"];

/// A player at one instant. `id` is its slot in `Sim::players`.
#[derive(Clone, Debug, Default)]
pub struct P {
    pub id: usize,
    pub team: u32,
    pub alive: bool,
    pub pos: [f32; 3],
    /// health, 0 to 1
    pub hp: f32,
    /// the vehicle it is in (its index in `Sim::vehicles`)
    pub vehicle: Option<usize>,
    pub kills: u32,
    pub deaths: u32,
    /// which soldier it is (changes with each life)
    pub life: Option<usize>,
    /// B05: a wall or the terrain hides it from the nearest enemy (only checked at low health)
    pub hidden: bool,
    /// B08: carries an anti-tank weapon
    pub at: bool,
    /// B35: on foot, healthy, still since the last sample and with an enemy in sight at [`B35_NEAR_M`]..[`B35_FAR_M`]: whether its
    /// crouched body is hidden from that enemy (`None`: not a case; only worked out for those cases, it costs rays)
    pub b35: Option<bool>,
}

/// A vehicle with someone inside.
#[derive(Clone, Debug)]
pub struct V {
    pub id: usize,
    pub class: VehicleClass,
    pub seats: usize,
    /// ids of the players on board
    pub occupants: Vec<usize>,
    pub pos: [f32; 3],
    /// health, 0 to 1 (0: destroyed)
    pub hp: f32,
    /// height above the ground, m
    pub alt: f32,
    /// there is land within 50 m (only checked on boats and helicopters)
    pub land_near: bool,
}

#[derive(Clone, Debug)]
pub struct F {
    pub pos: [f32; 3],
    pub radius: f32,
    pub team: u32,
    pub takeable: bool,
}

fn d(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn pct(n: u32, de: u32) -> Option<f64> {
    (de > 0).then(|| (1000.0 * n as f64 / de as f64).round() / 10.0)
}

fn median_s(v: &[f32]) -> Option<f64> {
    let mut v = v.to_vec();
    v.sort_by(f32::total_cmp);
    (!v.is_empty()).then(|| round2(v[v.len() / 2] as f64))
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[derive(Default, Debug, Clone)]
struct Side {
    first_capture: Option<f64>,
    fraction_max: f32,
    b02: [u32; 2],
    b03_trips: u32,
    /// B03: of the trips, how long passed from someone boarding until a second one boarded, s (only those that had one)
    b03_wait: Vec<f32>,
    b03_with2: u32,
    /// B02/B03: samples of a tank with its driver out of combat, and of those, with a teammate outside the tank nearby
    escort: [u32; 2],
    b03_occupancy: f32,
    b05: [u32; 5],
    b14: [u32; 2],
    /// Full B14: all the cases (reacted or looked at the heard shot), and those that also had a heard shot
    b14_full: [u32; 2],
    b14_heard: [u32; 2],
    /// B15: messages with a cause and messages (not counting acknowledgements), lives, maximum messages in one life, acknowledgements, and "medic!"
    /// attended and requested
    b15: [u32; 2],
    b15_lives: u32,
    b15_max_life: u32,
    b15_acks: u32,
    b15_medic: [u32; 2],
    /// B25: "medic!" calls with an own medic alive within [`B25_MEDIC_M`] at the moment of calling: attended and total; and the calls with
    /// none within it
    b25_near: [u32; 2],
    b25_no_medic: u32,
    b25_calls: u32,
    /// B25 among the callers that live [`B15_MEDIC_S`] s (or are healed before): attended and total; and the near calls whose
    /// caller died first, unattended (a medic cannot answer those)
    b25_live: [u32; 2],
    b25_died: u32,
    b06_healed_by_others: f32,
    b06_healed: f32,
    b06: [u32; 3],
    b07_repaired: f32,
    b07_raised: u32,
    b08: [u32; 2],
    /// B08: of the rockets, those aimed at an on-foot soldier (along the shot's line)
    b08_line: u32,
    /// B08: armoured vehicles within 40 m of an anti-tank soldier, and those that received a rocket within 10 s
    b08_armoured: [u32; 2],
    b09: [u32; 2],
    b11: [u32; 2],
    b12_multi: u32,
    b12_near: u32,
    b12_got_off: u32,
    b12_follow: [u32; 2],
    b13: [u32; 2],
    b18: [u32; 2],
    b04: [u32; 2],
    /// B17: reaction times (enemy in sight -> first shot), s, and aiming errors of on-foot shots, degrees
    b17_reaction: Vec<f32>,
    b17_error: Vec<f32>,
}

/// B20: a soldier is "still at its spawn" if, this long after being born, it is within `B20_RADIUS_M` of where it appeared.
pub const B20_AGE_S: f64 = 60.0;
pub const B20_RADIUS_M: f32 = 30.0;
/// B21: a vehicle with people arrives when it is within `B21_ARRIVE_M` of an enemy flag (the nearest) with no enemy within `B21_CLEAR_M`; someone
/// of its crew must be inside the flag's radius within `B21_WINDOW_S`.
pub const B21_ARRIVE_M: f32 = 80.0;
pub const B21_CLEAR_M: f32 = 40.0;
pub const B21_WINDOW_S: f64 = 20.0;
/// B22: an aircraft with a pilot should be higher than `HELI_HEIGHT` this long after it was first seen with someone in it; the
/// passengers of a flight should end on foot within `B22_FLAG_M` of an enemy flag.
pub const B22_TAKEOFF_S: f64 = 30.0;
pub const B22_FLAG_M: f32 = 80.0;
/// B23: seconds in the air counted only with an enemy within this ground distance, m; a drop is "near" within `B23_NEAR_M` of an enemy
/// vehicle or an enemy inside a flag.
pub const B23_ENEMY_M: f32 = 420.0;
pub const B23_NEAR_M: f32 = 30.0;
/// B26: an aircraft is in contact with an enemy aircraft (both in flight) within this distance, m; a crash with an enemy aircraft this
/// near in the `B26_CRASH_S` before it is not counted as a crash.
pub const B26_CONTACT_M: f32 = 600.0;
pub const B26_CRASH_NEAR_M: f32 = 300.0;
pub const B26_CRASH_S: f64 = 5.0;
/// B26: an aircraft destroyed, any way, within this many seconds of a hit from an enemy aircraft while it flew was downed by that side
/// (the pilot who jumps out of a burning aircraft leaves it to fall: long enough for a fall from the heights they fight at).
pub const B26_DOWNED_S: f64 = 20.0;

fn d3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[derive(Default, Debug, Clone)]
struct Side2 {
    /// B20: soldiers still near their spawn at `B20_AGE_S`, and soldiers checked
    b20: [u32; 2],
    /// B21: arrivals whose crew got inside the flag, and arrivals
    b21: [u32; 2],
    /// B22: aircraft airborne at `B22_TAKEOFF_S`, and aircraft with a pilot; flights with 2 or more passengers ending near an enemy flag
    b22_up: [u32; 2],
    b22_near_flights: u32,
    /// B23: seconds of aircraft in the air with an enemy near, shots from aircraft, drops near an enemy and drops
    b23_air_s: u32,
    b23_shots: u32,
    b23_drops: [u32; 2],
    /// B26: enemy aircraft in flight destroyed by one of ours, seconds in contact, shots and hits at an enemy aircraft, crashes
    b26_kills: u32,
    b26_downed: u32,
    b26_contact_s: u32,
    b26_shots: u32,
    b26_hits: u32,
    b26_crashes: u32,
}

/// B26's view of the aircraft with someone inside, refreshed after every step: (team, where, in flight).
#[derive(Default, Debug)]
struct Air {
    now: BTreeMap<usize, (u32, [f32; 3], bool)>,
    /// the last time an enemy aircraft in flight was within `B26_CRASH_NEAR_M`, per aircraft
    near_enemy: BTreeMap<usize, f64>,
    /// projectiles fired at an enemy aircraft within reach, waiting to hit: (projectile slot, team of the shooter)
    pending: Vec<(usize, u32)>,
    /// the last hit from an enemy aircraft that each aircraft took in flight: (when, the side that hit it)
    hit: BTreeMap<usize, (f64, u32)>,
    fired_seen: usize,
    damage_seen: usize,
}

/// The second-iteration cards (B20..B23), seen from outside like the rest.
#[derive(Default, Debug)]
struct Iter2 {
    s: [Side2; 3],
    /// B20: per player, the soldier it is checked for, where it appeared and whether it was already checked
    spawn: BTreeMap<usize, (Option<usize>, [f32; 3], bool)>,
    /// B21: (vehicle, flag) arrivals seen, and the open ones: (vehicle, flag, when, team, crew, inside)
    arrivals: BTreeSet<(usize, usize)>,
    open: Vec<(usize, usize, f64, u32, Vec<usize>, bool)>,
    /// B22: per aircraft, when it was first seen with someone in it and whether it was already checked
    aircraft: BTreeMap<usize, (f64, bool)>,
    /// B22: per helicopter, the passengers that ended on foot near an enemy flag, and the flights already counted
    passengers: BTreeMap<usize, BTreeSet<usize>>,
    counted: BTreeSet<usize>,
}

#[derive(Clone, Debug)]
struct Trip {
    start: [f32; 3],
    team: u32,
    counted: bool,
    /// when it was first seen with someone inside, and when with a second on board
    t0: f64,
    t2: Option<f64>,
}

#[derive(Clone, Debug, Default)]
struct CraftB12 {
    team: u32,
    previous: Vec<usize>,
    multi: bool,
    near: bool,
    /// how it was in the last sample (for when it empties: it stops being among the vehicles with someone inside)
    last: Option<V>,
}

#[derive(Clone, Debug, Default)]
struct LifeB06 {
    soldier: Option<usize>,
    low: bool,
    recovered: bool,
}

struct Death {
    t: f64,
    team: u32,
    pos: [f32; 3],
    cluster: bool,
}

struct CaseB18 {
    flag: usize,
    t0: f64,
    team: u32,
}

/// See the module docs. Fed in order: `sample`, once per second; `shot`, `heal` and `repair` as they happen (or `step` with a `Sim`).
pub struct Meter {
    pub watcher: Watcher,
    /// B21 is not measured: on maps whose profile makes the driver wait outside a defended flag on purpose
    /// ([`crate::bots::body::MapProfile::enter_wait`], Berlin) an arrival that stays outside is not a failure
    pub b21_off: bool,
    now: f64,
    first: Option<f64>,
    side: [Side; 3],
    contested_max: u32,
    owners: Vec<u32>,
    alive_now: Vec<P>,
    last_shot: BTreeMap<usize, f64>,
    last_pos: BTreeMap<usize, [f32; 3]>,
    trips: BTreeMap<usize, Trip>,
    b05: BTreeMap<usize, f64>,
    lives: BTreeMap<usize, LifeB06>,
    crew_low: BTreeMap<usize, u32>,
    deaths_seen: Vec<Death>,
    deaths_prev: BTreeMap<usize, u32>,
    kills_prev: BTreeMap<usize, u32>,
    air_prev: BTreeMap<usize, bool>,
    crafts: BTreeMap<usize, CraftB12>,
    got_off: Vec<(f64, usize, u32, [f32; 3], usize, bool)>,
    inside: Vec<bool>,
    cases_b18: Vec<CaseB18>,
    /// the life (soldier) of each player and when it was born
    born: BTreeMap<usize, (Option<usize>, f64)>,
    fired_seen: usize,
    damage_seen: usize,
    /// B23: the bombs from aircraft still falling: (projectile index, side, where it left, where it was last seen)
    bombs: Vec<(usize, u32, [f32; 3], [f32; 3])>,
    /// B14: open cases (player, soldier, when, where it was)
    b14: Vec<(usize, usize, f64, [f32; 3], Option<[f32; 3]>)>,
    /// Full B14: the shots of the last [`B14_HEARING_S`] s (when, side, where)
    heard_shots: Vec<(f64, u32, [f32; 3])>,
    /// B14: when each player last fired (on foot or from a seat)
    last_fired: BTreeMap<usize, f64>,
    /// B15: how many `Sim::radio_log` messages have been seen, the messages of each life (player, soldier) and the open "medic!" requests
    /// (player, soldier, when, side, health when asking)
    radio_seen: usize,
    b15_life: BTreeMap<(usize, Option<usize>, u32), u32>,
    b15_medic: Vec<(usize, usize, f64, u32, f32, bool)>,
    heals_seen: [u64; 3],
    /// B08: open cases (vehicle, anti-tank side, when, anti-tank soldiers that had it nearby, received a rocket)
    b08: Vec<(usize, u32, f64, Vec<usize>, bool)>,
    /// B17: per player, its soldier, the last time it had an enemy in sight and since when it has had it (if new: after
    /// [`B17_CALM_S`] without seeing anyone)
    b17: BTreeMap<usize, (usize, f64, Option<f64>)>,
    n: Iter2,
    air: Air,
    books: Books,
    st: Streetwise,
}

/// B17: an enemy in sight within this distance, m, is "new" after this long without seeing anyone, s; the first shot counts if it comes before
/// [`B17_WINDOW_S`]; a shot's error is the angle to the enemy nearest to its line, if within [`B17_ERROR_MAX`] degrees.
pub const B17_SIGHT_M: f32 = 60.0;
pub const B17_CALM_S: f64 = 3.0;
pub const B17_WINDOW_S: f64 = 10.0;
pub const B17_ERROR_MAX: f32 = 20.0;

impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}

impl Meter {
    pub fn new() -> Meter {
        Meter {
            watcher: Watcher::new(),
            b21_off: false,
            now: 0.0,
            first: None,
            side: Default::default(),
            contested_max: 0,
            owners: Vec::new(),
            alive_now: Vec::new(),
            last_shot: BTreeMap::new(),
            last_pos: BTreeMap::new(),
            trips: BTreeMap::new(),
            b05: BTreeMap::new(),
            lives: BTreeMap::new(),
            crew_low: BTreeMap::new(),
            deaths_seen: Vec::new(),
            deaths_prev: BTreeMap::new(),
            kills_prev: BTreeMap::new(),
            air_prev: BTreeMap::new(),
            crafts: BTreeMap::new(),
            got_off: Vec::new(),
            inside: Vec::new(),
            cases_b18: Vec::new(),
            born: BTreeMap::new(),
            fired_seen: 0,
            damage_seen: 0,
            bombs: Vec::new(),
            b14: Vec::new(),
            heard_shots: Vec::new(),
            last_fired: BTreeMap::new(),
            radio_seen: 0,
            b15_life: BTreeMap::new(),
            b15_medic: Vec::new(),
            heals_seen: [0; 3],
            b08: Vec::new(),
            b17: BTreeMap::new(),
            n: Iter2::default(),
            air: Air::default(),
            books: Books::default(),
            st: Streetwise::default(),
        }
    }

    /// B20..B23 (second iteration), per sample.
    fn sample_iter2(&mut self, t: f64, alive: &[P], vs: &[V], fs: &[F]) {
        let n = &mut self.n;
        // B20: where each soldier appeared, and where it is `B20_AGE_S` later
        for p in alive {
            let e = n.spawn.entry(p.id).or_insert((p.life, p.pos, false));
            if e.0 != p.life {
                *e = (p.life, p.pos, false);
            }
            let born = self.born.get(&p.id).map_or(t, |x| x.1);
            if !e.2 && t - born >= B20_AGE_S && (1..=2).contains(&p.team) {
                e.2 = true;
                let l = &mut n.s[p.team as usize];
                l.b20[1] += 1;
                if d(p.pos, e.1) < B20_RADIUS_M {
                    l.b20[0] += 1;
                }
            }
        }
        let team_of = |id: usize| alive.iter().find(|p| p.id == id).map_or(0, |p| p.team);
        for v in vs {
            let Some(&first) = v.occupants.first() else { continue };
            let team = team_of(first);
            if !(1..=2).contains(&team) || v.hp <= 0.0 {
                continue;
            }
            let aircraft = v.class.flies();
            // B21: the arrival of a vehicle with people at an enemy flag
            if !self.b21_off && !matches!(v.class, VehicleClass::Fixed | VehicleClass::Plane) && !alive.iter().any(|p| p.team != team && d(p.pos, v.pos) < B21_CLEAR_M) {
                // (only the nearest enemy flag: on DC_Al_Nas two lie 50 m apart, and a vehicle "arrived" at both, entering one)
                let near = fs.iter().enumerate().filter(|(_, f)| f.takeable && f.team != team && d(v.pos, f.pos) < B21_ARRIVE_M).min_by(|a, b| d(v.pos, a.1.pos).total_cmp(&d(v.pos, b.1.pos)));
                if let Some((i, _)) = near
                    && n.arrivals.insert((v.id, i))
                {
                    n.open.push((v.id, i, t, team, v.occupants.clone(), false));
                }
            }
            if aircraft {
                // B22: takeoff
                let e = n.aircraft.entry(v.id).or_insert((t, false));
                if !e.1 && t - e.0 >= B22_TAKEOFF_S {
                    e.1 = true;
                    let l = &mut n.s[team as usize];
                    l.b22_up[1] += 1;
                    if v.alt > HELI_HEIGHT {
                        l.b22_up[0] += 1;
                    }
                }
                // B23: time in the air with an enemy near
                if v.alt > HELI_HEIGHT && alive.iter().any(|p| p.team != team && d(p.pos, v.pos) < B23_ENEMY_M) {
                    n.s[team as usize].b23_air_s += 1;
                }
                // B26: seconds in contact with an enemy aircraft in flight
                if v.alt > HELI_HEIGHT
                    && vs.iter().any(|o| {
                        let ot = o.occupants.first().map_or(0, |&id| team_of(id));
                        ot != team && (1..=2).contains(&ot) && o.hp > 0.0 && o.alt > HELI_HEIGHT && o.class.flies() && d3(o.pos, v.pos) < B26_CONTACT_M
                    })
                {
                    n.s[team as usize].b26_contact_s += 1;
                }
            }
        }
        // B21: close the arrivals
        let mut open = std::mem::take(&mut n.open);
        open.retain_mut(|c| {
            let f = &fs[c.1];
            if alive.iter().any(|p| c.4.contains(&p.id) && d(p.pos, f.pos) < f.radius.max(5.0)) {
                c.5 = true;
            }
            if c.5 || t - c.2 >= B21_WINDOW_S {
                if crate::debug::options().quality_b21 {
                    let vd = vs.iter().find(|v| v.id == c.0).map_or(-1.0, |v| d(v.pos, f.pos));
                    let crew: Vec<String> = c.4.iter().map(|id| alive.iter().find(|p| p.id == *id).map_or("dead".into(), |p| format!("{}:{:.0}", p.id, d(p.pos, f.pos)))).collect();
                    eprintln!("b21 t={:.0} since={:.0} team {} vehicle {} flag {} radius {:.0} vehicle_d {vd:.0} crew {crew:?} inside {}", t, c.2, c.3, c.0, c.1, f.radius, c.5);
                }
                let l = &mut n.s[c.3 as usize];
                l.b21[1] += 1;
                l.b21[0] += c.5 as u32;
                return false;
            }
            true
        });
        n.open = open;
    }

    /// B23: a shot from an aircraft of `team` with `weapon` (its projectile: index and where it left); a drop (bomb) is followed until
    /// it lands ([`Meter::bombs_step`]).
    fn air_shot(&mut self, team: u32, weapon: &str, projectile: Option<(usize, [f32; 3])>) {
        if !(1..=2).contains(&team) {
            return;
        }
        let l = &mut self.n.s[team as usize];
        l.b23_shots += 1;
        if weapon.to_lowercase().contains("bomb") {
            l.b23_drops[1] += 1;
            if let Some((k, start)) = projectile {
                self.bombs.push((k, team, start, start));
            }
        }
    }

    /// B23: the bombs that landed this step: one is near if an enemy vehicle or an enemy inside a flag is within `B23_NEAR_M` of where
    /// it ended (where it was released says nothing: a well dropped bomb falls a few hundred metres further on).
    fn bombs_step(&mut self, s: &crate::Sim, fs: &[F]) {
        let mut landed = Vec::new();
        self.bombs.retain_mut(|b| {
            match s.projectiles.get(b.0).filter(|p| p.start == b.2) {
                Some(p) if p.alive() => {
                    b.3 = p.pos;
                    true
                }
                Some(p) => {
                    landed.push((b.1, p.pos));
                    false
                }
                // (its place taken by another: where it was last seen)
                None => {
                    landed.push((b.1, b.3));
                    false
                }
            }
        });
        for (team, at) in landed {
            let near = self.alive_now.iter().any(|p| p.team != team && d(p.pos, at) < B23_NEAR_M && (p.vehicle.is_some() || fs.iter().any(|f| d(p.pos, f.pos) < f.radius)));
            self.n.s[team as usize].b23_drops[0] += near as u32;
        }
    }

    /// B26: the aircraft with someone inside, now: (id, team, where, in flight). Call after the step's events were seen.
    pub fn air_frame(&mut self, t: f64, craft: &[(usize, u32, [f32; 3], bool)]) {
        let a = &mut self.air;
        a.now = craft.iter().map(|&(id, team, pos, up)| (id, (team, pos, up))).collect();
        for &(id, team, pos, _) in craft {
            if craft.iter().any(|&(o, ot, op, oup)| o != id && ot != team && oup && d3(op, pos) < B26_CRASH_NEAR_M) {
                a.near_enemy.insert(id, t);
            }
        }
    }

    /// B26: a shot from the aircraft `shooter`; `slot` is the projectile it made, if any. Counts when it is in flight with an enemy
    /// aircraft in flight within `B26_CONTACT_M`.
    pub fn air_fire(&mut self, shooter: usize, slot: Option<usize>) {
        let Some(&(team, pos, up)) = self.air.now.get(&shooter) else { return };
        if !up || !(1..=2).contains(&team) || !self.air.now.values().any(|&(t, p, u)| u && t != team && d3(p, pos) < B26_CONTACT_M) {
            return;
        }
        self.n.s[team as usize].b26_shots += 1;
        if let Some(k) = slot {
            self.air.pending.push((k, team));
        }
    }

    /// B26: the pending shots at `t`, given what each projectile has hit so far (`seen`: the vehicles, and whether it ended).
    pub fn air_hits(&mut self, t: f64, mut seen: impl FnMut(usize) -> (Vec<usize>, bool)) {
        let pending = std::mem::take(&mut self.air.pending);
        for (k, team) in pending {
            let (hit, ended) = seen(k);
            if let Some(&v) = hit.iter().find(|v| self.air.now.get(v).is_some_and(|&(t, _, up)| up && t != team)) {
                self.n.s[team as usize].b26_hits += 1;
                self.air.hit.insert(v, (t, team));
            } else if !ended {
                self.air.pending.push((k, team));
            }
        }
    }

    /// B26: the aircraft `victim` was destroyed at `t`, by a crash or not, and `by` is the vehicle that gave the last damage. A kill is an
    /// enemy aircraft in flight whose last damage came from an aircraft of the other side with someone inside; a crash, an aircraft
    /// with someone inside destroyed by a crash with no enemy aircraft within `B26_CRASH_NEAR_M` in the last `B26_CRASH_S` s. And it was
    /// downed, with or without anyone left inside, if an enemy aircraft hit it in flight in the last `B26_DOWNED_S` s.
    pub fn air_destroyed(&mut self, t: f64, victim: usize, crash: bool, by: Option<usize>) {
        if let Some((_, side)) = self.air.hit.remove(&victim).filter(|&(t0, _)| t - t0 <= B26_DOWNED_S) {
            self.n.s[side as usize].b26_downed += 1;
        }
        let Some(&(team, _, up)) = self.air.now.get(&victim) else { return };
        if !(1..=2).contains(&team) {
            return;
        }
        let killer = by.and_then(|b| self.air.now.get(&b)).map(|x| x.0).filter(|&bt| bt != team && (1..=2).contains(&bt));
        if let (Some(bt), true) = (killer, up) {
            self.n.s[bt as usize].b26_kills += 1;
        } else if crash && killer.is_none() && self.air.near_enemy.get(&victim).is_none_or(|&t0| t - t0 > B26_CRASH_S) {
            self.n.s[team as usize].b26_crashes += 1;
        }
    }

    /// B26 from the sim: the shots, hits and destructions of this step.
    fn b26_step(&mut self, s: &crate::Sim, t: f64) {
        use crate::damage::{Source, Status};
        let fired: Vec<(usize, Option<usize>)> = s.journal.fired.get(self.air.fired_seen..).unwrap_or(&[]).iter().map(|f| (f.1, f.3)).collect();
        self.air.fired_seen = s.journal.fired.len();
        for (v, slot) in fired {
            self.air_fire(v, slot);
        }
        self.air_hits(t, |k| s.projectiles.get(k).map_or((Vec::new(), true), |p| (p.impacts.iter().filter_map(|i| i.vehicle).collect(), p.end.is_some())));
        let events: Vec<(usize, bool, Option<usize>)> = s
            .journal
            .damage_log
            .get(self.air.damage_seen..)
            .unwrap_or(&[])
            .iter()
            .filter(|e| e.status == Some(Status::Destroyed))
            .map(|e| (e.vehicle, e.source == Source::Crash, e.by))
            .collect();
        self.air.damage_seen = s.journal.damage_log.len();
        for (v, crash, by) in events {
            self.air_destroyed(t, v, crash, by);
        }
        let ground = |p: [f32; 3]| s.world.height_at(p[0] as f64, p[2] as f64).unwrap_or(0.0) as f32;
        let mut craft: BTreeMap<usize, (u32, [f32; 3], bool)> = BTreeMap::new();
        for p in s.players.iter().filter(|p| p.alive && (1..=2).contains(&p.team)) {
            let Some(i) = p.soldier.and_then(|i| s.vehicles.get(i)).and_then(|v| v.entry.inside) else { continue };
            let Some(v) = s.vehicles.get(i).filter(|v| !v.removed && VehicleClass::of(v).flies()) else { continue };
            let pos = v.craft.body.pos;
            craft.entry(i).or_insert((p.team, pos, pos[1] - ground(pos) > HELI_HEIGHT));
        }
        let craft: Vec<_> = craft.into_iter().map(|(id, (team, pos, up))| (id, team, pos, up)).collect();
        self.air_frame(t, &craft);
    }

    /// B08: a shot by `player` with projectile `weapon`, from `pos` (only counts on foot). B05: when it last fired.
    pub fn shot(&mut self, t: f64, player: usize, on_foot: bool, weapon: &str, pos: [f32; 3]) {
        self.shot_dir(t, player, on_foot, weapon, pos, None);
    }

    /// The same, with the direction the projectile leaves in: B08 also looks at what it was aimed at (the enemy nearest to that line, within
    /// [`B08_LINE_DEGREES`] and 150 m; if there is none, the nearest).
    pub fn shot_dir(&mut self, t: f64, player: usize, on_foot: bool, weapon: &str, pos: [f32; 3], dir: Option<[f32; 3]>) {
        self.last_shot.insert(player, t);
        let n = weapon.to_lowercase();
        if !on_foot || !AT_NAMES.iter().any(|k| n.contains(k)) {
            return;
        }
        let team = self.alive_now.iter().find(|p| p.id == player).map_or(0, |p| p.team);
        if !(1..=2).contains(&team) {
            return;
        }
        for c in self.b08.iter_mut().filter(|c| c.3.contains(&player)) {
            c.4 = true;
        }
        let Some(e) = self.alive_now.iter().filter(|p| p.team != team).min_by(|a, b| d(a.pos, pos).total_cmp(&d(b.pos, pos))) else { return };
        let is_soldier = e.vehicle.is_none();
        let angle = |p: &P| {
            let dir = dir?;
            let a = [p.pos[0] - pos[0], p.pos[1] + 0.8 - pos[1], p.pos[2] - pos[2]];
            let (la, ld) = ((a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt(), (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt());
            if la > 150.0 || la < 0.5 || ld <= 0.0 {
                return None;
            }
            let g = ((a[0] * dir[0] + a[1] * dir[1] + a[2] * dir[2]) / (la * ld)).clamp(-1.0, 1.0).acos().to_degrees();
            (g < B08_LINE_DEGREES).then_some(g)
        };
        let aimed_at = self.alive_now.iter().filter(|p| p.team != team).filter_map(|p| angle(p).map(|g| (g, p))).min_by(|a, b| a.0.total_cmp(&b.0)).map(|x| x.1);
        let soldier_on_line = aimed_at.map_or(is_soldier, |p| p.vehicle.is_none());
        let l = &mut self.side[team as usize];
        l.b08[1] += 1;
        if is_soldier {
            l.b08[0] += 1;
        }
        l.b08_line += soldier_on_line as u32;
    }

    /// B06: a heal of `hp` points to a soldier of `team`; `by_others`, 0 to 1, what share was given by others (the damage log does not
    /// say who heals: `Sim::step` splits it using what `Sim::heals_by` counts in that step).
    pub fn heal(&mut self, team: u32, hp: f32, by_others: f32) {
        if (1..=2).contains(&team) {
            let l = &mut self.side[team as usize];
            l.b06_healed += hp;
            l.b06_healed_by_others += hp * by_others;
        }
    }

    /// B07: a repair of `hp` points to a vehicle of `team`.
    pub fn repair(&mut self, team: u32, hp: f32) {
        if (1..=2).contains(&team) {
            self.side[team as usize].b07_repaired += hp;
        }
    }

    /// B14: a case closes; `reacted`, whether it moved or looked at the enemy.
    pub fn b14(&mut self, team: u32, reacted: bool) {
        if (1..=2).contains(&team) {
            let l = &mut self.side[team as usize];
            l.b14[1] += 1;
            l.b14[0] += reacted as u32;
        }
    }

    /// Full B14: a case closes; `heard`, whether there was a heard enemy shot; `full`, whether it moved, looked at the visible enemy or
    /// looked at the heard shot.
    pub fn b14_heard(&mut self, team: u32, heard: bool, full: bool) {
        if (1..=2).contains(&team) {
            let l = &mut self.side[team as usize];
            l.b14_full[1] += 1;
            l.b14_full[0] += full as u32;
            if heard {
                l.b14_heard[1] += 1;
                l.b14_heard[0] += full as u32;
            }
        }
    }

    /// B15: player `player` (in its life `life`, of `team`) says `key` at `t` with `ctx` around. A "medic!" opens the case
    /// for its answer.
    pub fn radio(&mut self, t: f64, team: u32, player: usize, life: Option<usize>, key: &str, ctx: &ContextB15) {
        self.radio_life(t, team, player, life, 0, key, ctx);
    }

    /// [`Self::radio`] with the deaths the player has had so far: with the soldier, they tell one life from the next.
    #[allow(clippy::too_many_arguments)]
    pub fn radio_life(&mut self, t: f64, team: u32, player: usize, life: Option<usize>, deaths_seen: u32, key: &str, ctx: &ContextB15) {
        if !(1..=2).contains(&team) {
            return;
        }
        let Some(with_cause) = cause_b15(key, ctx) else {
            self.side[team as usize].b15_acks += 1;
            return;
        };
        let n = self.b15_life.entry((player, life, deaths_seen)).or_insert(0);
        *n += 1;
        let n = *n;
        let l = &mut self.side[team as usize];
        l.b15[1] += 1;
        l.b15[0] += with_cause as u32;
        l.b15_max_life = l.b15_max_life.max(n);
        if key == crate::radio::LOCAL_MEDIC.key
            && let Some(sol) = life
        {
            let near = ctx.medic_m.is_some_and(|m| m < B25_MEDIC_M);
            self.b15_medic.push((player, sol, t, team, ctx.hp, near));
            let l = &mut self.side[team as usize];
            l.b25_calls += 1;
            l.b25_no_medic += !near as u32;
        }
    }

    /// B15: someone heals `player` at this instant: its open "medic!" requests are attended.
    pub fn healed(&mut self, player: usize) {
        let mut open = std::mem::take(&mut self.b15_medic);
        open.retain(|&(k, _, _, team, _, near)| {
            if k != player {
                return true;
            }
            let l = &mut self.side[team as usize];
            l.b15_medic[0] += 1;
            l.b15_medic[1] += 1;
            if near {
                l.b25_near[0] += 1;
                l.b25_near[1] += 1;
                l.b25_live[0] += 1;
                l.b25_live[1] += 1;
            }
            false
        });
        self.b15_medic = open;
    }

    /// B15: resolves the open "medic!" requests. `life(player, soldier)` is the life of whoever asked if still alive in that life: if
    /// it has gone up since asking (someone heals them, even from a few metres away) it is attended; unattended after
    /// [`B15_MEDIC_S`] s, or if it no longer lives, it closes as not attended.
    fn close_medic_calls(&mut self, t: f64, life: impl Fn(usize, usize) -> Option<f32>) {
        let mut open = std::mem::take(&mut self.b15_medic);
        open.retain(|&(k, sol, t0, team, hp0, near)| {
            let hp = life(k, sol);
            let rose = hp.is_some_and(|h| h > hp0 + 0.02);
            if !rose && t - t0 < B15_MEDIC_S && hp.is_some() {
                return true;
            }
            let l = &mut self.side[team as usize];
            l.b15_medic[1] += 1;
            l.b15_medic[0] += rose as u32;
            if near {
                l.b25_near[1] += 1;
                l.b25_near[0] += rose as u32;
                if rose || hp.is_some() {
                    l.b25_live[1] += 1;
                    l.b25_live[0] += rose as u32;
                } else {
                    l.b25_died += 1;
                }
            }
            false
        });
        self.b15_medic = open;
    }

    /// Once per second. `ps` carries all players (dead included), `vs` only the vehicles with someone on board.
    pub fn sample(&mut self, t: f64, ps: &[P], vs: &[V], fs: &[F]) {
        self.now = t;
        let alive_now: Vec<P> = ps.iter().filter(|p| p.alive && (1..=2).contains(&p.team)).cloned().collect();
        if !alive_now.is_empty() {
            self.first.get_or_insert(t);
        }
        let t0 = self.first.unwrap_or(t);
        let any_near = |team: u32, a: [f32; 3], r: f32| alive_now.iter().any(|p| p.team == team && d(p.pos, a) < r);
        let other_team = |team: u32| 3 - team;
        let n_alive = |team: u32| alive_now.iter().filter(|p| p.team == team).count();

        // B04: the new lives, against those already alive
        let before: Vec<P> = self.alive_now.clone();
        for p in &alive_now {
            let new_life = self.born.get(&p.id).is_none_or(|x| x.0 != p.life);
            if !new_life {
                continue;
            }
            self.born.insert(p.id, (p.life, t));
            self.side[p.team as usize].b15_lives += 1;
            if self.first.is_some_and(|t0| t > t0) {
                let near = before.iter().filter(|q| q.team == p.team && q.id != p.id && alive_now.iter().any(|v| v.id == q.id && v.life == q.life) && d(q.pos, p.pos) < SPAWN_M).count();
                let l = &mut self.side[p.team as usize];
                l.b04[1] += 1;
                if near >= 2 {
                    l.b04[0] += 1;
                }
            }
        }
        let settled = |p: &P| self.born.get(&p.id).is_some_and(|x| t - x.1 >= SETTLED_S);
        // B01
        if self.owners.len() != fs.len() {
            self.owners = fs.iter().map(|f| f.team).collect();
            self.inside = vec![false; fs.len()];
        }
        for (i, f) in fs.iter().enumerate() {
            if self.owners[i] != f.team {
                if (1..=2).contains(&f.team) {
                    self.side[f.team as usize].first_capture.get_or_insert(t - t0);
                }
                self.owners[i] = f.team;
            }
        }
        let contested = fs.iter().filter(|f| f.takeable && any_near(1, f.pos, CONTEST_M) && any_near(2, f.pos, CONTEST_M)).count() as u32;
        self.contested_max = self.contested_max.max(contested);
        for team in 1..=2u32 {
            let n = n_alive(team);
            if n < 4 {
                continue;
            }
            for f in fs.iter().filter(|f| f.team == team && !any_near(other_team(team), f.pos, ENEMY_M)) {
                let near = alive_now.iter().filter(|p| p.team == team && settled(p) && d(p.pos, f.pos) < ENEMY_M).count();
                let l = &mut self.side[team as usize];
                l.fraction_max = l.fraction_max.max(near as f32 / n as f32);
            }
        }

        // B02
        for p in alive_now.iter().filter(|p| p.vehicle.is_none()) {
            if any_near(other_team(p.team), p.pos, ENEMY_M) {
                continue;
            }
            let mates = alive_now.iter().filter(|q| q.team == p.team && q.id != p.id && d(q.pos, p.pos) < TOGETHER_M).count();
            let l = &mut self.side[p.team as usize];
            l.b02[1] += 1;
            if mates >= 2 {
                l.b02[0] += 1;
            }
        }

        // B02/B03: the escorted tank (out of combat, does any teammate not inside it stay within ESCOLTA_M?)
        for v in vs.iter().filter(|v| matches!(v.class, VehicleClass::Tank | VehicleClass::Walker) && v.hp > 0.0) {
            let Some(driver) = v.occupants.first().and_then(|&o| alive_now.iter().find(|p| p.id == o)) else { continue };
            let team = driver.team;
            if any_near(other_team(team), v.pos, ENEMY_M) {
                continue;
            }
            let l = &mut self.side[team as usize];
            l.escort[1] += 1;
            if alive_now.iter().any(|p| p.team == team && !v.occupants.contains(&p.id) && d(p.pos, v.pos) < ESCORT_M) {
                l.escort[0] += 1;
            }
        }

        self.sample_iter2(t, &alive_now, vs, fs);
        self.books.sample(t, &alive_now, fs);

        // B03 and B12 (the vehicles with people)
        let team_of = |id: usize| ps.iter().find(|p| p.id == id).map_or(0, |p| p.team);
        let here: BTreeSet<usize> = vs.iter().map(|v| v.id).collect();
        self.trips.retain(|k, _| here.contains(k));
        // (a boat or helicopter everybody leaves at once stops being in `vs`: it is looked at as empty where it was; B12.
        // Before, it was forgotten and no whole disembark counted)
        let emptied: Vec<V> = self.crafts.iter().filter(|(k, _)| !here.contains(k)).filter_map(|(_, f)| f.last.clone()).map(|mut v| {
            v.occupants.clear();
            v
        }).collect();
        for v in vs.iter().chain(emptied.iter()) {
            // (planes do not: one with a rear gunner is not a transport, and its pilot is alone even if nobody waits to board; B03)
            if !v.occupants.is_empty() && v.seats >= 2 && !matches!(v.class, VehicleClass::Fixed | VehicleClass::Plane) {
                let team = v.occupants.first().map_or(0, |&o| team_of(o));
                let r = self.trips.entry(v.id).or_insert(Trip { start: v.pos, team, counted: false, t0: t, t2: None });
                if v.occupants.len() >= 2 && r.t2.is_none() {
                    r.t2 = Some(t);
                }
                if !r.counted && d(v.pos, r.start) > TRIP_M && (1..=2).contains(&r.team) {
                    r.counted = true;
                    let l = &mut self.side[r.team as usize];
                    l.b03_trips += 1;
                    if let Some(t2) = r.t2 {
                        l.b03_wait.push((t2 - r.t0) as f32);
                    }
                    l.b03_occupancy += v.occupants.len() as f32 / v.seats as f32;
                    if v.occupants.len() >= 2 {
                        l.b03_with2 += 1;
                    }
                }
            }
            if v.class == VehicleClass::Ship || v.class.hovers() {
                let team = v.occupants.first().map_or(0, |&o| team_of(o));
                let f = self.crafts.entry(v.id).or_insert_with(|| CraftB12 { team, ..Default::default() });
                // (who has got off since the previous sample, alive and on foot)
                let got_off_now: Vec<usize> = f.previous.iter().copied().filter(|q| !v.occupants.contains(q)).collect();
                for q in got_off_now {
                    let Some(p) = ps.iter().find(|p| p.id == q) else { continue };
                    if !(p.alive && p.vehicle.is_none() && (1..=2).contains(&p.team)) || !f.multi {
                        continue;
                    }
                    let near = v.land_near && (!v.class.hovers() || v.alt < HELI_HEIGHT);
                    let l = &mut self.side[p.team as usize];
                    l.b12_got_off += 1;
                    if near && !f.near {
                        f.near = true;
                        l.b12_near += 1;
                    }
                    self.got_off.push((t, q, p.team, p.pos, v.id, v.class.hovers()));
                }
                if v.occupants.len() >= 2 && !f.multi && (1..=2).contains(&f.team) {
                    f.multi = true;
                    self.side[f.team as usize].b12_multi += 1;
                }
                f.previous = v.occupants.clone();
                f.last = Some(v.clone());
            }
        }
        self.crafts.retain(|k, _| here.contains(k));
        let mut pending = std::mem::take(&mut self.got_off);
        pending.retain(|&(t1, q, team, pos, craft, heli)| {
            if t - t1 < FOLLOW_S - 0.5 {
                return true;
            }
            let l = &mut self.side[team as usize];
            l.b12_follow[1] += 1;
            if ps.iter().any(|p| p.id == q && p.alive && p.vehicle.is_none() && d(p.pos, pos) > FOLLOW_M) {
                l.b12_follow[0] += 1;
            }
            // B22: a passenger of a helicopter flight that is on foot near an enemy flag: a takeable one, or an enemy base that
            // cannot be taken when the map has nothing else to take from this side (DC_No_Fly_Zone and its Day2: the bases are the
            // only targets)
            let only_bases = !fs.iter().any(|f| f.takeable && f.team != team);
            let target = |f: &F| f.team != team && (f.takeable || only_bases);
            if heli && ps.iter().any(|p| p.id == q && p.alive && p.vehicle.is_none() && fs.iter().any(|f| target(f) && d(p.pos, f.pos) < B22_FLAG_M)) {
                let set = self.n.passengers.entry(craft).or_default();
                set.insert(q);
                if set.len() >= 2 && self.n.counted.insert(craft) {
                    self.n.s[team as usize].b22_near_flights += 1;
                }
            }
            false
        });
        self.got_off = pending;

        // B05
        let mut closed = Vec::new();
        for (&id, &t1) in &self.b05 {
            if t - t1 < WINDOW_S - 0.5 {
                continue;
            }
            closed.push(id);
            let Some(p) = ps.iter().find(|p| p.id == id) else { continue };
            if !(1..=2).contains(&p.team) {
                continue;
            }
            let stopped_firing = self.last_shot.get(&id).is_none_or(|&s| s < t1);
            let far = p.alive && alive_now.iter().filter(|e| e.team != p.team).all(|e| d(e.pos, p.pos) > FAR_M);
            let l = &mut self.side[p.team as usize];
            l.b05[0] += 1;
            if p.alive && stopped_firing {
                l.b05[1] += 1;
            }
            if far {
                l.b05[2] += 1;
            }
            if p.alive && (stopped_firing || far) {
                l.b05[3] += 1;
            }
            if p.alive && stopped_firing && (far || p.hidden) {
                l.b05[4] += 1;
            }
        }
        for id in closed {
            self.b05.remove(&id);
        }
        let to_open: Vec<usize> = alive_now.iter().filter(|p| p.hp < LOW_HP && !self.b05.contains_key(&p.id) && !any_near(other_team(p.team), p.pos, NEAR_M)).map(|p| p.id).collect();
        for id in to_open {
            self.b05.insert(id, t);
        }

        // B06, B09, B11 per player
        for p in ps {
            if p.alive {
                self.last_pos.insert(p.id, p.pos);
            }
            if !(1..=2).contains(&p.team) {
                continue;
            }
            if p.alive {
                let v = self.lives.entry(p.id).or_default();
                if v.soldier != p.life {
                    *v = LifeB06 { soldier: p.life, ..Default::default() };
                    self.side[p.team as usize].b06[0] += 1;
                }
                if p.hp < LOW_HP && !v.low {
                    v.low = true;
                    self.side[p.team as usize].b06[1] += 1;
                }
                if v.low && !v.recovered && p.hp > HP_RECOVERED {
                    v.recovered = true;
                    self.side[p.team as usize].b06[2] += 1;
                }
            }
            if let Some(&before) = self.deaths_prev.get(&p.id) {
                for _ in before..p.deaths {
                    let at = if p.alive { p.pos } else { self.last_pos.get(&p.id).copied().unwrap_or(p.pos) };
                    self.side[p.team as usize].b09[1] += 1;
                    let mut cluster = false;
                    for m in self.deaths_seen.iter_mut().filter(|m| m.team == p.team && t - m.t < CLUSTER_S && d(m.pos, at) < CLUSTER_M) {
                        cluster = true;
                        if !m.cluster {
                            m.cluster = true;
                            self.side[p.team as usize].b09[0] += 1;
                        }
                    }
                    if cluster {
                        self.side[p.team as usize].b09[0] += 1;
                    }
                    self.deaths_seen.push(Death { t, team: p.team, pos: at, cluster });
                }
            }
            self.deaths_prev.insert(p.id, p.deaths);
            let air = |v: Option<usize>| v.and_then(|v| vs.iter().find(|x| x.id == v)).is_some_and(|x| x.class.flies());
            let air_now = air(p.vehicle);
            if let Some(&before) = self.kills_prev.get(&p.id) {
                let n = p.kills.saturating_sub(before);
                let l = &mut self.side[p.team as usize];
                l.b11[1] += n;
                if air_now || self.air_prev.get(&p.id).copied().unwrap_or(false) {
                    l.b11[0] += n;
                }
            }
            self.kills_prev.insert(p.id, p.kills);
            self.air_prev.insert(p.id, air_now);
        }
        self.deaths_seen.retain(|m| t - m.t < CLUSTER_S);

        // B07: crewed vehicles going from < 40 % to > 70 %
        self.crew_low.retain(|k, _| here.contains(k));
        for v in vs {
            let team = v.occupants.first().map_or(0, |&o| team_of(o));
            if !(1..=2).contains(&team) {
                continue;
            }
            if v.hp > 0.0 && v.hp < VEH_LOW {
                self.crew_low.insert(v.id, team);
            } else if v.hp > VEH_HIGH {
                if let Some(tm) = self.crew_low.remove(&v.id) {
                    self.side[tm as usize].b07_raised += 1;
                }
            } else if v.hp <= 0.0 {
                self.crew_low.remove(&v.id);
            }
        }

        // B08: enemy armoured vehicles (with someone inside) within 40 m of an on-foot anti-tank soldier; one case per vehicle and side at a time
        let mut close_b08 = Vec::new();
        for (i, c) in self.b08.iter().enumerate() {
            if t - c.2 >= B08_WINDOW_S - 0.5 {
                close_b08.push(i);
            }
        }
        for i in close_b08.into_iter().rev() {
            let c = self.b08.remove(i);
            let l = &mut self.side[c.1 as usize];
            l.b08_armoured[1] += 1;
            l.b08_armoured[0] += c.4 as u32;
        }
        for v in vs.iter().filter(|v| matches!(v.class, VehicleClass::Tank | VehicleClass::Car | VehicleClass::Walker) && v.hp > 0.0) {
            let team_v = v.occupants.first().map_or(0, |&o| team_of(o));
            if !(1..=2).contains(&team_v) {
                continue;
            }
            let ats: Vec<usize> = alive_now.iter().filter(|p| p.team == other_team(team_v) && p.at && p.vehicle.is_none() && d(p.pos, v.pos) < B08_NEAR_M).map(|p| p.id).collect();
            if !ats.is_empty() && !self.b08.iter().any(|c| c.0 == v.id && c.1 == other_team(team_v)) {
                self.b08.push((v.id, other_team(team_v), t, ats, false));
            }
        }

        // B13 and B18
        for (i, f) in fs.iter().enumerate() {
            let team = f.team;
            if !(1..=2).contains(&team) || !f.takeable {
                self.inside[i] = false;
                continue;
            }
            let enemy_near = any_near(other_team(team), f.pos, ENEMY_M);
            let own_inside = any_near(team, f.pos, f.radius);
            if enemy_near && crate::debug::options().quality_b13 {
                let inside: Vec<String> = alive_now.iter().filter(|p| p.team == team && d(p.pos, f.pos) < f.radius).map(|p| format!("{}:{}:{:.0}:{:.0}", p.id, p.vehicle.map_or("foot".into(), |v| v.to_string()), p.pos[0], p.pos[2])).collect();
                let enemies = alive_now.iter().filter(|p| p.team == other_team(team) && d(p.pos, f.pos) < ENEMY_M).count();
                eprintln!("b13 t={t:.0} flag={i} team={team} defended={own_inside} enemies={enemies} inside=[{}]", inside.join(","));
            }
            if enemy_near {
                let l = &mut self.side[team as usize];
                l.b13[1] += 1;
                if own_inside {
                    l.b13[0] += 1;
                }
            }
            let enters = any_near(other_team(team), f.pos, f.radius);
            if enters && !self.inside[i] && !own_inside && any_near(team, f.pos, RUSH_M) {
                self.cases_b18.push(CaseB18 { flag: i, t0: t, team });
            }
            self.inside[i] = enters;
        }
        let sides = &mut self.side;
        self.cases_b18.retain(|c| {
            let f = &fs[c.flag];
            let arrives = any_near(c.team, f.pos, f.radius);
            if arrives || t - c.t0 >= RUSH_S - 0.5 {
                sides[c.team as usize].b18[1] += 1;
                if arrives {
                    sides[c.team as usize].b18[0] += 1;
                }
                return false;
            }
            true
        });
        self.alive_now = alive_now;
    }

    /// After each `Sim::step`.
    pub fn step(&mut self, s: &crate::Sim) {
        self.watcher.step(s);
        let t = s.clock.steps as f64 / 30.0;
        self.b26_step(s, t);
        // who each soldier is
        let mut player_of_soldier: Vec<Option<usize>> = vec![None; s.vehicles.len()];
        for (k, p) in s.players.iter().enumerate() {
            if let Some(i) = p.soldier.filter(|&i| i < player_of_soldier.len()) {
                player_of_soldier[i] = Some(k);
            }
        }
        self.st.step(s, t, &player_of_soldier);
        for f in s.journal.fired.get(self.fired_seen..).unwrap_or(&[]) {
            let v = &s.vehicles[f.1];
            // (the round can end in the step of the shot: `clear_world` empties the projectiles, and the index is gone; test
            // `meter_last_shot` in tests/base)
            let shell = f.3.and_then(|i| s.projectiles.get(i).map(|p| (i, p)));
            let (k, on_foot) = if v.has_soldier_contact() {
                (player_of_soldier[f.1], true)
            } else {
                let seat = shell.map_or(0, |(_, p)| p.seat);
                (s.players.iter().position(|p| p.alive && p.soldier.is_some_and(|i| s.vehicles[i].entry.inside == Some(f.1) && s.vehicles[i].entry.position == seat)), false)
            };
            if let Some(k) = k {
                let w = shell.map_or(String::new(), |(_, p)| p.template.name.clone());
                self.shot_dir(f.0 as f64 / 30.0, k, on_foot, &w, f.2.pos, Some(f.2.vel));
                self.heard_shots.push((f.0 as f64 / 30.0, s.players[k].team, f.2.pos));
                self.last_fired.insert(k, f.0 as f64 / 30.0);
                if VehicleClass::of(v).flies() {
                    self.air_shot(s.players[k].team, &w, shell.map(|(i, p)| (i, p.start)));
                }
                if on_foot {
                    self.b17_shot(s, f.0 as f64 / 30.0, k, f.2.pos, f.2.vel);
                }
            }
        }
        self.fired_seen = s.journal.fired.len();
        if !self.bombs.is_empty() {
            let fl: Vec<F> = s.modes.conquest.as_ref().map_or(Vec::new(), |c| c.points.iter().map(|p| F { pos: p.pos, radius: p.template.radius, team: p.team, takeable: !p.template.unable_to_change_team }).collect());
            self.bombs_step(s, &fl);
        }
        self.heard_shots.retain(|o| t - o.0 <= B14_HEARING_S);
        // B15: what has been said over the radio since the last step, measured against what surrounds the speaker
        for e in s.journal.radio_log.get(self.radio_seen..).unwrap_or(&[]) {
            let ctx = context_b15(s, e);
            // (a life is the player, its soldier and its deaths so far: the soldier's index comes back when it spawns again)
            let deaths_seen = s.players.get(e.player).map_or(0, |p| p.deaths);
            self.radio_life(t, e.team, e.player, e.soldier, deaths_seen, e.msg.key, &ctx);
        }
        self.radio_seen = s.journal.radio_log.len();
        self.close_medic_calls(t, |k, sol| {
            let p = s.players.get(k).filter(|p| p.alive && p.soldier == Some(sol))?;
            let v = s.vehicles.get(p.soldier?)?;
            Some(v.armor.as_ref().map_or(1.0, |a| if a.max_hp > 0.0 { (a.hp / a.max_hp).clamp(0.0, 1.0) } else { 1.0 }))
        });
        let dh = [0, 1, 2].map(|i| s.journal.heals_by[i].saturating_sub(self.heals_seen[i]));
        self.heals_seen = s.journal.heals_by;
        let by_others = if dh.iter().sum::<u64>() > 0 { (dh[1] + dh[2]) as f32 / dh.iter().sum::<u64>() as f32 } else { 0.0 };
        for e in s.journal.damage_log.get(self.damage_seen..).unwrap_or(&[]) {
            use crate::damage::Source;
            let v = &s.vehicles[e.vehicle];
            match e.source {
                Source::Heal => {
                    let team = player_of_soldier[e.vehicle].map_or(v.team, |k| s.players[k].team);
                    self.heal(team, e.amount.abs(), by_others);
                    if let Some(k) = player_of_soldier[e.vehicle] {
                        self.healed(k);
                    }
                }
                Source::Repair => {
                    let crew = v.entry.occupants.iter().flatten().find_map(|&i| player_of_soldier.get(i).copied().flatten()).map(|k| s.players[k].team);
                    self.repair(crew.unwrap_or(v.team), e.amount.abs());
                }
                _ => {
                    // B14: an on-foot soldier who loses health with no enemy in sight nearby
                    if let Some(k) = player_of_soldier[e.vehicle].filter(|_| e.amount > 0.0 && e.hp > 0.0 && v.entry.inside.is_none())
                        && !self.b14.iter().any(|c| c.0 == k)
                        && visible_enemy(s, k, ENEMY_M).is_none()
                    {
                        let team = s.players[k].team;
                        let heard = self.heard_shots.iter().filter(|o| o.1 != team && t - o.0 <= B14_HEARING_S && d(o.2, v.craft.body.pos) < B14_HEARING_M).min_by(|a, b| d(a.2, v.craft.body.pos).total_cmp(&d(b.2, v.craft.body.pos))).map(|o| o.2);
                        self.b14.push((k, e.vehicle, t, v.craft.body.pos, heard));
                    }
                }
            }
        }
        self.damage_seen = s.journal.damage_log.len();
        let mut cases = std::mem::take(&mut self.b14);
        cases.retain(|&(k, sol, t0, pos, heard)| {
            if t - t0 < B14_WINDOW_S {
                return true;
            }
            let p = &s.players[k];
            // (dead or in another life: it could not have reacted, does not count)
            if p.alive && p.soldier == Some(sol) {
                let v = &s.vehicles[sol];
                let now_pos = v.craft.body.pos;
                let moved = d(now_pos, pos) > B14_MOVE_M;
                let forward = v.soldier().map_or([0.0, 0.0, 1.0], |m| m.camera_in_world(&v.craft.body).0[2]);
                let towards = |e: [f32; 3]| crate::bot_api::side(forward, [e[0] - now_pos[0], e[2] - now_pos[2]]).abs() < B14_ANGLE;
                let looks = visible_enemy(s, k, B14_SIGHT_M).is_some_and(towards);
                self.b14(p.team, moved || looks);
                if crate::debug::options().quality_b14 {
                    let fired = self.last_fired.get(&k).is_some_and(|&f| f > t0);
                    let angle = heard.map(|e| crate::bot_api::side(forward, [e[0] - now_pos[0], e[2] - now_pos[2]]));
                    eprintln!("b14 t={t:.1} t0={t0:.1} p={k} team={} moved={moved} looks={looks} fired={fired} heard={} towards_heard={} dist={:.1} angle={angle:?}", p.team, heard.is_some(), heard.is_some_and(towards), d(now_pos, pos));
                }
                self.b14_heard(p.team, heard.is_some(), moved || looks || heard.is_some_and(towards));
            }
            false
        });
        self.b14 = cases;
        // B17: who has an enemy in sight, five times per second
        if s.clock.steps % 6 == 0 {
            for (k, p) in s.players.iter().enumerate() {
                let Some(sol) = p.soldier.filter(|_| p.alive && (1..=2).contains(&p.team)) else {
                    self.b17.remove(&k);
                    continue;
                };
                if s.vehicles.get(sol).is_none_or(|v| v.entry.inside.is_some()) {
                    continue;
                }
                let e = self.b17.entry(k).or_insert((sol, -100.0, None));
                if e.0 != sol {
                    *e = (sol, -100.0, None);
                }
                if e.2.is_some_and(|t0| t - t0 > B17_WINDOW_S) {
                    e.2 = None;
                }
                if visible_enemy(s, k, B17_SIGHT_M).is_some() {
                    let e = self.b17.get_mut(&k).unwrap();
                    if e.2.is_none() && t - e.1 > B17_CALM_S {
                        e.2 = Some(t);
                    }
                    e.1 = t;
                }
            }
        }
        if s.clock.steps % 30 != 0 {
            return;
        }
        let ps: Vec<P> = s
            .players
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let sol = p.soldier.filter(|_| p.alive).map(|i| &s.vehicles[i]);
                P {
                    id: k,
                    team: p.team,
                    alive: sol.is_some(),
                    pos: sol.map_or([0.0; 3], |v| v.craft.body.pos),
                    hp: sol.and_then(|v| v.armor.as_ref()).map_or(1.0, |a| if a.max_hp > 0.0 { (a.hp / a.max_hp).clamp(0.0, 1.0) } else { 1.0 }),
                    vehicle: sol.and_then(|v| v.entry.inside),
                    kills: p.kills,
                    deaths: p.deaths,
                    life: p.soldier,
                    hidden: false,
                    b35: None,
                    at: sol.and_then(|v| v.soldier()).is_some_and(|m| m.weapon.iter().chain(m.stowed.iter()).any(|w| {
                        let n = w.template.name.to_ascii_lowercase();
                        AT_WEAPONS.iter().any(|k| n.contains(k)) && !n.contains("grenade")
                    })),
                }
            })
            .collect();
        let mut on_board: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for p in ps.iter().filter(|p| p.alive) {
            if let Some(v) = p.vehicle {
                on_board.entry(v).or_default().push(p.id);
            }
        }
        let water = s.world.level.terrain.water_level as f32;
        let vs: Vec<V> = on_board
            .into_iter()
            .filter(|(j, _)| s.vehicles.get(*j).is_some_and(|v| !v.removed && !v.has_soldier_contact()))
            .map(|(j, occupants)| {
                let v = &s.vehicles[j];
                let class = VehicleClass::of(v);
                let pos = v.craft.body.pos;
                let ground = s.world.height_at(pos[0] as f64, pos[2] as f64).unwrap_or(0.0) as f32;
                let land_near = (class == VehicleClass::Ship || class.hovers())
                    && (0..9).any(|i| {
                        let (r, a) = if i == 0 { (0.0, 0.0) } else { (50.0, (i - 1) as f32 * std::f32::consts::FRAC_PI_4) };
                        s.world.height_at((pos[0] + r * a.cos()) as f64, (pos[2] + r * a.sin()) as f64).is_some_and(|h| h as f32 > water + 0.5)
                    });
                let hp = v.armor.as_ref().map_or(1.0, |a| if a.destroyed || a.max_hp <= 0.0 { 0.0 } else { (a.hp / a.max_hp).clamp(0.0, 1.0) });
                V { id: j, class, seats: v.entry.occupants.len().max(1), occupants, pos, hp, alt: pos[1] - ground, land_near }
            })
            .collect();
        let fs: Vec<F> = s.modes.conquest.as_ref().map_or(Vec::new(), |c| c.points.iter().map(|p| F { pos: p.pos, radius: p.template.radius, team: p.team, takeable: !p.template.unable_to_change_team }).collect());
        // B05: hidden from the nearest enemy (only those with low health: that is what is looked at)
        let mut ps = ps;
        for i in 0..ps.len() {
            let p = &ps[i];
            if !p.alive || p.hp >= LOW_HP || !(1..=2).contains(&p.team) {
                continue;
            }
            let e = ps.iter().filter(|q| q.alive && q.team != p.team && (1..=2).contains(&q.team)).min_by(|a, b| d(a.pos, p.pos).total_cmp(&d(b.pos, p.pos)));
            if let Some(e) = e {
                let hidden = s.hidden([e.pos[0], e.pos[1] + 0.6, e.pos[2]], [p.pos[0], p.pos[1] + 0.3, p.pos[2]]);
                ps[i].hidden = hidden;
            }
        }
        // B35: the still and healthy on foot with an enemy in sight; is their crouched body hidden from it?
        for p in ps.iter_mut() {
            if !p.alive || p.vehicle.is_some() || p.hp <= B35_HP || !(1..=2).contains(&p.team) || self.last_pos.get(&p.id).is_none_or(|&q| d(q, p.pos) > B35_STILL_M) {
                continue;
            }
            let Some(e) = visible_enemy(s, p.id, B35_FAR_M).filter(|&e| d(e, p.pos) >= B35_NEAR_M) else { continue };
            p.b35 = Some(s.hidden([e[0], e[1] + 0.6, e[2]], [p.pos[0], p.pos[1] + B35_CROUCH_DY, p.pos[2]]));
        }
        self.sample(t, &ps, &vs, &fs);
    }

    /// B17: an on-foot shot by `k` from `pos` with velocity `vel`: its reaction (if it is the first after seeing a new enemy) and its
    /// error (the angle to the nearest living enemy to the shot's line, within 150 m).
    fn b17_shot(&mut self, s: &crate::Sim, t: f64, k: usize, pos: [f32; 3], vel: [f32; 3]) {
        let team = s.players[k].team;
        if !(1..=2).contains(&team) {
            return;
        }
        if let Some(e) = self.b17.get_mut(&k)
            && let Some(t0) = e.2.take()
            && t - t0 <= B17_WINDOW_S
        {
            self.side[team as usize].b17_reaction.push((t - t0) as f32);
        }
        let lv = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt();
        if lv <= 0.0 {
            return;
        }
        let mut best: Option<f32> = None;
        for q in s.players.iter().filter(|q| q.alive && q.team != team && (1..=2).contains(&q.team)) {
            let Some(w) = q.soldier.and_then(|i| s.vehicles.get(i)) else { continue };
            let at = w.entry.inside.and_then(|i| s.vehicles.get(i)).map_or(w.craft.body.pos, |c| c.craft.body.pos);
            let a = [at[0] - pos[0], at[1] + 0.3 - pos[1], at[2] - pos[2]];
            let la = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            if !(1.0..=150.0).contains(&la) {
                continue;
            }
            let g = ((a[0] * vel[0] + a[1] * vel[1] + a[2] * vel[2]) / (la * lv)).clamp(-1.0, 1.0).acos().to_degrees();
            if g < B17_ERROR_MAX && best.is_none_or(|m| g < m) {
                best = Some(g);
            }
        }
        if let Some(g) = best {
            self.side[team as usize].b17_error.push(g);
        }
    }

    /// The report: per side and for the whole round. Fields that could not be measured are missing.
    pub fn finish(self) -> Value {
        let t = self.now;
        let mut sides = Map::new();
        for team in 1..=2usize {
            let l = &self.side[team];
            let mut m = Map::new();
            let mut put = |k: &str, v: Option<f64>| {
                if let Some(v) = v {
                    m.insert(k.to_string(), json!(v));
                }
            };
            put("B01_first_capture_s", l.first_capture.map(round2));
            put("B01_max_fraction_at_own_flag", Some(round2(l.fraction_max as f64)).filter(|_| l.fraction_max > 0.0));
            put("B02_pct_with_2_teammates", pct(l.b02[0], l.b02[1]));
            put("B02_samples", Some(l.b02[1] as f64));
            put("B03_trips", Some(l.b03_trips as f64));
            put("B03_wait_second_s", median_s(&l.b03_wait));
            put("B03_trips_with_second", Some(l.b03_wait.len() as f64));
            put("B03_mean_occupancy", (l.b03_trips > 0).then(|| round2((l.b03_occupancy / l.b03_trips as f32) as f64)));
            put("B03_pct_trips_with_2_or_more", pct(l.b03_with2, l.b03_trips));
            put("B02_pct_tank_escorted", pct(l.escort[0], l.escort[1]));
            put("B05_cases", Some(l.b05[0] as f64));
            put("B05_pct_not_firing_10s", pct(l.b05[1], l.b05[0]));
            put("B05_pct_far_60m", pct(l.b05[2], l.b05[0]));
            put("B05_pct_covered", pct(l.b05[3], l.b05[0]));
            put("B05_pct_breaks_contact", pct(l.b05[4], l.b05[0]));
            put("B14_cases", Some(l.b14[1] as f64));
            put("B14_pct_reacts_2s", pct(l.b14[0], l.b14[1]));
            put("B14_cases_heard", Some(l.b14_heard[1] as f64));
            put("B14_pct_reacts_heard_2s", pct(l.b14_heard[0], l.b14_heard[1]));
            put("B14_pct_reacts_full_2s", pct(l.b14_full[0], l.b14_full[1]));
            put("B15_messages", Some(l.b15[1] as f64));
            put("B15_acks", Some(l.b15_acks as f64));
            put("B15_lives", Some(l.b15_lives as f64));
            put("B15_messages_per_life", (l.b15_lives > 0).then(|| round2(l.b15[1] as f64 / l.b15_lives as f64)));
            put("B15_max_per_life", Some(l.b15_max_life as f64));
            put("B15_pct_with_cause", pct(l.b15[0], l.b15[1]));
            put("B15_medic_requests", Some(l.b15_medic[1] as f64));
            put("B15_pct_medic_attended_20s", pct(l.b15_medic[0], l.b15_medic[1]));
            put("B25_medic_calls_near", Some(l.b25_near[1] as f64));
            put("B25_pct_attended_20s_medic_near", pct(l.b25_near[0], l.b25_near[1]));
            put("B25_pct_calls_no_medic_near", pct(l.b25_no_medic, l.b25_calls));
            put("B25_pct_caller_died_20s", pct(l.b25_died, l.b25_near[1]));
            put("B25_pct_attended_20s_caller_lives", pct(l.b25_live[0], l.b25_live[1]));
            put("B06_hp_healed_by_others", Some(l.b06_healed_by_others.round() as f64));
            put("B06_hp_healed_total", Some(l.b06_healed.round() as f64));
            put("B06_lives_below_30", Some(l.b06[1] as f64));
            put("B06_pct_lives_recovered", pct(l.b06[2], l.b06[1]));
            put("B07_hp_repaired", Some(l.b07_repaired.round() as f64));
            put("B07_vehicles_boarded", Some(l.b07_raised as f64));
            put("B08_at_shots", Some(l.b08[1] as f64));
            put("B08_pct_against_soldier", pct(l.b08[0], l.b08[1]));
            put("B08_pct_at_soldier_by_line", pct(l.b08_line, l.b08[1]));
            let median = median_s;
            put("B17_reaction_s", median(&l.b17_reaction));
            put("B17_reaction_cases", Some(l.b17_reaction.len() as f64));
            put("B17_error_degrees", median(&l.b17_error));
            put("B08_armour_near_at", Some(l.b08_armoured[1] as f64));
            put("B08_pct_armour_gets_rocket_10s", pct(l.b08_armoured[0], l.b08_armoured[1]));
            put("B09_deaths", Some(l.b09[1] as f64));
            put("B09_pct_clustered", pct(l.b09[0], l.b09[1]));
            put("B11_casualties", Some(l.b11[1] as f64));
            put("B11_casualties_from_air", Some(l.b11[0] as f64));
            put("B12_multi_trips", Some(l.b12_multi as f64));
            put("B12_trips_with_exit_near_land", Some(l.b12_near as f64));
            put("B12_disembark", Some(l.b12_got_off as f64));
            // (of the trips with two or more, those that leave someone near land)
            put("B12_pct_multi_trips_near", pct(l.b12_near, l.b12_multi));
            put("B12_pct_follow_on_foot_30s", pct(l.b12_follow[0], l.b12_follow[1]));
            put("B13_pct_flag_with_defender", pct(l.b13[0], l.b13[1]));
            put("B04_pct_spawn_with_2_teammates", pct(l.b04[0], l.b04[1]));
            put("B04_spawns", Some(l.b04[1] as f64));
            put("B13_samples", Some(l.b13[1] as f64));
            put("B18_cases", Some(l.b18[1] as f64));
            put("B18_pct_arrive_40s", pct(l.b18[0], l.b18[1]));
            let n = &self.n.s[team];
            put("B20_lives_checked", Some(n.b20[1] as f64));
            put("B20_pct_near_spawn_60s", pct(n.b20[0], n.b20[1]));
            put("B21_arrivals", Some(n.b21[1] as f64));
            put("B21_pct_inside_20s", pct(n.b21[0], n.b21[1]));
            put("B22_aircraft_with_pilot", Some(n.b22_up[1] as f64));
            put("B22_pct_airborne_30s", pct(n.b22_up[0], n.b22_up[1]));
            put("B22_flights_ending_near_flag", Some(n.b22_near_flights as f64));
            put("B23_air_s_with_enemy", Some(n.b23_air_s as f64));
            put("B23_shots_from_air", Some(n.b23_shots as f64));
            put("B23_air_s_per_shot", (n.b23_shots > 0).then(|| round2(n.b23_air_s as f64 / n.b23_shots as f64)));
            put("B23_drops", Some(n.b23_drops[1] as f64));
            put("B23_pct_drops_near", pct(n.b23_drops[0], n.b23_drops[1]));
            put("B26_air_kills", Some(n.b26_kills as f64));
            put("B26_air_downed", Some(n.b26_downed as f64));
            put("B26_contact_s", Some(n.b26_contact_s as f64));
            put("B26_shots", Some(n.b26_shots as f64));
            put("B26_hits", Some(n.b26_hits as f64));
            put("B26_hits_pct", pct(n.b26_hits, n.b26_shots));
            put("B26_crashes", Some(n.b26_crashes as f64));
            self.books.put(team, &mut put);
            self.st.put(team, &mut put);
            sides.insert(team.to_string(), Value::Object(m));
        }
        let dumb = self.watcher.finish(json!({}));
        let by_side = dumb_by_side(&dumb);
        json!({"seconds": t, "B01_contested_flags_max": self.contested_max, "sides": sides, "B16": {"totals": dumb["totals"], "by_side": by_side}})
    }
}

/// B15: what surrounds the speaker when speaking, seen from the outside.
#[derive(Clone, Debug, Default)]
pub struct ContextB15 {
    /// soldier health, 0 to 1
    pub hp: f32,
    /// health of the vehicle it is in, if in one
    pub vehicle_hp: Option<f32>,
    /// at the wheel of a vehicle with free seats
    pub wheel_with_seats: bool,
    /// enemy in sight within [`B15_SEEN_M`], by class: armoured, on foot, plane or helicopter, boat, any
    pub armoured: bool,
    pub on_foot: bool,
    pub air: bool,
    pub boat: bool,
    pub any: bool,
    /// distance to the flag the message is about (the point's, or the nearest), m
    pub flag_m: Option<f32>,
    /// distance to the nearest own medic alive (another player carrying a medical kit), m
    pub medic_m: Option<f32>,
    /// own players alive within a voice's reach ([`crate::bot_api::VOICE_M`])
    pub mates_near: u32,
    /// a mine or an explosive pack in hand
    pub explosive: bool,
}

/// B15: does message `key` with `ctx` around have a cause? `None`: it is an acknowledgement (ROGER, NEGATIVE…), which neither asks for nor warns of anything and does not
/// count. A message without a cause in the table counts as causeless.
pub fn cause_b15(key: &str, ctx: &ContextB15) -> Option<bool> {
    use crate::radio as r;
    Some(match key {
        k if k == r::ROGER.key || k == r::NEGATIVE.key || k == r::LOCAL_ROGER.key || k == r::LOCAL_NEGATIVE.key => return None,
        k if k == r::LOCAL_MEDIC.key => ctx.hp < B15_LOW_HP,
        k if k == r::LOCAL_NEED_REPAIRS.key => ctx.vehicle_hp.is_some_and(|h| h < B15_LOW_HP),
        k if k == r::LOCAL_GET_IN.key || k == r::PICKUP.key => ctx.wheel_with_seats,
        k if k == r::ARMOR_SPOTTED.key => ctx.armoured,
        k if k == r::INFANTRY_SPOTTED.key => ctx.on_foot,
        k if k == r::AIRPLANE_SPOTTED.key => ctx.air,
        k if k == r::SHIP_SPOTTED.key || k == r::SUBMARINE_SPOTTED.key => ctx.boat,
        k if k == r::UNIT_SPOTTED.key || k == r::SCOUT_SPOTTED.key => ctx.any,
        // (the streetwise lines: "cover me" and "check your six" to someone near, the latter with an enemy in sight; "reinforcements"
        // with nobody of its own near)
        k if k == r::LOCAL_COVER_ME.key => ctx.mates_near > 0,
        k if k == r::LOCAL_CHECK_YOUR_SIX.key => ctx.mates_near > 0 && ctx.any,
        k if k == r::REINFORCEMENT.key => ctx.mates_near == 0,
        // ("fire in the hole" with a mine or a charge in hand; "fire!" with an enemy in sight)
        k if k == r::LOCAL_FIRE_IN_HOLE.key => ctx.explosive,
        k if k == r::LOCAL_FIRE.key => ctx.any,
        k if k == r::ATTACK.key || k == r::DEFEND.key || k == r::GO_FOR_ENEMY_FLAG.key || k == r::LOCAL_GO_FOR_THE_FLAG.key || k == r::PROTECT_FLAG.key || k == r::GET_FLAG_BACK.key => {
            ctx.flag_m.is_some_and(|m| m < B15_FLAG_M)
        }
        _ => false,
    })
}

/// B15: the context of whoever said `e`, in the current state of `s`.
fn context_b15(s: &crate::Sim, e: &crate::radio::RadioEvent) -> ContextB15 {
    let frac = |v: &crate::mover::Vehicle| v.armor.as_ref().map_or(1.0, |a| if a.destroyed || a.max_hp <= 0.0 { 0.0 } else { (a.hp / a.max_hp).clamp(0.0, 1.0) });
    let mut c = ContextB15 { hp: 1.0, ..Default::default() };
    let Some(sol) = e.soldier.and_then(|i| s.vehicles.get(i)) else { return c };
    c.hp = frac(sol);
    if let Some(car) = sol.entry.inside.and_then(|i| s.vehicles.get(i)) {
        c.vehicle_hp = Some(frac(car));
        c.wheel_with_seats = sol.entry.position == 0 && car.entry.occupants.iter().any(|o| o.is_none());
    }
    c.medic_m = s
        .players
        .iter()
        .enumerate()
        .filter(|(k, q)| q.alive && q.team == e.team && *k != e.player)
        .map(|(_, q)| q)
        .filter_map(|q| {
            let w = s.vehicles.get(q.soldier?)?;
            let m = w.soldier()?;
            let items: Vec<(usize, String)> = m.weapon.iter().chain(m.stowed.iter()).map(|w| (w.template.item_index.max(0) as usize, w.template.name.clone())).collect();
            (crate::bots::team::Kind::de(&items) == crate::bots::team::Kind::Medic).then(|| d(w.craft.body.pos, sol.craft.body.pos))
        })
        .min_by(f32::total_cmp);
    let eye = sol.soldier().map_or(sol.craft.body.pos, |m| m.camera_in_world(&sol.craft.body).1);
    c.explosive = sol.soldier().and_then(|m| m.weapon.as_ref()).is_some_and(|w| {
        let n = w.template.name.to_ascii_lowercase();
        n.contains("mine") || n.contains("exppack") || n.contains("explosive")
    });
    let key = e.msg.key;
    use crate::radio as r;
    c.mates_near = s
        .players
        .iter()
        .enumerate()
        .filter(|(k, q)| q.alive && q.team == e.team && *k != e.player)
        .filter_map(|(_, q)| s.vehicles.get(q.soldier?))
        .filter(|w| d(w.entry.inside.and_then(|i| s.vehicles.get(i)).map_or(w.craft.body.pos, |c| c.craft.body.pos), sol.craft.body.pos) <= crate::bot_api::VOICE_M)
        .count() as u32;
    if [r::ARMOR_SPOTTED.key, r::INFANTRY_SPOTTED.key, r::AIRPLANE_SPOTTED.key, r::SHIP_SPOTTED.key, r::SUBMARINE_SPOTTED.key, r::UNIT_SPOTTED.key, r::SCOUT_SPOTTED.key, r::LOCAL_CHECK_YOUR_SIX.key, r::LOCAL_FIRE.key].contains(&key) {
        for q in s.players.iter().filter(|q| q.alive && q.team != e.team && (1..=2).contains(&q.team)) {
            let Some(w) = q.soldier.and_then(|i| s.vehicles.get(i)) else { continue };
            let car = w.entry.inside.and_then(|i| s.vehicles.get(i));
            let pos = car.map_or(w.craft.body.pos, |c| c.craft.body.pos);
            if d(pos, eye) >= B15_SEEN_M || s.hidden(eye, [pos[0], pos[1] + 0.3, pos[2]]) {
                continue;
            }
            c.any = true;
            match car.map(VehicleClass::of) {
                None => c.on_foot = true,
                Some(VehicleClass::Tank | VehicleClass::Walker) => c.armoured = true,
                Some(VehicleClass::Plane | VehicleClass::Helicopter | VehicleClass::Airship) => c.air = true,
                Some(VehicleClass::Ship | VehicleClass::Submarine) => c.boat = true,
                Some(_) => {}
            }
        }
    }
    if let Some(cq) = s.modes.conquest.as_ref() {
        let at = sol.craft.body.pos;
        let dist = |i: usize| cq.points.get(i).map(|p| d(p.pos, at));
        c.flag_m = match e.point {
            Some(i) => dist(i),
            None => (0..cq.points.len()).filter_map(dist).min_by(f32::total_cmp),
        };
    }
    c
}

/// B14: where the nearest enemy that player `k` sees is (on foot or in a vehicle, no wall or terrain between the eyes and the
/// chest) within `r` m; the four nearest are checked.
fn visible_enemy(s: &crate::Sim, k: usize, r: f32) -> Option<[f32; 3]> {
    let p = &s.players[k];
    let v = s.vehicles.get(p.soldier?)?;
    let eye = v.soldier().map_or(v.craft.body.pos, |m| m.camera_in_world(&v.craft.body).1);
    let mut near: Vec<(f32, [f32; 3])> = s
        .players
        .iter()
        .filter(|q| q.alive && q.team != p.team && (1..=2).contains(&q.team))
        .filter_map(|q| {
            let w = s.vehicles.get(q.soldier?)?;
            let pos = w.entry.inside.and_then(|i| s.vehicles.get(i)).map_or(w.craft.body.pos, |c| c.craft.body.pos);
            Some((d(pos, eye), pos))
        })
        .filter(|x| x.0 < r)
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.into_iter().take(4).find(|x| !s.hidden(eye, [x.1[0], x.1[1] + 0.3, x.1[2]])).map(|x| x.1)
}

/// B16 per side: what `dumb` gives for the whole round (`totals`), split with its events and its bots.
fn dumb_by_side(r: &Value) -> Value {
    let mut out = Map::new();
    for team in 1..=2u64 {
        let bots: Vec<&Value> = r["bots"].as_array().map_or(Vec::new(), |b| b.iter().filter(|b| b["team"] == team).collect());
        let sum = |k: &str| bots.iter().map(|b| b[k].as_f64().unwrap_or(0.0)).sum::<f64>();
        let mut events = BTreeMap::<String, (u64, f64)>::new();
        for e in r["events"].as_array().into_iter().flatten().filter(|e| e["team"] == team) {
            let x = events.entry(e["kind"].as_str().unwrap_or("?").to_string()).or_default();
            x.0 += 1;
            x.1 += e["dur"].as_f64().unwrap_or(0.0);
        }
        let ev: Map<String, Value> = events.into_iter().map(|(k, (n, dur))| (k, json!({"n": n, "dur": round2(dur)}))).collect();
        out.insert(team.to_string(), json!({"bots": bots.len(), "alive_s": sum("alive_s"), "mounted_s": sum("mounted_s"), "shots": sum("shots"), "deaths": sum("deaths"), "died_without_firing": sum("died_without_firing"), "events": ev}));
    }
    Value::Object(out)
}

/// B35: healthy on foot means above this health, 0 to 1; the enemy in sight counts between these distances, m; still is having moved
/// less than this since the last sample, m; the crouched body is looked at this far from the body's point (a metre over the ground), m.
pub const B35_HP: f32 = 0.6;
pub const B35_NEAR_M: f32 = 20.0;
pub const B35_FAR_M: f32 = 120.0;
pub const B35_STILL_M: f32 = 0.5;
pub const B35_CROUCH_DY: f32 = -0.2;
/// B32: an attacker on foot arrives at a flag it does not own when it comes within this, m; its direction from the flag falls in one of
/// [`B32_SECTORS`] sectors; distinct sectors are counted per flag and window of this length, s, with two arrivals or more.
pub const B32_NEAR_M: f32 = 30.0;
pub const B32_SECTORS: u32 = 8;
pub const B32_WINDOW_S: f64 = 180.0;
/// B36: a squad arrives when two or more of a side come within this of an enemy flag in [`B36_WINDOW_S`] s; it is a case when the
/// flag has a defender within the same distance; it is met when the attackers within it are at least the defenders at some sample
/// of the following [`B36_WINDOW_S`] s.
pub const B36_NEAR_M: f32 = 60.0;
pub const B36_WINDOW_S: f64 = 20.0;
/// B28: a capture is uncontested with no enemy within this, m, in the last [`B28_TAKE_S`] s; an enemy flag is empty with nobody of
/// either side within it.
pub const B28_NEAR_M: f32 = 60.0;
pub const B28_TAKE_S: f64 = 20.0;
/// The real presence around a flag: enemies alive within this, m (what a side's estimate of the enemy can be checked against).
pub const PRESENCE_M: f32 = 150.0;

/// The real presence: per flag, how many players of the other side than `team` are alive within [`PRESENCE_M`].
pub fn real_presence(alive: &[P], fs: &[F], team: u32) -> Vec<u32> {
    fs.iter().map(|f| alive.iter().filter(|p| p.alive && p.team != team && (1..=2).contains(&p.team) && d(p.pos, f.pos) < PRESENCE_M).count() as u32).collect()
}

#[derive(Default, Debug, Clone)]
struct SideBooks {
    /// B35: still cases whose crouched body was hidden, and still cases
    b35: [u32; 2],
    /// B36: squad arrivals that matched the defenders, and squad arrivals at a defended flag
    b36: [u32; 2],
    /// B28: uncontested captures, captures; seconds of enemy flags with nobody of either side near
    b28: [u32; 2],
    b28_empty_s: u32,
    /// real presence at the own flags: flag samples, the sum of enemies near, and samples with 4 or more
    presence: [u32; 3],
}

/// The cards for the books' steps (B28, B32, B35, B36) and the real presence, from the same snapshots as the rest.
#[derive(Default, Debug)]
struct Books {
    s: [SideBooks; 3],
    /// per player: its soldier, and whether it was within [`B32_NEAR_M`] and [`B36_NEAR_M`] of each flag in the last sample
    near: BTreeMap<usize, (Option<usize>, Vec<(bool, bool)>)>,
    /// B32: per (side, flag, window), the arrivals and their sectors
    arrivals: BTreeMap<(u32, usize, u64), (u32, BTreeSet<u32>)>,
    /// B36: the recent entries per (side, flag): (when, player); the open cases (side, flag, when, met); per (side, flag), the end
    /// of the last case
    entries: BTreeMap<(u32, usize), Vec<(f64, usize)>>,
    open: Vec<(u32, usize, f64, bool)>,
    busy: BTreeMap<(u32, usize), f64>,
    /// B28: per flag, its owner and when each side was last within [`B28_NEAR_M`]
    owner: Vec<u32>,
    last_near: Vec<[f64; 3]>,
    t0: Option<f64>,
}

impl Books {
    fn sample(&mut self, t: f64, alive: &[P], fs: &[F]) {
        let t0 = *self.t0.get_or_insert(t);
        let count = |team: u32, f: &F, r: f32| alive.iter().filter(|p| p.team == team && d(p.pos, f.pos) < r).count() as u32;
        // B35
        for p in alive {
            if let Some(h) = p.b35 {
                let l = &mut self.s[p.team as usize];
                l.b35[1] += 1;
                l.b35[0] += h as u32;
            }
        }
        // B32 and B36: who comes within reach of a flag it does not own
        for p in alive {
            let now: Vec<(bool, bool)> = fs.iter().map(|f| (d(p.pos, f.pos) < B32_NEAR_M, d(p.pos, f.pos) < B36_NEAR_M)).collect();
            let prev = self.near.insert(p.id, (p.life, now.clone()));
            let Some((_, prev)) = prev.filter(|x| x.0 == p.life && x.1.len() == fs.len()) else { continue };
            for (i, f) in fs.iter().enumerate() {
                if !f.takeable || f.team == p.team {
                    continue;
                }
                if now[i].0 && !prev[i].0 && p.vehicle.is_none() {
                    let a = (p.pos[2] - f.pos[2]).atan2(p.pos[0] - f.pos[0]).rem_euclid(std::f32::consts::TAU);
                    let sector = ((a / std::f32::consts::TAU * B32_SECTORS as f32) as u32).min(B32_SECTORS - 1);
                    let e = self.arrivals.entry((p.team, i, ((t - t0) / B32_WINDOW_S) as u64)).or_default();
                    e.0 += 1;
                    e.1.insert(sector);
                }
                if now[i].1 && !prev[i].1 && (1..=2).contains(&f.team) {
                    let key = (p.team, i);
                    let es = self.entries.entry(key).or_default();
                    es.retain(|x| t - x.0 <= B36_WINDOW_S && x.1 != p.id);
                    es.push((t, p.id));
                    let free = self.busy.get(&key).is_none_or(|&end| t >= end);
                    if es.len() >= 2 && free && count(f.team, f, B36_NEAR_M) >= 1 {
                        self.open.push((p.team, i, t, false));
                        self.busy.insert(key, t + B36_WINDOW_S);
                        es.clear();
                    }
                }
            }
        }
        let ids: BTreeSet<usize> = alive.iter().map(|p| p.id).collect();
        self.near.retain(|k, _| ids.contains(k));
        let mut open = std::mem::take(&mut self.open);
        open.retain_mut(|c| {
            if let Some(f) = fs.get(c.1) {
                c.3 |= count(c.0, f, B36_NEAR_M) >= count(3 - c.0, f, B36_NEAR_M);
            }
            if t - c.2 < B36_WINDOW_S - 0.5 {
                return true;
            }
            let l = &mut self.s[c.0 as usize];
            l.b36[1] += 1;
            l.b36[0] += c.3 as u32;
            false
        });
        self.open = open;
        // B28 and the real presence
        if self.owner.len() != fs.len() {
            self.owner = fs.iter().map(|f| f.team).collect();
            self.last_near = vec![[f64::NEG_INFINITY; 3]; fs.len()];
        }
        for (i, f) in fs.iter().enumerate() {
            for team in 1..=2u32 {
                if count(team, f, B28_NEAR_M) > 0 {
                    self.last_near[i][team as usize] = t;
                }
            }
            if self.owner[i] != f.team {
                self.owner[i] = f.team;
                if (1..=2).contains(&f.team) {
                    let l = &mut self.s[f.team as usize];
                    l.b28[1] += 1;
                    l.b28[0] += (t - self.last_near[i][3 - f.team as usize] > B28_TAKE_S) as u32;
                }
            }
            if !f.takeable || !(1..=2).contains(&f.team) {
                continue;
            }
            if count(1, f, B28_NEAR_M) + count(2, f, B28_NEAR_M) == 0 {
                self.s[3 - f.team as usize].b28_empty_s += 1;
            }
        }
        for team in 1..=2u32 {
            let real = real_presence(alive, fs, team);
            let l = &mut self.s[team as usize];
            for (f, n) in fs.iter().zip(real) {
                if f.team == team && f.takeable {
                    l.presence[0] += 1;
                    l.presence[1] += n;
                    l.presence[2] += (n >= 4) as u32;
                }
            }
        }
    }

    fn put(&self, team: usize, put: &mut impl FnMut(&str, Option<f64>)) {
        let l = &self.s[team];
        put("B35_still_cases", Some(l.b35[1] as f64));
        put("B35_pct_still_hidden_crouched", pct(l.b35[0], l.b35[1]));
        let windows: Vec<f32> = self.arrivals.iter().filter(|(k, v)| k.0 == team as u32 && v.0 >= 2).map(|(_, v)| v.1.len() as f32).collect();
        put("B32_windows", Some(windows.len() as f64));
        put("B32_sectors_median", median_s(&windows));
        put("B36_cases", Some(l.b36[1] as f64));
        put("B36_pct_matched", pct(l.b36[0], l.b36[1]));
        put("B28_captures", Some(l.b28[1] as f64));
        put("B28_pct_uncontested", pct(l.b28[0], l.b28[1]));
        put("B28_enemy_flag_empty_s", Some(l.b28_empty_s as f64));
        put("presence_mean", (l.presence[0] > 0).then(|| round2(l.presence[1] as f64 / l.presence[0] as f64)));
        put("presence_pct_4_or_more", pct(l.presence[2], l.presence[0]));
    }
}

/// B27: a victim was busy if one of the killer's side other than the killer hurt it in this long before, s, or it was in a vehicle
/// this long before, s.
pub const B27_BUSY_S: f64 = 5.0;
pub const B27_GOT_OFF_S: f64 = 3.0;
/// B30: a mine is laid where enemy vehicles pass when one with a crew went within this of it, m, in this long before, s; a vehicle
/// destroyed within [`B30_HIT_M`] of a mine the side laid counts as destroyed by mines, m.
pub const B30_NEAR_M: f32 = 15.0;
pub const B30_TRAFFIC_S: f64 = 180.0;
pub const B30_HIT_M: f32 = 10.0;
/// B33: an action is announced when its doer said something on the radio within this of it, s.
pub const B33_SAID_S: f64 = 3.0;

#[derive(Default, Debug, Clone)]
struct SideSt {
    /// B27: kills of busy victims, kills
    b27: [u32; 2],
    /// B29: boardings of vehicles the enemy drove or that were born on its side
    b29: u32,
    /// B30: mines laid where enemy vehicles passed, mines laid; enemy vehicles destroyed by mines
    b30: [u32; 2],
    b30_kills: u32,
    /// B33: streetwise actions announced, actions
    b33: [u32; 2],
}

/// The streetwise cards (B27, B29, B30, B33), from the sim's journal: who hurt whom, who boarded what, the mines laid.
#[derive(Default, Debug)]
struct Streetwise {
    s: [SideSt; 3],
    /// per soldier: the hurts of the last [`B27_BUSY_S`] (when, by which vehicle), and the last time it was in a vehicle
    hurt: BTreeMap<usize, Vec<(f64, usize)>>,
    in_vehicle: BTreeMap<usize, f64>,
    /// per vehicle: the side born on (its team when first seen) and the side of its last driver; per player, the vehicle it was in
    owner: BTreeMap<usize, (u32, u32)>,
    inside: BTreeMap<usize, Option<usize>>,
    /// B30: enemy ground vehicles with a crew, per side that drove them: (when, where); the mines laid: (side, where)
    traffic: Vec<(f64, u32, [f32; 3])>,
    mines: Vec<(u32, [f32; 3])>,
    /// B33: the actions waiting to see whether they are announced: (when, player, side, announced)
    pending: Vec<(f64, usize, u32, bool)>,
    /// B33: the messages of the last [`B33_SAID_S`]: (when, player)
    said: Vec<(f64, usize)>,
    damage_seen: usize,
    fired_seen: usize,
    radio_seen: usize,
}

impl Streetwise {
    fn step(&mut self, s: &crate::Sim, t: f64, player_of_soldier: &[Option<usize>]) {
        let team_of_vehicle = |j: usize| -> Option<u32> {
            if let Some(k) = player_of_soldier.get(j).copied().flatten() {
                return Some(s.players[k].team);
            }
            let v = s.vehicles.get(j)?;
            v.entry.occupants.iter().flatten().find_map(|&o| player_of_soldier.get(o).copied().flatten()).map(|k| s.players[k].team)
        };
        // B33: what was said lately (an action counts as announced with a message up to 3 s before or after it)
        let said: Vec<(f64, usize)> = s.journal.radio_log.get(self.radio_seen..).unwrap_or(&[]).iter().map(|e| (e.step as f64 / 30.0, e.player)).collect();
        self.radio_seen = s.journal.radio_log.len();
        self.said.extend(said.iter().copied());
        self.said.retain(|x| t - x.0 <= B33_SAID_S);
        for x in self.pending.iter_mut() {
            x.3 |= said.iter().any(|&(w, k)| k == x.1 && (w - x.0).abs() <= B33_SAID_S);
        }
        let recent = self.said.clone();
        let act = |pending: &mut Vec<(f64, usize, u32, bool)>, k: usize, team: u32| pending.push((t, k, team, recent.iter().any(|x| x.1 == k)));
        // who is in what, and the vehicles' sides
        for (k, p) in s.players.iter().enumerate() {
            let Some(sol) = p.soldier.filter(|_| p.alive && (1..=2).contains(&p.team)) else {
                self.inside.remove(&k);
                continue;
            };
            let now = s.vehicles.get(sol).and_then(|v| v.entry.inside);
            if now.is_some() {
                self.in_vehicle.insert(sol, t);
            }
            let before = self.inside.insert(k, now).flatten();
            if let Some(j) = now {
                let v = &s.vehicles[j];
                let o = self.owner.entry(j).or_insert((v.team, 0));
                if before != Some(j) {
                    // B29: boarding what the enemy drove or what was born on its side
                    if o.1 == 3 - p.team || (o.1 == 0 && o.0 == 3 - p.team) {
                        self.s[p.team as usize].b29 += 1;
                        act(&mut self.pending, k, p.team);
                    }
                }
                if s.vehicles[sol].entry.position == 0 {
                    o.1 = p.team;
                }
            }
        }
        // B30: the enemy's ground vehicles with a crew, once a second
        if s.clock.steps % 30 == 0 {
            for (j, &(_, drv)) in &self.owner {
                let Some(v) = s.vehicles.get(*j).filter(|v| !v.removed && !v.has_soldier_contact()) else { continue };
                if v.entry.occupants.iter().flatten().next().is_some() && !VehicleClass::of(v).flies() && (1..=2).contains(&drv) {
                    self.traffic.push((t, drv, v.craft.body.pos));
                }
            }
            self.traffic.retain(|x| t - x.0 < B30_TRAFFIC_S);
        }
        for f in s.journal.fired.get(self.fired_seen..).unwrap_or(&[]) {
            let is_mine = f.3.and_then(|i| s.projectiles.get(i)).is_some_and(|p| p.template.name.to_ascii_lowercase().contains("mine"));
            let Some(k) = player_of_soldier.get(f.1).copied().flatten().filter(|_| is_mine) else { continue };
            let team = s.players[k].team;
            if !(1..=2).contains(&team) {
                continue;
            }
            let pos = f.2.pos;
            let l = &mut self.s[team as usize];
            l.b30[1] += 1;
            if self.traffic.iter().any(|x| x.1 == 3 - team && d(x.2, pos) < B30_NEAR_M) {
                l.b30[0] += 1;
            }
            self.mines.push((team, pos));
            act(&mut self.pending, k, team);
        }
        self.fired_seen = s.journal.fired.len();
        // B27 (and B30's kills): the hurts, and the deaths
        for e in s.journal.damage_log.get(self.damage_seen..).unwrap_or(&[]) {
            if e.amount <= 0.0 {
                continue;
            }
            let victim = &s.vehicles[e.vehicle];
            if victim.has_soldier_contact() {
                let hurts = self.hurt.entry(e.vehicle).or_default();
                hurts.retain(|h| t - h.0 < B27_BUSY_S);
                let before: Vec<(f64, usize)> = hurts.clone();
                if let Some(by) = e.by {
                    hurts.push((t, by));
                }
                if e.hp > 0.0 {
                    continue;
                }
                let (Some(by), Some(vk)) = (e.by, player_of_soldier.get(e.vehicle).copied().flatten()) else { continue };
                let vteam = s.players[vk].team;
                let Some(kteam) = team_of_vehicle(by).filter(|&k| (1..=2).contains(&k) && k != vteam) else { continue };
                let busy = before.iter().any(|h| h.1 != by && team_of_vehicle(h.1) == Some(kteam)) || self.in_vehicle.get(&e.vehicle).is_some_and(|&w| t - w < B27_GOT_OFF_S && t - w > 0.05);
                let l = &mut self.s[kteam as usize];
                l.b27[1] += 1;
                l.b27[0] += busy as u32;
                if busy {
                    let killer = player_of_soldier.get(by).copied().flatten().or_else(|| s.vehicles.get(by)?.entry.occupants.iter().flatten().find_map(|&o| player_of_soldier.get(o).copied().flatten()));
                    if let Some(k) = killer {
                        act(&mut self.pending, k, kteam);
                    }
                }
                self.hurt.remove(&e.vehicle);
            } else if e.hp <= 0.0 && e.status.is_some() {
                let Some(&(_, drv)) = self.owner.get(&e.vehicle) else { continue };
                let pos = victim.craft.body.pos;
                if (1..=2).contains(&drv) && self.mines.iter().any(|m| m.0 == 3 - drv && d(m.1, pos) < B30_HIT_M) {
                    self.s[(3 - drv) as usize].b30_kills += 1;
                }
            }
        }
        self.damage_seen = s.journal.damage_log.len();
        let mut pending = std::mem::take(&mut self.pending);
        pending.retain(|x| {
            if t - x.0 < B33_SAID_S {
                return true;
            }
            let l = &mut self.s[x.2 as usize];
            l.b33[1] += 1;
            l.b33[0] += x.3 as u32;
            false
        });
        self.pending = pending;
    }

    fn put(&self, team: usize, put: &mut impl FnMut(&str, Option<f64>)) {
        let l = &self.s[team];
        put("B27_kills", Some(l.b27[1] as f64));
        put("B27_pct_busy_victims", pct(l.b27[0], l.b27[1]));
        put("B29_enemy_vehicles_boarded", Some(l.b29 as f64));
        put("B30_mines", Some(l.b30[1] as f64));
        put("B30_pct_mines_on_traffic", pct(l.b30[0], l.b30[1]));
        put("B30_vehicles_destroyed_by_mines", Some(l.b30_kills as f64));
        put("B33_actions", Some(l.b33[1] as f64));
        put("B33_pct_announced_3s", pct(l.b33[0], l.b33[1]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: usize, team: u32, x: f32, z: f32) -> P {
        P { id, team, alive: true, pos: [x, 0.0, z], hp: 1.0, ..Default::default() }
    }

    fn flag(x: f32, team: u32) -> F {
        F { pos: [x, 0.0, 0.0], radius: 10.0, team, takeable: true }
    }

    fn side(m: Meter, team: &str) -> Value {
        m.finish()["sides"][team].clone()
    }

    fn plane(id: usize, x: f32, y: f32) -> V {
        V { id, class: VehicleClass::Plane, seats: 1, occupants: vec![id], pos: [x, y, 0.0], hp: 1.0, alt: y, land_near: false }
    }

    #[test]
    fn b35_counts_the_still_cases_only() {
        let mut m = Meter::new();
        let mut a = p(0, 1, 0.0, 0.0);
        a.b35 = Some(true);
        let mut b = p(1, 1, 5.0, 0.0);
        b.b35 = Some(false);
        let c = p(2, 2, 50.0, 0.0);
        m.sample(1.0, &[a.clone(), b.clone(), c.clone()], &[], &[]);
        a.b35 = Some(true);
        m.sample(2.0, &[a, p(1, 1, 5.0, 0.0), c], &[], &[]);
        let l = side(m, "1");
        assert_eq!((l["B35_still_cases"].as_f64(), l["B35_pct_still_hidden_crouched"].as_f64()), (Some(3.0), Some(66.7)));
    }

    #[test]
    fn b32_distinct_sectors_of_arrival() {
        let mut m = Meter::new();
        let f = [flag(0.0, 2)];
        // 0 comes from the east, 1 from the north, 2 from the east again: two sectors in the window
        m.sample(1.0, &[p(0, 1, 50.0, 0.0), p(1, 1, 0.0, 50.0), p(2, 1, 60.0, 1.0)], &[], &f);
        m.sample(2.0, &[p(0, 1, 20.0, 0.0), p(1, 1, 0.0, 20.0), p(2, 1, 25.0, 1.0)], &[], &f);
        let l = side(m, "1");
        assert_eq!((l["B32_windows"].as_f64(), l["B32_sectors_median"].as_f64()), (Some(1.0), Some(2.0)));
    }

    #[test]
    fn b36_squad_arrival_against_the_defenders() {
        let mut m = Meter::new();
        let f = [flag(0.0, 2)];
        let def = |x| p(9, 2, x, 0.0);
        let def2 = p(8, 2, 5.0, 5.0);
        let def3 = p(7, 2, 5.0, -5.0);
        // two attackers come in together against three defenders, then a third arrives: 3 >= 3, met
        m.sample(1.0, &[p(0, 1, 100.0, 0.0), p(1, 1, 100.0, 5.0), p(2, 1, 200.0, 0.0), def(0.0), def2.clone(), def3.clone()], &[], &f);
        m.sample(2.0, &[p(0, 1, 50.0, 0.0), p(1, 1, 50.0, 5.0), p(2, 1, 200.0, 0.0), def(0.0), def2.clone(), def3.clone()], &[], &f);
        m.sample(3.0, &[p(0, 1, 50.0, 0.0), p(1, 1, 50.0, 5.0), p(2, 1, 50.0, 0.0), def(0.0), def2.clone(), def3.clone()], &[], &f);
        for t in 4..=25 {
            m.sample(t as f64, &[p(0, 1, 50.0, 0.0), def(0.0), def2.clone(), def3.clone()], &[], &f);
        }
        let l = side(m, "1");
        assert_eq!((l["B36_cases"].as_f64(), l["B36_pct_matched"].as_f64()), (Some(1.0), Some(100.0)));
    }

    #[test]
    fn b28_uncontested_capture_and_empty_flag() {
        let mut m = Meter::new();
        m.sample(1.0, &[p(0, 1, 0.0, 0.0), p(1, 2, 500.0, 0.0)], &[], &[flag(0.0, 2), flag(1000.0, 2)]);
        m.sample(2.0, &[p(0, 1, 0.0, 0.0), p(1, 2, 500.0, 0.0)], &[], &[flag(0.0, 1), flag(1000.0, 2)]);
        let l = side(m, "1");
        assert_eq!((l["B28_captures"].as_f64(), l["B28_pct_uncontested"].as_f64()), (Some(1.0), Some(100.0)));
        // the far enemy flag was empty both seconds
        assert_eq!(l["B28_enemy_flag_empty_s"], 2.0);
    }

    #[test]
    fn real_presence_per_flag() {
        let ps = [p(0, 1, 0.0, 0.0), p(1, 2, 100.0, 0.0), p(2, 2, 400.0, 0.0)];
        assert_eq!(real_presence(&ps, &[flag(0.0, 1), flag(400.0, 1)], 1), vec![1, 1]);
        assert_eq!(real_presence(&ps, &[flag(0.0, 1)], 2), vec![1]);
    }

    #[test]
    fn b26_contact_needs_both_in_flight_and_close() {
        let mut m = Meter::new();
        let ps = [p(0, 1, 0.0, 0.0), p(1, 2, 0.0, 0.0), p(2, 2, 0.0, 0.0)];
        // 0 and 1 are 400 m apart in flight; 2 is on the ground next to 0
        let vs = [plane(0, 0.0, 300.0), plane(1, 400.0, 300.0), plane(2, 10.0, 2.0)];
        m.sample(1.0, &ps, &vs, &[]);
        // 1 is 700 m away from 0 now
        let vs = [plane(0, 0.0, 300.0), plane(1, 700.0, 300.0), plane(2, 10.0, 2.0)];
        m.sample(2.0, &ps, &vs, &[]);
        let r = m.finish();
        assert_eq!(r["sides"]["1"]["B26_contact_s"], 1.0);
        assert_eq!(r["sides"]["2"]["B26_contact_s"], 1.0);
    }

    #[test]
    fn b26_kills_shots_hits_and_crashes() {
        let mut m = Meter::new();
        // 10 (team 1) and 20 (team 2) fly 300 m apart; 30 (team 2) is far away; 40 (team 1) is on the ground
        let craft = [(10, 1, [0.0, 300.0, 0.0], true), (20, 2, [300.0, 300.0, 0.0], true), (30, 2, [9000.0, 300.0, 0.0], true), (40, 1, [0.0, 2.0, 5000.0], false)];
        m.air_frame(10.0, &craft);
        // 10 sees 20 (counts, and hits); 30 sees nobody; 40 is on the ground
        m.air_fire(10, Some(7));
        m.air_fire(10, None);
        m.air_fire(30, Some(8));
        m.air_fire(40, Some(9));
        m.air_hits(10.0, |k| if k == 7 { (vec![20], false) } else { (vec![], false) });
        // a shot that has hit nothing yet waits; one that ended does not
        m.air_fire(10, Some(11));
        m.air_hits(10.0, |_| (vec![], true));
        // 20 is shot down by 10: a kill for side 1; 30 is destroyed in a crash with no enemy near: a crash for side 2
        m.air_destroyed(11.0, 20, false, Some(10));
        m.air_destroyed(11.0, 30, true, None);
        // 40 crashes with an enemy near 2 s before: not a crash; one hit from something that is not an aircraft is not a kill
        m.air_frame(12.0, &[(40, 1, [0.0, 2.0, 0.0], false), (20, 2, [50.0, 100.0, 0.0], true)]);
        m.air_destroyed(14.0, 40, true, None);
        m.air_destroyed(14.0, 40, false, Some(99));
        let r = m.finish();
        let (a, b) = (&r["sides"]["1"], &r["sides"]["2"]);
        assert_eq!((a["B26_air_kills"].as_f64(), a["B26_shots"].as_f64(), a["B26_hits"].as_f64()), (Some(1.0), Some(3.0), Some(1.0)));
        assert_eq!(a["B26_crashes"], 0.0);
        assert_eq!(a["B26_hits_pct"], 33.3);
        assert_eq!((b["B26_air_kills"].as_f64(), b["B26_crashes"].as_f64(), b["B26_shots"].as_f64()), (Some(0.0), Some(1.0), Some(0.0)));
        // 20 was hit by 10 one second before it was destroyed: downed by side 1, and only once
        assert_eq!((a["B26_air_downed"].as_f64(), b["B26_air_downed"].as_f64()), (Some(1.0), Some(0.0)));
    }

    #[test]
    fn b26_an_abandoned_aircraft_is_downed_by_who_hit_it() {
        let mut m = Meter::new();
        m.air_frame(10.0, &[(10, 1, [0.0, 300.0, 0.0], true), (20, 2, [300.0, 300.0, 0.0], true), (30, 2, [400.0, 300.0, 0.0], true)]);
        m.air_fire(10, Some(7));
        m.air_fire(10, Some(8));
        m.air_hits(10.0, |k| if k == 7 { (vec![20], true) } else { (vec![30], true) });
        // the pilot of 20 jumps: nobody is inside when it hits the ground 12 s later; 30 flies on and crashes a minute later
        m.air_frame(11.0, &[(10, 1, [0.0, 300.0, 0.0], true), (30, 2, [400.0, 300.0, 0.0], true)]);
        m.air_destroyed(22.0, 20, true, None);
        m.air_destroyed(70.0, 30, true, None);
        let r = m.finish();
        assert_eq!((r["sides"]["1"]["B26_air_downed"].as_f64(), r["sides"]["1"]["B26_air_kills"].as_f64()), (Some(1.0), Some(0.0)));
        assert_eq!(r["sides"]["2"]["B26_air_downed"], 0.0);
    }

    #[test]
    fn b01_first_capture_and_contest() {
        let mut m = Meter::new();
        let ps = [p(0, 1, 0.0, 0.0), p(1, 2, 20.0, 0.0), p(2, 1, 500.0, 0.0), p(3, 2, 900.0, 0.0)];
        m.sample(5.0, &ps, &[], &[flag(0.0, 0), flag(300.0, 2)]);
        m.sample(10.0, &ps, &[], &[flag(0.0, 1), flag(300.0, 2)]);
        let r = m.finish();
        assert_eq!(r["B01_contested_flags_max"], 1);
        assert_eq!(r["sides"]["1"]["B01_first_capture_s"], 5.0);
        assert!(r["sides"]["2"].get("B01_first_capture_s").is_none());
    }

    #[test]
    fn b01_fraction_at_an_own_flag_with_no_enemy() {
        let mut m = Meter::new();
        let ps: Vec<P> = (0..4).map(|i| p(i, 1, i as f32, 0.0)).chain([p(4, 2, 900.0, 0.0)]).collect();
        m.sample(1.0, &ps, &[], &[flag(0.0, 1)]);
        // (just-spawned do not count: it is the spawn, not the choice)
        m.sample(2.0, &ps, &[], &[flag(0.0, 1)]);
        let mut m2 = Meter::new();
        m2.sample(1.0, &ps, &[], &[flag(0.0, 1)]);
        assert!(side(m, "1").get("B01_max_fraction_at_own_flag").is_none());
        m2.sample(1.0 + SETTLED_S, &ps, &[], &[flag(0.0, 1)]);
        assert_eq!(side(m2, "1")["B01_max_fraction_at_own_flag"], 1.0);
    }

    #[test]
    fn b04_spawns_with_two_mates_already_there() {
        let mut m = Meter::new();
        let mut ps: Vec<P> = (0..3).map(|i| p(i, 1, i as f32, 0.0)).chain([p(3, 1, 500.0, 0.0), p(4, 2, 900.0, 0.0)]).collect();
        ps[3].alive = false;
        m.sample(1.0, &ps, &[], &[]);
        // the 3 spawns next to 0..2; then 0 dies and spawns far from everybody
        ps[3] = P { alive: true, pos: [3.0, 0.0, 0.0], life: Some(30), ..ps[3].clone() };
        m.sample(2.0, &ps, &[], &[]);
        ps[0] = P { pos: [400.0, 0.0, 0.0], life: Some(31), ..ps[0].clone() };
        m.sample(3.0, &ps, &[], &[]);
        let l = side(m, "1");
        assert_eq!(l["B04_spawns"], 2.0);
        assert_eq!(l["B04_pct_spawn_with_2_teammates"], 50.0);
    }

    #[test]
    fn b02_group_on_foot_out_of_combat() {
        let mut m = Meter::new();
        // three together (each with two others within 15 m) and one alone far away; the enemy at 1 km
        let ps = [p(0, 1, 0.0, 0.0), p(1, 1, 5.0, 0.0), p(2, 1, 10.0, 0.0), p(3, 1, 400.0, 0.0), p(4, 2, 1000.0, 1000.0)];
        m.sample(1.0, &ps, &[], &[]);
        let l = side(m, "1");
        assert_eq!(l["B02_pct_with_2_teammates"], 75.0);
        assert_eq!(l["B02_samples"], 4.0);
    }

    #[test]
    fn b03_occupancy_on_leaving() {
        let mut m = Meter::new();
        let ps = [p(0, 1, 0.0, 0.0), p(1, 1, 0.0, 0.0)];
        let v = |x: f32, occ: Vec<usize>| V { id: 9, class: VehicleClass::Car, seats: 4, occupants: occ, pos: [x, 0.0, 0.0], hp: 1.0, alt: 0.0, land_near: false };
        m.sample(1.0, &ps, &[v(0.0, vec![0, 1])], &[]);
        m.sample(2.0, &ps, &[v(50.0, vec![0, 1])], &[]);
        m.sample(3.0, &ps, &[v(100.0, vec![0, 1])], &[]);
        m.sample(4.0, &ps, &[v(200.0, vec![0])], &[]);
        let l = side(m, "1");
        assert_eq!(l["B03_trips"], 1.0);
        assert_eq!(l["B03_wait_second_s"], 0.0, "the second boarded in the first sample");
        assert_eq!(l["B03_trips_with_second"], 1.0);
        assert_eq!(l["B03_mean_occupancy"], 0.5);
        assert_eq!(l["B03_pct_trips_with_2_or_more"], 100.0);
    }

    #[test]
    fn b03_wait_until_the_second_boards() {
        let mut m = Meter::new();
        let ps = [p(0, 1, 0.0, 0.0), p(1, 1, 0.0, 0.0)];
        let v = |x: f32, occ: Vec<usize>| V { id: 9, class: VehicleClass::Car, seats: 4, occupants: occ, pos: [x, 0.0, 0.0], hp: 1.0, alt: 0.0, land_near: false };
        m.sample(1.0, &ps, &[v(0.0, vec![0])], &[]);
        m.sample(2.0, &ps, &[v(0.0, vec![0])], &[]);
        m.sample(7.0, &ps, &[v(0.0, vec![0, 1])], &[]);
        m.sample(8.0, &ps, &[v(100.0, vec![0, 1])], &[]);
        let l = side(m, "1");
        assert_eq!(l["B03_wait_second_s"], 6.0);
    }

    #[test]
    fn b05_wounded_who_stops_firing() {
        let mut m = Meter::new();
        let mut wounded = p(0, 1, 0.0, 0.0);
        wounded.hp = 0.2;
        let enemy = p(1, 2, 30.0, 0.0);
        m.sample(1.0, &[wounded.clone(), enemy.clone()], &[], &[]);
        m.shot(5.0, 0, true, "k98", [0.0; 3]);
        // (keeps firing inside the window: does not count)
        m.sample(11.0, &[wounded.clone(), enemy.clone()], &[], &[]);
        // second case: neither fires nor stays within 60 m
        let mut far = wounded.clone();
        far.pos = [200.0, 0.0, 0.0];
        m.sample(12.0, &[wounded.clone(), enemy.clone()], &[], &[]);
        m.sample(22.0, &[far, enemy], &[], &[]);
        let l = side(m, "1");
        assert_eq!(l["B05_cases"], 2.0);
        assert_eq!(l["B05_pct_covered"], 50.0);
        assert_eq!(l["B05_pct_far_60m"], 50.0);
        assert_eq!(l["B05_pct_breaks_contact"], 50.0);
    }

    #[test]
    fn b05_breaks_contact_if_it_stops_firing_and_hides() {
        let mut m = Meter::new();
        let mut wounded = p(0, 1, 0.0, 0.0);
        wounded.hp = 0.2;
        let enemy = p(1, 2, 30.0, 0.0);
        m.sample(1.0, &[wounded.clone(), enemy.clone()], &[], &[]);
        // does not fire but stays in sight: covered (loose) yes, breaks contact no
        m.sample(11.0, &[wounded.clone(), enemy.clone()], &[], &[]);
        m.sample(12.0, &[wounded.clone(), enemy.clone()], &[], &[]);
        let mut covered = wounded.clone();
        covered.hidden = true;
        m.sample(22.0, &[covered, enemy], &[], &[]);
        m.b14(1, true);
        m.b14(1, false);
        let l = side(m, "1");
        assert_eq!(l["B05_cases"], 2.0);
        assert_eq!(l["B05_pct_covered"], 100.0);
        assert_eq!(l["B05_pct_breaks_contact"], 50.0);
        assert_eq!(l["B14_pct_reacts_2s"], 50.0);
    }

    #[test]
    fn b06_heal_and_recovery() {
        let mut m = Meter::new();
        let mut a = p(0, 1, 0.0, 0.0);
        a.life = Some(7);
        a.hp = 0.2;
        m.sample(1.0, &[a.clone()], &[], &[]);
        m.heal(1, 50.0, 1.0);
        m.heal(1, 10.0, 0.0);
        a.hp = 0.9;
        m.sample(2.0, &[a], &[], &[]);
        let l = side(m, "1");
        assert_eq!(l["B06_hp_healed_by_others"], 50.0);
        assert_eq!(l["B06_hp_healed_total"], 60.0);
        assert_eq!(l["B06_pct_lives_recovered"], 100.0);
    }

    #[test]
    fn b07_repair_of_a_crewed_tank() {
        let mut m = Meter::new();
        let ps = [p(0, 2, 0.0, 0.0)];
        let t = |hp: f32| V { id: 3, class: VehicleClass::Tank, seats: 2, occupants: vec![0], pos: [0.0; 3], hp, alt: 0.0, land_near: false };
        m.sample(1.0, &ps, &[t(0.3)], &[]);
        m.repair(2, 120.0);
        m.sample(2.0, &ps, &[t(0.8)], &[]);
        let l = side(m, "2");
        assert_eq!((l["B07_hp_repaired"].as_f64(), l["B07_vehicles_boarded"].as_f64()), (Some(120.0), Some(1.0)));
    }

    #[test]
    fn b08_rocket_at_a_soldier_and_at_a_tank() {
        let mut m = Meter::new();
        let mut tank = p(2, 2, 100.0, 0.0);
        tank.vehicle = Some(5);
        m.sample(1.0, &[p(0, 1, 0.0, 0.0), p(1, 2, 20.0, 0.0), tank], &[], &[]);
        m.shot(1.0, 0, true, "BazookaProjectile", [0.0; 3]);
        m.shot(1.0, 0, true, "k98_bullet", [0.0; 3]);
        // (the tank at 0 m from the shot is the nearest: a rocket at an armoured vehicle)
        m.shot(1.1, 0, true, "PanzerschreckRocket", [100.0, 0.0, 0.0]);
        // aiming at the tank from beside the soldier: the nearest is the soldier, but the rocket was going at the tank
        m.shot_dir(1.2, 0, true, "BazookaProjectile", [18.0, 0.0, 0.0], Some([1.0, 0.0, 0.0]));
        let l = side(m, "1");
        assert_eq!(l["B08_pct_at_soldier_by_line"], 33.3);
        assert_eq!(l["B08_at_shots"], 3.0);
        assert_eq!(l["B08_pct_against_soldier"], 66.7);
    }

    #[test]
    fn b08_armour_near_an_anti_tank_gets_a_rocket() {
        let mut m = Meter::new();
        let mut at = p(0, 1, 0.0, 0.0);
        at.at = true;
        let mut other_at = p(1, 1, 500.0, 0.0);
        other_at.at = true;
        let mut pilot = p(2, 2, 30.0, 0.0);
        pilot.vehicle = Some(7);
        let tank = |x: f32| V { id: 7, class: VehicleClass::Tank, seats: 1, occupants: vec![2], pos: [x, 0.0, 0.0], hp: 1.0, alt: 0.0, land_near: false };
        let mut pilot2 = p(3, 2, 530.0, 0.0);
        pilot2.vehicle = Some(8);
        let tank2 = V { id: 8, occupants: vec![3], pos: [530.0, 0.0, 0.0], ..tank(0.0) };
        m.sample(1.0, &[at.clone(), other_at.clone(), pilot.clone(), pilot2.clone()], &[tank(30.0), tank2.clone()], &[]);
        // the first gets a rocket from the anti-tank soldier who had it nearby; the second, nothing
        m.shot(3.0, 0, true, "BazookaProjectile", [0.0; 3]);
        for t in 2..=12 {
            m.sample(t as f64, &[at.clone(), other_at.clone(), pilot.clone(), pilot2.clone()], &[tank(30.0), tank2.clone()], &[]);
        }
        let l = side(m, "1");
        assert_eq!(l["B08_armour_near_at"], 2.0);
        assert_eq!(l["B08_pct_armour_gets_rocket_10s"], 50.0);
    }

    #[test]
    fn b09_clustered_deaths() {
        let mut m = Meter::new();
        let mut a = p(0, 1, 0.0, 0.0);
        let mut b = p(1, 1, 2.0, 0.0);
        let c = p(2, 1, 300.0, 0.0);
        m.sample(1.0, &[a.clone(), b.clone(), c.clone()], &[], &[]);
        a.alive = false;
        a.deaths = 1;
        m.sample(5.0, &[a.clone(), b.clone(), c.clone()], &[], &[]);
        b.alive = false;
        b.deaths = 1;
        m.sample(9.0, &[a, b, c], &[], &[]);
        let l = side(m, "1");
        assert_eq!((l["B09_deaths"].as_f64(), l["B09_pct_clustered"].as_f64()), (Some(2.0), Some(100.0)));
    }

    #[test]
    fn b11_kills_from_a_plane() {
        let mut m = Meter::new();
        let mut a = p(0, 1, 0.0, 0.0);
        a.vehicle = Some(4);
        let plane = V { id: 4, class: VehicleClass::Plane, seats: 1, occupants: vec![0], pos: [0.0; 3], hp: 1.0, alt: 100.0, land_near: false };
        m.sample(1.0, &[a.clone()], &[plane.clone()], &[]);
        a.kills = 2;
        m.sample(2.0, &[a], &[plane], &[]);
        let l = side(m, "1");
        assert_eq!((l["B11_casualties"].as_f64(), l["B11_casualties_from_air"].as_f64()), (Some(2.0), Some(2.0)));
    }

    #[test]
    fn b12_landing_near_land_and_they_go_on_on_foot() {
        let launch = |occ: Vec<usize>| V { id: 8, class: VehicleClass::Ship, seats: 4, occupants: occ, pos: [0.0; 3], hp: 1.0, alt: 0.0, land_near: true };
        let mut m = Meter::new();
        let (mut a, mut b) = (p(0, 1, 0.0, 0.0), p(1, 1, 0.0, 0.0));
        a.vehicle = Some(8);
        b.vehicle = Some(8);
        m.sample(1.0, &[a.clone(), b.clone()], &[launch(vec![0, 1])], &[]);
        // one gets off and walks away; the other stays in the boat
        a.vehicle = None;
        a.pos = [30.0, 0.0, 0.0];
        m.sample(2.0, &[a.clone(), b.clone()], &[launch(vec![1])], &[]);
        a.pos = [90.0, 0.0, 0.0];
        m.sample(33.0, &[a, b], &[launch(vec![1])], &[]);
        let l = side(m, "1");
        assert_eq!((l["B12_multi_trips"].as_f64(), l["B12_trips_with_exit_near_land"].as_f64(), l["B12_disembark"].as_f64()), (Some(1.0), Some(1.0), Some(1.0)));
        assert_eq!(l["B12_pct_follow_on_foot_30s"], 100.0);
    }

    /// B02/B03: a tank out of combat with a teammate on foot at 20 m counts as escorted; alone, no; with an enemy nearby, it is not checked.
    #[test]
    fn b02_escorted_tank() {
        let tank = |pos: [f32; 3]| V { id: 9, class: VehicleClass::Tank, seats: 2, occupants: vec![0], pos, hp: 1.0, alt: 0.0, land_near: false };
        let mut m = Meter::new();
        let mut a = p(0, 1, 0.0, 0.0);
        a.vehicle = Some(9);
        m.sample(1.0, &[a.clone(), p(1, 1, 20.0, 0.0)], &[tank([0.0; 3])], &[]);
        m.sample(2.0, &[a.clone(), p(1, 1, 90.0, 0.0)], &[tank([0.0; 3])], &[]);
        m.sample(3.0, &[a, p(1, 1, 90.0, 0.0), p(2, 2, 30.0, 0.0)], &[tank([0.0; 3])], &[]);
        let l = side(m, "1");
        assert_eq!(l["B02_pct_tank_escorted"], 50.0);
    }

    /// B12: both get off at once (the boat is left empty and stops being among the vehicles with people): counts all the same.
    #[test]
    fn b12_all_get_off_at_once() {
        let launch = |occ: Vec<usize>| V { id: 8, class: VehicleClass::Ship, seats: 4, occupants: occ, pos: [0.0; 3], hp: 1.0, alt: 0.0, land_near: true };
        let mut m = Meter::new();
        let (mut a, mut b) = (p(0, 1, 0.0, 0.0), p(1, 1, 0.0, 0.0));
        a.vehicle = Some(8);
        b.vehicle = Some(8);
        m.sample(1.0, &[a.clone(), b.clone()], &[launch(vec![0, 1])], &[]);
        a.vehicle = None;
        b.vehicle = None;
        m.sample(2.0, &[a.clone(), b.clone()], &[], &[]);
        let l = side(m, "1");
        assert_eq!((l["B12_multi_trips"].as_f64(), l["B12_trips_with_exit_near_land"].as_f64(), l["B12_disembark"].as_f64()), (Some(1.0), Some(1.0), Some(2.0)));
    }

    #[test]
    fn b13_and_b18_defend_and_rush() {
        let mut m = Meter::new();
        let f = [flag(0.0, 1)];
        // an enemy enters the flag; an own one at 100 m arrives within 40 s
        m.sample(1.0, &[p(0, 2, 3.0, 0.0), p(1, 1, 100.0, 0.0)], &[], &f);
        m.sample(10.0, &[p(0, 2, 3.0, 0.0), p(1, 1, 5.0, 0.0)], &[], &f);
        // another case: the enemy leaves and re-enters, and nobody arrives
        m.sample(11.0, &[p(0, 2, 500.0, 0.0), p(1, 1, 100.0, 0.0)], &[], &f);
        m.sample(12.0, &[p(0, 2, 3.0, 0.0), p(1, 1, 100.0, 0.0)], &[], &f);
        m.sample(60.0, &[p(0, 2, 3.0, 0.0), p(1, 1, 100.0, 0.0)], &[], &f);
        let l = side(m, "1");
        assert_eq!(l["B18_cases"], 2.0);
        assert_eq!(l["B18_pct_arrive_40s"], 50.0);
        assert!(l["B13_samples"].as_f64().unwrap() >= 3.0);
        assert!(l["B13_pct_flag_with_defender"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn what_is_not_measured_is_missing_not_zero() {
        let r = Meter::new().finish();
        assert!(r["sides"]["1"].get("B02_pct_with_2_teammates").is_none());
        assert!(r["sides"]["1"].get("B01_first_capture_s").is_none());
        assert!(r["sides"]["1"].get("B04_pct_spawn_with_2_teammates").is_none());
        assert!(r["B16"]["totals"].is_object());
    }

    #[test]
    fn b15_cause_of_each_message() {
        use crate::radio as r;
        let base = ContextB15 { hp: 1.0, ..Default::default() };
        let wounded = ContextB15 { hp: 0.3, ..Default::default() };
        assert_eq!(cause_b15(r::LOCAL_MEDIC.key, &wounded), Some(true));
        assert_eq!(cause_b15(r::LOCAL_MEDIC.key, &base), Some(false));
        let broken = ContextB15 { hp: 1.0, vehicle_hp: Some(0.2), ..Default::default() };
        assert_eq!(cause_b15(r::LOCAL_NEED_REPAIRS.key, &broken), Some(true));
        assert_eq!(cause_b15(r::LOCAL_NEED_REPAIRS.key, &base), Some(false));
        let wheel = ContextB15 { wheel_with_seats: true, ..base.clone() };
        for m in [r::LOCAL_GET_IN, r::PICKUP] {
            assert_eq!(cause_b15(m.key, &wheel), Some(true));
            assert_eq!(cause_b15(m.key, &base), Some(false));
        }
        let ve = |f: fn(&mut ContextB15)| {
            let mut c = base.clone();
            f(&mut c);
            c
        };
        assert_eq!(cause_b15(r::ARMOR_SPOTTED.key, &ve(|c| c.armoured = true)), Some(true));
        assert_eq!(cause_b15(r::ARMOR_SPOTTED.key, &ve(|c| c.on_foot = true)), Some(false));
        assert_eq!(cause_b15(r::INFANTRY_SPOTTED.key, &ve(|c| c.on_foot = true)), Some(true));
        assert_eq!(cause_b15(r::AIRPLANE_SPOTTED.key, &ve(|c| c.air = true)), Some(true));
        assert_eq!(cause_b15(r::AIRPLANE_SPOTTED.key, &base), Some(false));
        let near = ve(|c| c.flag_m = Some(120.0));
        let far = ve(|c| c.flag_m = Some(350.0));
        for m in [r::ATTACK, r::DEFEND, r::GO_FOR_ENEMY_FLAG] {
            assert_eq!(cause_b15(m.key, &near), Some(true));
            assert_eq!(cause_b15(m.key, &far), Some(false));
            assert_eq!(cause_b15(m.key, &base), Some(false));
        }
        assert_eq!(cause_b15(r::ROGER.key, &base), None, "an acknowledgement asks for nothing");
        assert_eq!(cause_b15(r::LOCAL_FIRE_IN_HOLE.key, &base), Some(false), "no explosive in hand: no cause");
        assert_eq!(cause_b15(r::LOCAL_FIRE_IN_HOLE.key, &ve(|c| c.explosive = true)), Some(true));
        assert_eq!(cause_b15(r::LOCAL_HOLD_FIRE.key, &base), Some(false), "not in the table: no cause");
    }

    #[test]
    fn b15_messages_per_life_and_answer_to_the_medic() {
        use crate::radio as r;
        let mut m = Meter::new();
        let wounded = ContextB15 { hp: 0.2, ..Default::default() };
        let healthy = ContextB15 { hp: 1.0, ..Default::default() };
        // two lives of a player on side 1 and one on side 2
        m.sample(1.0, &[p(0, 1, 0.0, 0.0), p(1, 2, 90.0, 0.0)], &[], &[]);
        m.radio(2.0, 1, 0, Some(7), r::LOCAL_MEDIC.key, &wounded);
        m.radio(3.0, 1, 0, Some(7), r::LOCAL_GET_IN.key, &healthy);
        m.radio(3.5, 1, 0, Some(7), r::ROGER.key, &healthy);
        // nobody heals them
        m.radio(4.0, 2, 1, Some(9), r::LOCAL_MEDIC.key, &wounded);
        m.healed(1);
        m.close_medic_calls(30.0, |_, _| Some(0.2));
        let r1 = side(m, "1");
        assert_eq!(r1["B15_messages"], 2.0);
        assert_eq!(r1["B15_acks"], 1.0);
        assert_eq!(r1["B15_lives"], 1.0);
        assert_eq!(r1["B15_messages_per_life"], 2.0);
        assert_eq!(r1["B15_max_per_life"], 2.0);
        assert_eq!(r1["B15_pct_with_cause"], 50.0);
        assert_eq!(r1["B15_medic_requests"], 1.0);
        assert_eq!(r1["B15_pct_medic_attended_20s"], 0.0);
    }

    #[test]
    fn b15_medic_attended_and_life_lost() {
        use crate::radio as r;
        let mut m = Meter::new();
        let wounded = ContextB15 { hp: 0.2, ..Default::default() };
        m.radio(2.0, 1, 0, Some(7), r::LOCAL_MEDIC.key, &wounded);
        m.radio(2.0, 1, 1, Some(8), r::LOCAL_MEDIC.key, &wounded);
        m.radio(2.0, 1, 2, Some(9), r::LOCAL_MEDIC.key, &wounded);
        m.healed(0);
        // the 1 dies before being healed; the 2 stays alive past 20 s
        m.close_medic_calls(10.0, |k, _| (k != 1).then_some(0.2));
        // the 2 rises in health without the heal being seen (a medic at 6 m)
        m.close_medic_calls(12.0, |k, _| Some(if k == 2 { 0.5 } else { 0.2 }));
        m.close_medic_calls(23.0, |_, _| Some(0.2));
        let l = side(m, "1");
        assert_eq!(l["B15_medic_requests"], 3.0);
        assert!((l["B15_pct_medic_attended_20s"].as_f64().unwrap() - 66.7).abs() < 0.01);
    }

    #[test]
    fn b25_calls_with_a_medic_near_are_counted_apart() {
        use crate::radio as r;
        let mut m = Meter::new();
        let near = ContextB15 { hp: 0.2, medic_m: Some(100.0), ..Default::default() };
        let far = ContextB15 { hp: 0.2, medic_m: Some(400.0), ..Default::default() };
        let none = ContextB15 { hp: 0.2, ..Default::default() };
        m.radio(2.0, 1, 0, Some(7), r::LOCAL_MEDIC.key, &near);
        m.radio(2.0, 1, 1, Some(8), r::LOCAL_MEDIC.key, &near);
        m.radio(2.0, 1, 2, Some(9), r::LOCAL_MEDIC.key, &far);
        m.radio(2.0, 1, 3, Some(10), r::LOCAL_MEDIC.key, &none);
        m.healed(0);
        m.close_medic_calls(30.0, |_, _| Some(0.2));
        let l = side(m, "1");
        assert_eq!(l["B25_medic_calls_near"], 2.0);
        assert_eq!(l["B25_pct_attended_20s_medic_near"], 50.0);
        assert_eq!(l["B25_pct_calls_no_medic_near"], 50.0);
    }

    #[test]
    fn b25_a_caller_that_dies_first_is_left_out() {
        use crate::radio as r;
        let mut m = Meter::new();
        let near = ContextB15 { hp: 0.2, medic_m: Some(100.0), ..Default::default() };
        // four calls with a medic near: one healed, one alive and not healed at 20 s, two dead before
        for k in 0..4 {
            m.radio(2.0, 1, k, Some(7 + k), r::LOCAL_MEDIC.key, &near);
        }
        m.healed(0);
        m.close_medic_calls(5.0, |k, _| (k == 1).then_some(0.2));
        m.close_medic_calls(30.0, |k, _| (k == 1).then_some(0.2));
        let l = side(m, "1");
        assert_eq!(l["B25_pct_attended_20s_medic_near"], 25.0);
        assert_eq!(l["B25_pct_caller_died_20s"], 50.0);
        assert_eq!(l["B25_pct_attended_20s_caller_lives"], 50.0);
    }

    #[test]
    fn b14_full_counts_hearing() {
        let mut m = Meter::new();
        // one case with a heard shot looking toward it, one heard without reacting, one without hearing that moves
        m.b14(1, false);
        m.b14_heard(1, true, true);
        m.b14_heard(1, true, false);
        m.b14_heard(1, false, true);
        let l = side(m, "1");
        assert_eq!(l["B14_cases_heard"], 2.0);
        assert_eq!(l["B14_pct_reacts_heard_2s"], 50.0);
        assert!((l["B14_pct_reacts_full_2s"].as_f64().unwrap() - 66.7).abs() < 0.01);
    }

    #[test]
    fn without_radio_b15_is_zero_with_no_percentages() {
        let l = side(Meter::new(), "1");
        assert_eq!(l["B15_messages"], 0.0);
        assert!(l.get("B15_pct_with_cause").is_none());
        assert!(l.get("B15_messages_per_life").is_none());
    }

    /// B01..B18 over a short Berlin round with 16 bots: wide ranges
    #[test]
    fn a_short_round_of_berlin() {
        let game = bytefield_testdata::game();
        let Ok(w) = crate::World::load(&game, "bf1942", "Berlin", "conquest") else { return };
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.enable_bots();
        for i in 0..16 {
            let k = s.join(0, true);
            s.players[k].kit = (i / 2) % 5;
        }
        let mut m = Meter::new();
        for _ in 0..30 * 90 {
            s.step();
            m.step(&s);
        }
        let r = m.finish();
        for team in ["1", "2"] {
            let l = &r["sides"][team];
            let n = l["B02_samples"].as_f64().unwrap();
            assert!(n > 100.0, "{l}");
            for k in l.as_object().unwrap().keys().filter(|k| k.contains("pct")) {
                let v = l[k].as_f64().unwrap();
                assert!((0.0..=100.0).contains(&v), "{k} {v}");
            }
        }
        assert!(r["B16"]["totals"]["bots seen"].as_f64().unwrap() >= 16.0);
        // B27: the kills of a minute and a half are seen, with their killer's side
        let kills: f64 = ["1", "2"].iter().map(|t| r["sides"][*t]["B27_kills"].as_f64().unwrap()).sum();
        let deaths: u32 = s.players.iter().map(|p| p.deaths).sum();
        assert!(kills > 0.0 && kills <= deaths as f64, "{kills} kills for {deaths} deaths");
        assert!(r["B16"]["by_side"]["1"]["bots"].as_u64().unwrap() > 0);
        // B17: in a minute and a half of Berlin there are reactions and shots with error measurable
        let reaction: Vec<f64> = ["1", "2"].iter().filter_map(|t| r["sides"][*t]["B17_reaction_s"].as_f64()).collect();
        let err: Vec<f64> = ["1", "2"].iter().filter_map(|t| r["sides"][*t]["B17_error_degrees"].as_f64()).collect();
        assert!(!reaction.is_empty() && reaction.iter().all(|x| (0.0..=10.0).contains(x)), "{reaction:?}");
        assert!(!err.is_empty() && err.iter().all(|x| (0.0..20.0).contains(x)), "{err:?}");
    }
}
