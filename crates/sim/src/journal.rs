//! What the round counts and logs, apart from what moves it: the shots, the damage, the heals, the spawns, what was
//! made and asked for. The systems append to it as they go; the tests, the scoreboard, `dumb` and the clients read it.

use crate::{damage, players, projectile, radio, supply, vecmath};

/// What the round counts and logs: the output for tests, the scoreboard and the clients, not what moves the round
// TODO: it grows without bound in a long round but for what `Sim::trim_logs` cuts; whether it can be switched off (a
// server that nobody tests) is the user's call.
#[derive(Default)]
pub struct Journal {
    /// the soldiers' shots: (step, vehicle, shot, its projectile's index)
    pub fired: Vec<(u64, usize, projectile::Shot, Option<usize>)>,
    /// how many objects [`Sim::spawn_vehicle`] made ([`crate::mover::Vehicle::born`])
    pub made: u32,
    /// the medics' packs used (the soldier's use med pack that healed someone or themselves)
    pub heals: u64,
    /// those heals by who got them: the medic, a soldier of its team, one of another team (the
    /// pack does not look at the team)
    pub heals_by: [u64; 3],
    /// the wrenches used: a vehicle repaired, a mine or charge
    /// picked up
    pub repairs: u64,
    pub disarmed: u64,
    /// every damage given, every explosion, every object a spawner made (step, spawner, vehicle)
    // (these logs and the others grow for the tests, which read them by length; a server and a client cut them,
    // [`Sim::trim_logs`] and 321, and the client reads them with [`log_since`])
    pub damage_log: Vec<damage::DamageEvent>,
    pub explosions: Vec<(u64, damage::Explosion)>,
    /// every projectile's hit (`handleCollisionForProjectile`): its material and the hit (what a
    /// client shows: `MaterialManager.setEffectTemplate` of the pair)
    pub impacts: Vec<(u32, projectile::Impact)>,
    /// and who fired it: (step, the shooter's vehicle, the vehicle hit, the point)
    pub hit_log: Vec<(u64, Option<usize>, Option<usize>, vecmath::V3)>,
    /// every explosion's push: (step, vehicle, point, acceleration)
    pub pushes: Vec<(u64, usize, [f32; 3], [f32; 3])>,
    /// every vehicle a spawner made after the start: (step, spawner, vehicle)
    pub spawn_log: Vec<(u64, usize, usize)>,
    /// the kits picked up (`kit.rs`): step, soldier, the kit (`items`)
    pub kit_log: Vec<(u64, usize, usize)>,
    /// what [`Sim::instantiate`] was asked to make and does not: `"<kind> <template>"` (`"? …"`:
    /// no such template), times asked
    pub unhandled: std::collections::BTreeMap<String, usize>,
    /// The templates [`Sim::instantiate`] was asked for that exist, in order (the server's objects, made or not here):
    /// the static ghosts' ids follow it.
    pub created: Vec<String>,
    pub player_log: Vec<(u64, usize, players::PlayerEvent)>,
    /// what the players said on the radio
    pub radio_log: Vec<radio::RadioEvent>,
    /// how many entries [`Sim::trim_logs`] cut from the front of the logs a client reads (its cursors count from the
    /// round's start: [`log_since`])
    pub log_base: LogBase,
    /// what they gave
    pub supply_log: Vec<supply::SupplyEvent>,
}

/// How many entries [`Sim::trim_logs`] cut from the front of each log a client reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogBase {
    pub damage: usize,
    pub explosions: usize,
    pub impacts: usize,
    pub fired: usize,
    pub player: usize,
    pub radio: usize,
}

/// What a reader has not seen of a log that [`Sim::trim_logs`] cuts from the front (`base` entries cut), with its cursor
/// `seen` counted from the round's start, and moves the cursor to the end. A cursor past the end (another round, a log
/// started again) reads the whole log; one behind the cut, what is left.
pub fn log_since<'a, T>(log: &'a [T], base: usize, seen: &mut usize) -> &'a [T] {
    let end = base + log.len();
    let from = if end < *seen { 0 } else { seen.saturating_sub(base).min(log.len()) };
    *seen = end;
    &log[from..]
}
