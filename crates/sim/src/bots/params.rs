//! Named parameters ([`BotParams`]) and a side's parameter file (`params/bots/*.toml`).
//!
//! [`BotParams`] are the numbers the learning loop may move: each has a name, a default, a range with a physical sense and a why.
//! They are read in layers, each over the one before: the defaults in the code → `params/bots/global.toml` → the mod's layer
//! (`params/bots/bf1942.toml`, `params/bots/DC_Final.toml`) → `BYTEFIELD_BOTS_PARAMS=<file>` → what the mind's name forces
//! (`bots:rally_m=140`, which is how a side's file reaches the mind). The global and mod layers are compiled into the program, so a round
//! does not depend on the directory it runs from; the parameters travel inside each mind (no global state: rounds run on many threads).
//!
//! A side's file, the fixed rivals the learning loop measures against: the mind with its variants, the team head's levers
//! ([`crate::bots::squad::Levers::force`]) and named parameters.
//!
//! ```toml
//! mind = "bots:regroup"   # a `--mind`-style name
//! [levers]
//! size = 5
//! [params]
//! rally_m = 140
//! ```
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Who reads a parameter: the side's head (one per side; with `tournament3 --split` both halves must agree on these) or each bot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Team,
    Individual,
}

/// What is known of a parameter: its name, default, range (both ends allowed), unit, who reads it and why it is what it is.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub name: &'static str,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    pub unit: &'static str,
    pub scope: Scope,
    pub why: &'static str,
}

impl Spec {
    /// A count (unit `n`): it takes whole numbers only.
    pub fn integer(&self) -> bool {
        self.unit == "n"
    }
}

