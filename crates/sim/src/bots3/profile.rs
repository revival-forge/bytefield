//! Difficulty: how much a player misses, how long it takes to react and how much it risks. A rookie aims badly, reacts
//! slowly and rushes in; a veteran aims somewhat better, reacts sooner and gambles less. It never sees more: a tougher bot is not
//! smarter. Normal aims and reacts like `bots2` (a clumsier one loses against it: with 1.3° of deviation, 17/14 and −3 in 32 rounds).
//! Measured from outside (`quality.rs`: aim includes the weapon's spread, ~1° base): rookie ~4° and ~1.5–2.2 s;
//! normal ~1.15° and ~1.3–1.8 s; veteran ~1° and ~1.2–1.7 s. Selection: `bots3:rookie|normal|veteran` and `BYTEFIELD_BOTS3_PROFILE`.

/// How a bot plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Profile {
    Rookie,
    #[default]
    Normal,
    Veteran,
}

// TODO: the three numbers of each profile are set against bots2 and the measures above, not by players: tune them by
// play-testing. How: `examples/tournament3 --a bots3:veteran --b bots3:rookie` (B17_reaction_s and B17_error_degrees in its
// table) after each change, and a tester's rounds against each profile.
impl Profile {
    /// By its name (`rookie`, `normal`, `veteran`).
    pub fn by_name(n: &str) -> Option<Profile> {
        match n.to_ascii_lowercase().as_str() {
            "rookie" => Some(Profile::Rookie),
            "normal" => Some(Profile::Normal),
            "veteran" => Some(Profile::Veteran),
            _ => None,
        }
    }

    /// Aiming error, degrees (standard deviation).
    pub fn error_degrees(self) -> f32 {
        match self {
            Profile::Rookie => 3.8,
            Profile::Normal => 0.55,
            Profile::Veteran => 0.4,
        }
    }

    /// Time it takes to react to a new enemy (median), seconds.
    pub fn reaction_s(self) -> f32 {
        match self {
            Profile::Rookie => 1.5,
            Profile::Normal => 0.6,
            Profile::Veteran => 0.4,
        }
    }

    /// How much it risks, from 0 (always under cover) to 1 (always in front).
    pub fn aggressiveness(self) -> f32 {
        match self {
            Profile::Rookie => 0.8,
            Profile::Normal => 0.5,
            Profile::Veteran => 0.4,
        }
    }

    /// Below this health it takes cover or retreats: the rookie risks more, the veteran less.
    pub fn low_health(self) -> f32 {
        match self {
            Profile::Rookie => 0.2,
            Profile::Normal => 0.3,
            Profile::Veteran => 0.35,
        }
    }

    /// How long it stays under cover before peeking out again (or retreating), s: the rookie gets impatient.
    pub fn under_cover_s(self) -> f32 {
        match self {
            Profile::Rookie => 8.0,
            Profile::Normal => 15.0,
            Profile::Veteran => 12.0,
        }
    }

    /// At the wheel, how long it waits for the passengers it was given to board before setting off, s.
    pub fn passengers_wait_s(self) -> f32 {
        match self {
            Profile::Rookie => 10.0,
            Profile::Normal => 12.0,
            Profile::Veteran => 15.0,
        }
    }
}
