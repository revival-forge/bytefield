//! The simulation's clock: the fixed step, the world time and the steps taken. Only `Sim::step` and the round's start and end
//! move it; the systems read it (it is `Copy`).

/// The fixed step and how far the round has gone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    /// the step's length in seconds
    pub dt: f64,
    /// the world time in seconds (it stays 0 in the pre-game, as on an empty dedicated server)
    pub time: f64,
    /// the steps taken
    pub steps: u64,
}