macro_rules! bot_params {
    ($( $(#[doc = $doc:literal])* $name:ident : $ty:ty = $def:expr, [$min:expr, $max:expr], $unit:literal, $scope:ident; )*) => {
        /// The named parameters (see the module). With every value at its default the bots are the same as with none.
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct BotParams {
            $( $(#[doc = $doc])* pub $name: $ty, )*
        }

        impl Default for BotParams {
            fn default() -> BotParams {
                BotParams { $( $name: $def, )* }
            }
        }

        /// Every parameter, in the order they are declared.
        pub const SPECS: &[Spec] = &[ $( Spec { name: stringify!($name), default: $def as f64, min: $min as f64, max: $max as f64, unit: $unit, scope: Scope::$scope, why: concat!($($doc, "\n",)*) }, )* ];

        impl BotParams {
            /// A parameter's value by its name.
            pub fn get(&self, name: &str) -> Option<f64> {
                match name {
                    $( stringify!($name) => Some(self.$name as f64), )*
                    _ => None,
                }
            }

            fn put(&mut self, name: &str, value: f64) {
                match name {
                    $( stringify!($name) => self.$name = value as $ty, )*
                    _ => unreachable!(),
                }
            }
        }
    };
}

bot_params! {
    // `bots:regroup` (and `bots:regroup-way`, R1), [`crate::bots::team::Team::regroup`]
    /// The assembly point is this far short of the flag: out of the defenders' sight and fire at the flag, near enough to go in
    /// together in a few seconds (with the old `regroup`, along the bearing the squads come from; with `regroup-way`, along each
    /// squad's way on foot).
    rally_m: f32 = 125.0, [60.0, 220.0], "m", Team;
    /// `regroup-way`: two squads whose points are nearer than this share one (0: each its own), so that they go in together.
    rally_merge_m: f32 = 60.0, [0.0, 150.0], "m", Team;
    /// A person within this of the point is there: a squad standing round it, not one bunched on top of it.
    rally_near_m: f32 = 30.0, [10.0, 60.0], "m", Team;
    /// The squads wait at most this long after the first got there: long enough for those walking behind, short enough not to lose
    /// the flag's moment.
    rally_wait_s: f64 = 25.0, [5.0, 90.0], "s", Team;
    /// With nobody there, the point is given up after this long (the squads went another way, or died on it).
    rally_come_s: f64 = 90.0, [30.0, 240.0], "s", Team;
    /// A squad nearer the flag than this share of the point's distance is already going in and is not held back.
    rally_inside: f32 = 0.6, [0.3, 0.9], "", Team;
    /// Once let go, the flag has no new point for this long: the squads that went in are not called back out.
    rally_released_s: f64 = 60.0, [0.0, 180.0], "s", Team;
    /// The point is made when a squad on foot coming to the flag is within this of it (made a kilometre away, nobody got there
    /// in time on El_Alamein); it must stay beyond `rally_m` + `rally_near_m`.
    rally_start_m: f32 = 350.0, [300.0, 700.0], "m", Team;

    // The split ([`crate::bots::team::split_up`]): which flag each unit goes to.
    /// How often the side splits its people among the flags: often enough to answer a flag being taken, seldom enough that a squad
    /// does not turn round on the way.
    split_period_s: f64 = 4.0, [2.0, 12.0], "s", Team;
    /// An own flag being taken calls the units with somebody nearer than this, at once: a few seconds' run, not a trip across the map.
    urgency_m: f32 = 150.0, [60.0, 350.0], "m", Team;
    /// At most this many people answer a flag being taken (one more than the enemies on it, up to this): the rest keep attacking.
    urgency_most: usize = 3, [1, 6], "n", Team;
    /// An enemy seen nearer than this to an own flag threatens it and calls defenders: a flag's radius and a bit, not the valley round it.
    threat_m: f32 = 60.0, [25.0, 150.0], "m", Team;
    /// How long a report of a seen enemy is worth: what a person remembers of where the enemy was before it has surely moved.
    report_s: f64 = 10.0, [4.0, 30.0], "s", Team;
    /// Defenders come to a threatened own flag from within this (the units nearest first).
    defend_m: f32 = 400.0, [150.0, 800.0], "m", Team;
    /// At most this many people defend a threatened flag (as many as the enemies seen, up to this).
    defend_most: usize = 3, [1, 6], "n", Team;
    /// The unit left guarding the own front flag is at most this share of the side (a squad, never half of it).
    guard_share: f32 = 0.3, [0.1, 0.5], "", Team;
    /// … and is taken from those within this of the flag (one already there, not one sent back across the map).
    guard_m: f32 = 250.0, [100.0, 500.0], "m", Team;
    /// Attacking two flags, the second's share against the first's 0.6 (only their ratio counts): 0.4 splits 60/40.
    attack_w2_second: f32 = 0.4, [0.1, 0.9], "", Team;
    /// Attacking three flags, the second's and third's shares against the first's 0.45 (default 45/35/20).
    attack_w3_second: f32 = 0.35, [0.1, 0.6], "", Team;
    /// (the third's: 0 sends nobody there but keeps it in the list)
    attack_w3_third: f32 = 0.2, [0.0, 0.45], "", Team;
    /// The side attacks three flags at once only with at least this many free people (and three units): fewer, and each share is
    /// too small to take anything.
    attack_three_people: usize = 12, [6, 24], "n", Team;
    /// What a person over a flag's share costs, as if the flag were this much farther: how hard the shares are kept.
    quota_over_m: f32 = 500.0, [150.0, 1500.0], "m", Team;
    /// The flag a unit already had counts as this much nearer: it does not change its mind every split.
    keep_flag_m: f32 = 150.0, [0.0, 400.0], "m", Team;
    /// `bots:memory`: what a death is worth on a way on foot, as metres of way: a way through cells where the side dies more than on
    /// the map's average pays this for each extra death it expects ([`crate::bots::memory`]).
    memory_death_m: f32 = 300.0, [0.0, 1500.0], "m", Team;

    // Taking a flag ([`crate::bots::life::Enter`]): when the attacker goes in. The first three only act where the map's profile makes
    // the attacker wait outside a defended flag (`enter_wait`: Berlin); elsewhere it goes in at once.
    /// The attacker decides whether to go in once within this of the flag: near enough to see who holds it.
    enter_near_m: f32 = 40.0, [20.0, 80.0], "m", Individual;
    /// The flag is defended while it has seen an enemy less than this ago: what a person keeps in mind of a fight.
    enter_memory_s: f64 = 45.0, [10.0, 120.0], "s", Individual;
    /// It watches round the flag this long after arriving before taking it for undefended.
    enter_watch_s: f64 = 30.0, [5.0, 90.0], "s", Individual;
    /// A defender's flag is threatened by an enemy in sight nearer than this to its centre (then it looks for a new way in).
    flag_threat_m: f32 = 25.0, [10.0, 60.0], "m", Individual;

    // Firing on foot with `bots:fire-rhythm` ([`crate::bots::body::shooter::FpsTweaks::rhythm`]); off, the slower default rhythm.
    /// The pause between bursts, over a person's (a quarter of a second at point blank to 1.75 s at the limit of its reach).
    // TODO: tuned once with SPSA (both, 120 steps, side against side): no slope above the noise, `rhythm_pause` drifted to its floor
    // (final 0.36, burst 1.08) and against the defaults it stayed undecided (+0.20 +- 0.12 SD in 128 rounds, foot gate red in Kursk,
    // No_Fly_Zone_Day2, Weapon_Bunkers and Wake), so both stay at 1. Worth a second try only with more rounds per step, or once
    // `fire-first`'s foot jam is gone; a shorter pause also strays further from the default rhythm.
    rhythm_pause: f32 = 1.0, [0.3, 3.0], "", Individual;
    /// How much longer a burst is the nearer the enemy, over a person's (about 10 rounds at point blank; at the limit, the default 2).
    rhythm_burst: f32 = 1.0, [0.0, 2.0], "", Individual;

    // Survival ([`crate::bots::individual::Action::utility`]): what it feels like doing, from 0 to 1; the highest wins.
    /// Going to its goal with nothing else on: the floor every other action has to beat.
    util_goal: f32 = 0.5, [0.3, 0.7], "", Individual;
    /// Fighting an enemy in sight but not on top of it (on top it always fights).
    util_fight: f32 = 0.7, [0.4, 0.95], "", Individual;
    /// Taking cover with little health and the enemy in sight.
    util_cover: f32 = 0.9, [0.5, 1.0], "", Individual;
    /// Retreating with little health and the enemy in sight (turning its back to it).
    util_retreat_seen: f32 = 0.6, [0.3, 0.9], "", Individual;
    /// (out of sight, after a recent contact)
    util_retreat: f32 = 0.8, [0.4, 1.0], "", Individual;
    /// Going to a medic, wounded (plus 0.4 for each point of health under `health_medic`).
    util_medic: f32 = 0.75, [0.4, 0.95], "", Individual;
    /// A medic healing a mate, before what its distance takes off (a call is worth 0.85).
    util_heal: f32 = 0.6, [0.3, 0.9], "", Individual;
    /// An engineer repairing a vehicle of its own.
    util_repair: f32 = 0.75, [0.4, 0.95], "", Individual;
    /// With the enemy nearer than this it fights whatever its health (fleeing with its back turned at 15 m is dying).
    fight_near_m: f32 = 20.0, [8.0, 40.0], "m", Individual;
    /// After a contact it keeps retreating or minding the enemy this long without seeing anyone.
    enemy_memory_s: f32 = 12.0, [4.0, 30.0], "s", Individual;
    /// It changes action only for one it likes this much more (no dithering between two that are nearly worth the same).
    util_margin: f32 = 0.1, [0.0, 0.3], "", Individual;
    /// Below this health it goes to a medic near.
    health_medic: f32 = 0.5, [0.3, 0.8], "", Individual;
    /// It retreats this far from the enemy: out of its sight round a corner or two, not back to the base.
    retreat_m: f32 = 70.0, [30.0, 150.0], "m", Individual;
}

impl BotParams {
    /// Sets a parameter by its name, if its value is a number within its range.
    pub fn set(&mut self, name: &str, value: f64) -> Result<(), String> {
        let s = spec(name).ok_or_else(|| format!("bots has no parameter {name}"))?;
        if !(s.min..=s.max).contains(&value) {
            return Err(format!("{name} = {value}: out of its range [{}, {}] {}", s.min, s.max, s.unit));
        }
        if s.integer() && value.fract() != 0.0 {
            return Err(format!("{name} = {value}: a count, it takes a whole number"));
        }
        self.put(name, value);
        Ok(())
    }

    /// The values a layer file sets (`[params]`), over these.
    pub fn apply(&mut self, values: &BTreeMap<String, f64>) -> Result<(), String> {
        values.iter().try_for_each(|(k, v)| self.set(k, *v))
    }

    /// The parameters for a map: the defaults, the global and mod layers, `BYTEFIELD_BOTS_PARAMS`, and `forced` last (from the mind's
    /// name). Panics with a broken layer: a round must not run with other numbers than it says.
    pub fn layered(level: &str, forced: &[(String, f64)]) -> BotParams {
        let mut p = BotParams::default();
        let mut layer = |what: &str, text: &str| {
            let f = LayerFile::parse(text).unwrap_or_else(|e| panic!("bots parameters, {what}: {e}"));
            p.apply(&f.params).unwrap_or_else(|e| panic!("bots parameters, {what}: {e}"));
        };
        layer("params/bots/global.toml", GLOBAL);
        if let Some((name, text)) = mod_layer(level) {
            layer(name, text);
        }
        if let Some(path) = crate::debug::options().bots_params.as_deref() {
            let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("BYTEFIELD_BOTS_PARAMS={path}: {e}"));
            layer(path, &text);
        }
        for (k, v) in forced {
            p.set(k, *v).unwrap_or_else(|e| panic!("bots: {e}"));
        }
        p
    }

    /// Whether the side's head reads the same values in both (what `--split` needs: one head for two halves).
    pub fn same_for_team(&self, other: &BotParams) -> bool {
        SPECS.iter().filter(|s| s.scope == Scope::Team).all(|s| self.get(s.name) == other.get(s.name))
    }
}

/// A parameter's spec by its name.
pub fn spec(name: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.name == name)
}

const GLOBAL: &str = include_str!("../../../../params/bots/global.toml");
const BF1942: &str = include_str!("../../../../params/bots/bf1942.toml");
const DC_FINAL: &str = include_str!("../../../../params/bots/DC_Final.toml");

/// The mod layer for a map, by its name: the minds are made knowing only the level's name, so a `DC_` map takes DC_Final's (Desert
/// Combat 0.7 too), a `GC_` map none (Galactic Conquest uses the global layer for now) and any other bf1942's (the expansions too).
fn mod_layer(level: &str) -> Option<(&'static str, &'static str)> {
    let l = level.to_ascii_lowercase();
    if l.starts_with("dc_") {
        Some(("params/bots/DC_Final.toml", DC_FINAL))
    } else if l.starts_with("gc_") {
        None
    } else {
        Some(("params/bots/bf1942.toml", BF1942))
    }
}

/// A layer of parameters (`global.toml`, a mod's, `BYTEFIELD_BOTS_PARAMS`): what it is and the values it sets.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LayerFile {
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
}

impl LayerFile {
    pub fn parse(text: &str) -> Result<LayerFile, String> {
        let f: LayerFile = toml::from_str(text).map_err(|e| e.to_string())?;
        BotParams::default().apply(&f.params)?;
        Ok(f)
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SideFile {
    /// `bots` or `bots:` with variants joined with `+`
    pub mind: String,
    /// what the file is (shown nowhere: for whoever reads it)
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub levers: BTreeMap<String, f32>,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
}

impl SideFile {
    pub fn load(path: &Path) -> Result<SideFile, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        SideFile::parse(&text)
    }

    pub fn parse(text: &str) -> Result<SideFile, String> {
        let f: SideFile = toml::from_str(text).map_err(|e| e.to_string())?;
        if !crate::bots::is_name(&f.mind) {
            return Err(format!("mind {}: not a bots mind", f.mind));
        }
        if let Some(k) = f.levers.keys().find(|k| !crate::bots::squad::Levers::default().force(k, 0.0)) {
            return Err(format!("bots has no lever {k}"));
        }
        BotParams::default().apply(&f.params)?;
        Ok(f)
    }

    /// The mind's name with the levers and parameters forced (`bots:regroup+size=5+rally_m=140`), as [`crate::bots::mind`] takes it.
    pub fn mind_name(&self) -> String {
        let mut parts: Vec<String> = self.mind.split_once(':').map_or(Vec::new(), |(_, v)| v.split('+').filter(|v| !v.is_empty()).map(String::from).collect());
        parts.extend(self.levers.iter().map(|(k, v)| format!("{k}={v}")));
        parts.extend(self.params.iter().map(|(k, v)| format!("{k}={v}")));
        let head = self.mind.split_once(':').map_or(self.mind.as_str(), |(h, _)| h);
        if parts.is_empty() { head.to_string() } else { format!("{head}:{}", parts.join("+")) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_side_file_gives_the_mind_name() {
        assert_eq!(SideFile::parse("mind = \"bots\"").unwrap().mind_name(), "bots");
        assert_eq!(SideFile::parse("mind = \"bots:regroup\"\n[levers]\nsize = 5\n").unwrap().mind_name(), "bots:regroup+size=5");
        assert_eq!(SideFile::parse("mind = \"bots\"\n[levers]\nguard = 0\nsize = 3\n").unwrap().mind_name(), "bots:guard=0+size=3");
        assert_eq!(SideFile::parse("mind = \"bots:regroup\"\n[params]\nrally_m = 140\n").unwrap().mind_name(), "bots:regroup+rally_m=140");
        assert!(SideFile::parse("mind = \"plain\"").is_err());
        assert!(SideFile::parse("mind = \"bots\"\n[levers]\nnothing = 1\n").is_err());
        assert!(SideFile::parse("mind = \"bots\"\nmistyped = 1\n").is_err());
        assert!(SideFile::parse("mind = \"bots\"\n[params]\nnothing = 1\n").is_err());
        assert!(SideFile::parse("mind = \"bots\"\n[params]\nrally_m = 5000\n").is_err(), "out of range");
    }

    /// What the mind's name forces reaches the side's head (it did not: the head kept the defaults, and the parameters of scope `Team`
    /// changed nothing) and each bot.
    #[test]
    fn a_forced_parameter_reaches_the_head() {
        let e = crate::bots::head_of("bots:regroup-way+rally_m=140+urgency_m=100", "Kursk");
        assert_eq!((e.params.rally_m, e.params.urgency_m), (140.0, 100.0));
        assert_eq!(crate::bots::head_of("bots", "Kursk").params, BotParams::default());
    }

    #[test]
    fn the_anchors() {
        let anchor = |n: &str| SideFile::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../params/bots/{n}.toml"))).unwrap();
        assert_eq!(anchor("anchor-0").mind_name(), "bots:no-prone-fight+no-fps");
        assert_eq!(anchor("anchor-1").mind_name(), "bots:no-fps");
        assert_eq!(anchor("anchor-2").mind_name(), "bots:no-pistol-close+no-armour+no-close-fight+no-grenade-throw+no-heard-walk+no-fov+no-recall");
        assert_eq!(anchor("anchor-3").mind_name(), "bots:no-armour+no-close-fight+no-grenade-throw+no-heard-walk+no-fov+no-recall");
        assert_eq!(anchor("anchor-4").mind_name(), "bots:no-heard-walk+no-fov+no-recall");
        assert_eq!(anchor("anchor-5").mind_name(), "bots:no-fov+no-recall");
        assert_eq!(anchor("anchor-6").mind_name(), "bots");
    }

    /// Each spec's default is within its range and is what the struct holds; the shipped layers parse and, while nothing has been
    /// tuned, set nothing; each layer goes over the one before.
    #[test]
    fn the_layers() {
        let d = BotParams::default();
        for s in SPECS {
            assert!(s.min <= s.default && s.default <= s.max, "{}: default out of range", s.name);
            assert_eq!(d.get(s.name).unwrap() as f32, s.default as f32, "{}: the struct's default is not the spec's", s.name);
            assert!(!s.why.trim().is_empty(), "{}: no why", s.name);
        }
        for text in [GLOBAL, BF1942, DC_FINAL] {
            assert!(LayerFile::parse(text).unwrap().params.is_empty(), "a layer sets values before any has been tuned");
        }
        assert_eq!(BotParams::layered("Berlin", &[]), d);
        assert_eq!(BotParams::layered("DC_Al_Nas", &[]), d);
        assert_eq!(BotParams::layered("GC_Endor", &[]), d);
        assert_eq!(mod_layer("DC_Al_Nas").unwrap().0, "params/bots/DC_Final.toml");
        assert_eq!(mod_layer("Berlin").unwrap().0, "params/bots/bf1942.toml");
        assert!(mod_layer("GC_Endor").is_none());
        let mut p = d;
        let mut layer = BTreeMap::new();
        layer.insert("rally_m".to_string(), 150.0);
        layer.insert("rally_wait_s".to_string(), 40.0);
        p.apply(&layer).unwrap();
        let mut over = BTreeMap::new();
        over.insert("rally_m".to_string(), 90.0);
        p.apply(&over).unwrap();
        assert_eq!((p.rally_m, p.rally_wait_s, p.rally_near_m), (90.0, 40.0, d.rally_near_m));
        let f = BotParams::layered("Berlin", &[("rally_m".into(), 100.0)]);
        assert_eq!(f.rally_m, 100.0);
        assert!(p.set("rally_m", 10.0).is_err() && p.set("nothing", 1.0).is_err());
        assert!(p.set("urgency_most", 2.5).is_err() && p.set("urgency_most", 2.0).is_ok(), "a count takes whole numbers");
        assert_eq!(p.urgency_most, 2);
        assert!(!f.same_for_team(&d) && d.same_for_team(&BotParams::default()));
    }
}
