//! Switches for studying the engine: trace output and a few experiments to compare against. None of them is part
//! of a round: unset, the simulation is the one the tests and the scoreboard measure.
//!
//! They come from the process environment, read once by [`DebugOptions::from_env`] (the only place of the engine that
//! reads it; the AI keeps its own, and the tests against the original's traces read a few switches of their own, for
//! the dump of a failing comparison). A program that wants others, a test or an embedder, calls [`set`]
//! before the first simulation; otherwise the first [`options`] reads the environment. Reading them once keeps the
//! environment out of the hot paths, and makes plain what a run depends on.

use std::sync::OnceLock;

/// What a run was asked to trace or to change. `Default` is the plain engine.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugOptions {
    /// `BYTEFIELD_CONTACT_ORDER=0`: the contactors in the template's order, to compare (on by default: the original's order).
    pub contact_order: bool,
    /// `BYTEFIELD_CONQUEST_SPAWN_MAP=0`: conquest's bots take any spawn point, also those off the AI map.
    pub conquest_spawn_map: bool,
    /// `BYTEFIELD_NAVGEN=0`: a level without its navigation maps gets none made.
    pub navgen: bool,
    /// `BYTEFIELD_NAV_TANKS=gridswim`: what swims drives over a tank grid of its own, with the water open and dearer than land,
    /// instead of the driver's shortcuts across it (off by default: under trial).
    pub nav_tanks_swim: bool,
    /// `BYTEFIELD_AI_DEBUG_ENTRY=1`: each bot that asks to enter a vehicle, what it asked and what it found (stderr).
    pub trace_entry: bool,
    /// `BYTEFIELD_BOTS_PROFILE=rookie|normal|veteran`: bots's difficulty (normal when unset): how `--bot-ai bots` picks it.
    pub bots_profile: Option<String>,
    /// `BYTEFIELD_BOTS_PARAMS=<file>`: a layer of the bots' named parameters (`[params]`) over the built-in ones
    /// (`bots::params::BotParams::layered`).
    pub bots_params: Option<String>,
    /// `BYTEFIELD_BOTS_MEMORY_TRACE=1`: with `bots:memory`, each side's tolls as they are made (stderr, [`crate::bots::memory`]).
    pub bots_memory_trace: bool,
    /// `BYTEFIELD_BOTS_RADIO=1`: each message a bots bot says and each request it heeds (stderr).
    pub bots_radio: bool,
    /// `BYTEFIELD_BOTS_MODS=1`: what player 0 sees on starting and the kind of map it yields (stderr, mod maps).
    pub bots_mods: bool,
    /// `BYTEFIELD_BOTS_TRACE=1`: the team writes its squads and orders on every split (stderr).
    pub bots_team: bool,
    /// `BYTEFIELD_BOTS_ME=<player>`: that player writes its order, seat and axes twice a second (stderr).
    pub bots_me: Option<usize>,
    /// `BYTEFIELD_BOTS_COARSE=1`: the path grid blocks a cell with any blocked pixel, as before the 1 m passages were kept (to measure).
    pub bots_coarse: bool,
    /// `BYTEFIELD_BOTS_ACTIONS=1`: each action change of each player, with what decided it (stderr).
    pub bots_actions: bool,
    /// `BYTEFIELD_BOTS_WOUNDED=1`: each low-health player writes once a second its action and what it sees (stderr).
    pub bots_wounded: bool,
    /// `BYTEFIELD_BOTS_SHOT=1`: each tick a player on foot presses the trigger: its action, weapon, target and whether a
    /// wall hides the target from the eye and from somewhat lower down (stderr).
    pub bots_shots: bool,
    /// `BYTEFIELD_BOTS_AIR=1`: each pilot in the air, a line a second: its target and what it lacks to fire, or, with no
    /// target, how many enemies are within range and how many in front (stderr).
    pub bots_air: bool,
    /// `BYTEFIELD_QUALITY_B13=1`: each B13 sample of the quality meter (a side's flag with an enemy near): when, which flag and
    /// side, and who of that side is inside it: id, on foot or in which vehicle, x, z (stderr).
    pub quality_b13: bool,
    /// `BYTEFIELD_QUALITY_B14=1`: each closed B14 case of the quality meter: when, who, whether it moved, looked or heard (stderr).
    pub quality_b14: bool,
    /// `BYTEFIELD_QUALITY_B16=1`: each still or stuck run of B16 as it closes: when, who and which side, the kind, how long, on foot
    /// or in what, how much of it was fighting and where (stderr).
    pub quality_b16: bool,
    /// `BYTEFIELD_QUALITY_B21=1`: each closed B21 arrival: when, which vehicle and flag, how close the vehicle and its crew are to
    /// the flag on closing, and whether the crew got inside (stderr).
    pub quality_b21: bool,
    /// `BYTEFIELD_BOTS_STREETWISE=<player>:<trait>`: that player's streetwise trait is this one instead of the drawn one (to
    /// study one bot; it changes how that bot plays).
    pub bots_streetwise: Option<(usize, f32)>,
    /// `BYTEFIELD_COVER_TRACE=1`: what `bots:cover-flank` does, once a second per bot (stderr).
    pub bots_cover: bool,
    /// `BYTEFIELD_AMBUSH_TRACE=1`: each ambush a bot sets for an enemy's habit: where, how far, how long it waits (stderr).
    pub bots_ambush: bool,
    /// `BYTEFIELD_TPS_TRACE=<player>|all`: the tactical position searches of that player, or of every one (stderr).
    pub bots_tps: Option<usize>,
    /// `BYTEFIELD_SPOT_TRACE=1`: what became of each firing spot a bot chose, and its time stopped to fight per life (stderr,
    /// [`crate::bots::spot_trace`]).
    pub bots_spot: bool,
    /// `BYTEFIELD_FIRE_TRACE=1`: why a bot on foot does not fire with an enemy in reach, per contact and per life (stderr,
    /// [`crate::bots::fire_trace`]).
    pub bots_fire: bool,
    /// `BYTEFIELD_VTOL_FINE=1`, with `bots_air`: each frame of a VTOL's takeoff and each leg of its way (stderr).
    pub bots_vtol_fine: bool,
    /// `NAVMESH_LINK_TIMES=1`: how long building the navigation mesh's ladders, drops and jumps took (stderr).
    pub navmesh_link_times: bool,
}

