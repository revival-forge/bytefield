//! Switches for studying the engine: trace output and a few experiments to compare against. None of them is part
//! of a round: unset, the simulation is the one the tests and the scoreboard measure.
//!
//! They come from the process environment, read once by [`DebugOptions::from_env`] (the only place of the engine that
//! reads it; the AI keeps its own). A program that wants others, a test or an embedder, calls [`set`]
//! before the first simulation; otherwise the first [`options`] reads the environment. Reading them once keeps the
//! environment out of the hot paths, and makes plain what a run depends on.

use std::sync::OnceLock;

/// What a run was asked to trace or to change. `Default` is the plain engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugOptions {
    /// `BYTEFIELD_CONTACT_ORDER=0`: the contactors in the template's order, to compare (on by default: the original's order).
    pub contact_order: bool,
    /// `BYTEFIELD_CONQUEST_SPAWN_MAP=0`: conquest's bots take any spawn point, also those off the AI map.
    pub conquest_spawn_map: bool,
    /// `BYTEFIELD_NAVGEN=0`: a level without its navigation maps gets none made.
    pub navgen: bool,
    /// `BYTEFIELD_AI_DEBUG_ENTRY=1`: each bot that asks to enter a vehicle, what it asked and what it found (stderr).
    pub trace_entry: bool,
    /// `BYTEFIELD_BOTS3_PROFILE=rookie|normal|veteran`: bots3's difficulty (normal when unset): how `--bot-ai bots3` picks it.
    pub bots3_profile: Option<String>,
    /// `BYTEFIELD_BOTS3_RADIO=1`: each message a bots3 bot says and each request it heeds (stderr).
    pub bots3_radio: bool,
    /// `BYTEFIELD_BOTS3_MODS=1`: what player 0 sees on starting and the kind of map it yields (stderr, mod maps).
    pub bots3_mods: bool,
    /// `BYTEFIELD_BOTS3_TRACE=1`: the team writes its squads and orders on every split (stderr).
    pub bots3_team: bool,
    /// `BYTEFIELD_BOTS3_ME=<player>`: that player writes its order, seat and axes twice a second (stderr).
    pub bots3_me: Option<usize>,
    /// `BYTEFIELD_BOTS3_ACTIONS=1`: each action change of each player, with what decided it (stderr).
    pub bots3_actions: bool,
    /// `BYTEFIELD_BOTS3_WOUNDED=1`: each low-health player writes once a second its action and what it sees (stderr).
    pub bots3_wounded: bool,
    /// `BYTEFIELD_BOTS3_SHOT=1`: each tick a player on foot presses the trigger: its action, weapon, target and whether a
    /// wall hides the target from the eye and from somewhat lower down (stderr).
    pub bots3_shots: bool,
    /// `BYTEFIELD_BOTS3_AIR=1`: each pilot in the air, a line a second: its target and what it lacks to fire, or, with no
    /// target, how many enemies are within range and how many in front (stderr).
    pub bots3_air: bool,
    /// `BYTEFIELD_QUALITY_B14=1`: each closed B14 case of the quality meter: when, who, whether it moved, looked or heard (stderr).
    pub quality_b14: bool,
    /// `BYTEFIELD_QUALITY_B21=1`: each closed B21 arrival: when, which vehicle and flag, how close the vehicle and its crew are to
    /// the flag on closing, and whether the crew got inside (stderr).
    pub quality_b21: bool,
}

impl Default for DebugOptions {
    fn default() -> Self {
        DebugOptions {
            contact_order: true,
            conquest_spawn_map: true,
            navgen: true,
            trace_entry: false,
            bots3_profile: None,
            bots3_radio: false,
            bots3_mods: false,
            bots3_team: false,
            bots3_me: None,
            bots3_actions: false,
            bots3_wounded: false,
            bots3_shots: false,
            bots3_air: false,
            quality_b14: false,
            quality_b21: false,
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
            trace_entry: set("BYTEFIELD_AI_DEBUG_ENTRY"),
            bots3_profile: std::env::var("BYTEFIELD_BOTS3_PROFILE").ok(),
            bots3_radio: set("BYTEFIELD_BOTS3_RADIO"),
            bots3_mods: set("BYTEFIELD_BOTS3_MODS"),
            bots3_team: set("BYTEFIELD_BOTS3_TRACE"),
            bots3_me: std::env::var("BYTEFIELD_BOTS3_ME").ok().and_then(|x| x.parse().ok()),
            bots3_actions: set("BYTEFIELD_BOTS3_ACTIONS"),
            bots3_wounded: set("BYTEFIELD_BOTS3_WOUNDED"),
            bots3_shots: set("BYTEFIELD_BOTS3_SHOT"),
            bots3_air: set("BYTEFIELD_BOTS3_AIR"),
            quality_b14: set("BYTEFIELD_QUALITY_B14"),
            quality_b21: set("BYTEFIELD_QUALITY_B21"),
        }
    }
}

// TODO: the options are the process's, not each `Sim`'s: two simulations in one process cannot trace differently. Nobody
// needs it today; a server running several rounds at once would pass `&DebugOptions` to `Sim::new` (they are `Clone`).
static OPTIONS: OnceLock<DebugOptions> = OnceLock::new();

/// The options of this process: those given to [`set`], or the environment's.
pub fn options() -> &'static DebugOptions {
    OPTIONS.get_or_init(DebugOptions::from_env)
}

/// Gives the options before the first simulation reads them; once read they stay (the old ones come back as `Err`).
pub fn set(options: DebugOptions) -> Result<(), DebugOptions> {
    OPTIONS.set(options)
}
