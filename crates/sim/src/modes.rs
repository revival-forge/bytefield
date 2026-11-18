//! The game mode's state, set by the mode's `start_*` (`None` where the mode has none): conquest's control points and
//! tickets, capture the flag's flags, objective mode's objectives. Each mode's module moves its own.

use crate::{conquest, ctf, objective};

/// The game mode's state: conquest's control points and tickets, capture the flag's flags, the objectives
#[derive(Default)]
pub struct Modes {
    /// control points and tickets, once `start_conquest` set them up ([`conquest`])
    pub conquest: Option<conquest::Conquest>,
    /// the flags, in capture the flag ([`ctf`])
    pub ctf: Option<ctf::Ctf>,
    /// the objectives, in objective mode ([`objective`])
    pub objectives: Option<objective::Objectives>,
}