impl Default for DebugOptions {
    fn default() -> Self {
        DebugOptions {
            contact_order: true,
            conquest_spawn_map: true,
            navgen: true,
            nav_tanks_swim: false,
            trace_entry: false,
            bots_profile: None,
            bots_params: None,
            bots_memory_trace: false,
            bots_radio: false,
            bots_mods: false,
            bots_team: false,
            bots_me: None,
            bots_coarse: false,
            bots_actions: false,
            bots_wounded: false,
            bots_shots: false,
            bots_air: false,
            quality_b13: false,
            quality_b14: false,
            quality_b16: false,
            quality_b21: false,
            bots_streetwise: None,
            bots_cover: false,
            bots_ambush: false,
            bots_tps: None,
            bots_spot: false,
            bots_fire: false,
            bots_vtol_fine: false,
            navmesh_link_times: false,
        }
    }
}

impl DebugOptions {
    /// The options the process environment asks for (see each field).
    pub fn from_env() -> Self {
        let set = |k: &str| std::env::var_os(k).is_some();
        let not_zero = |k: &str| std::env::var_os(k).is_none_or(|v| v != "0");
        DebugOptions {
            contact_order: not_zero("BYTEFIELD_CONTACT_ORDER"),
            conquest_spawn_map: not_zero("BYTEFIELD_CONQUEST_SPAWN_MAP"),
            navgen: not_zero("BYTEFIELD_NAVGEN"),
            nav_tanks_swim: std::env::var("BYTEFIELD_NAV_TANKS").is_ok_and(|v| v == "gridswim"),
            trace_entry: set("BYTEFIELD_AI_DEBUG_ENTRY"),
            bots_profile: std::env::var("BYTEFIELD_BOTS_PROFILE").ok(),
            bots_params: std::env::var("BYTEFIELD_BOTS_PARAMS").ok(),
            bots_memory_trace: set("BYTEFIELD_BOTS_MEMORY_TRACE"),
            bots_radio: set("BYTEFIELD_BOTS_RADIO"),
            bots_mods: set("BYTEFIELD_BOTS_MODS"),
            bots_team: set("BYTEFIELD_BOTS_TRACE"),
            bots_me: std::env::var("BYTEFIELD_BOTS_ME").ok().and_then(|x| x.parse().ok()),
            bots_coarse: set("BYTEFIELD_BOTS_COARSE"),
            bots_actions: set("BYTEFIELD_BOTS_ACTIONS"),
            bots_wounded: set("BYTEFIELD_BOTS_WOUNDED"),
            bots_shots: set("BYTEFIELD_BOTS_SHOT"),
            bots_air: set("BYTEFIELD_BOTS_AIR"),
            quality_b13: set("BYTEFIELD_QUALITY_B13"),
            quality_b14: set("BYTEFIELD_QUALITY_B14"),
            quality_b16: set("BYTEFIELD_QUALITY_B16"),
            quality_b21: set("BYTEFIELD_QUALITY_B21"),
            bots_streetwise: std::env::var("BYTEFIELD_BOTS_STREETWISE").ok().and_then(|v| {
                let (p, x) = v.split_once(':')?;
                Some((p.parse().ok()?, x.parse().ok()?))
            }),
            bots_cover: set("BYTEFIELD_COVER_TRACE"),
            bots_ambush: set("BYTEFIELD_AMBUSH_TRACE"),
            bots_tps: std::env::var("BYTEFIELD_TPS_TRACE").ok().and_then(|x| if x == "all" { Some(usize::MAX) } else { x.parse().ok() }),
            bots_spot: set("BYTEFIELD_SPOT_TRACE"),
            bots_fire: set("BYTEFIELD_FIRE_TRACE"),
            bots_vtol_fine: set("BYTEFIELD_VTOL_FINE"),
            navmesh_link_times: set("NAVMESH_LINK_TIMES"),
        }
    }
}

// The options are the process's, not each `Sim`'s: two simulations in one process cannot trace differently. Kept so on
// purpose: nobody needs it, and the bots, which read most of them, have no `&Sim` to take them from.
static OPTIONS: OnceLock<DebugOptions> = OnceLock::new();

/// The options of this process: those given to [`set`], or the environment's.
pub fn options() -> &'static DebugOptions {
    OPTIONS.get_or_init(DebugOptions::from_env)
}

/// Gives the options before the first simulation reads them; once read they stay (the old ones come back as `Err`).
pub fn set(options: DebugOptions) -> Result<(), DebugOptions> {
    OPTIONS.set(options)
}
