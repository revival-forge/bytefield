//! bots3 profiles for the mods (Desert Combat and Galactic Conquest), chosen from what a player sees on starting the round
//! (how many flags there are, how far apart, which vehicles are in sight): not from a list of maps.
//!
//! A player arriving on a new map classifies it at a glance: if the flags are close together it is a city (fighting is close up,
//! no point looking for a car), if there are walkers or the flags are very far apart the car is the way to go, and if there are boats or
//! planes it is sea and air. bots2 does not go through here: it follows [`MapProfile::de`].
use super::body::{MapProfile, OPEN};
use super::squad::Levers;
use crate::bot_api::{BotView, VehicleClass};

/// The kind of map, by what is seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponClass {
    /// flags close together: close-up combat, vehicles of little use
    City,
    /// distant flags, with ground vehicles: like the base game's open maps
    Open,
    /// boats among the vehicles in sight: a large part of the map is water
    AirSea,
    /// flags over a kilometre apart, or walkers (AT-AT, AT-ST): you cannot get there without a vehicle
    Far,
}

/// What is seen on starting: how many flags, how far each is from the nearest one, and the vehicles.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub flags: usize,
    /// median of each flag's distance to the nearest one, m
    pub neighbour_m: f32,
    /// greatest distance between two flags, m
    pub width_m: f32,
    pub on_ground: usize,
    pub air: usize,
    pub sea: usize,
    pub walkers: usize,
}

/// A walker by its template: only the fallback for [`VehicleClass::Walker`], which the API gives by shape (legs without springs and many
/// joints, [`VehicleClass::of`]).
pub fn is_walker(template: &str) -> bool {
    let n = template.to_ascii_lowercase();
    ["atat", "at-at", "atst", "at-st", "walker", "atrt", "at-rt"].iter().any(|k| n.contains(k))
}

impl Summary {
    pub fn de(v: &BotView) -> Summary {
        let flags: Vec<[f32; 2]> = v.flags().iter().filter(|f| f.takeable).map(|f| [f.pos[0], f.pos[2]]).collect();
        let d = |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        let mut neighbours: Vec<f32> = flags.iter().enumerate().filter_map(|(i, a)| flags.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, b)| d(*a, *b)).reduce(f32::min)).collect();
        neighbours.sort_by(|a, b| a.total_cmp(b));
        let wide = flags.iter().flat_map(|a| flags.iter().map(move |b| d(*a, *b))).fold(0.0f32, f32::max);
        let mut r = Summary { flags: flags.len(), neighbour_m: neighbours.get(neighbours.len() / 2).copied().unwrap_or(0.0), width_m: wide, ..Summary::default() };
        for x in v.vehicles().iter().filter(|x| !x.fixed) {
            match x.class {
                VehicleClass::Plane | VehicleClass::Helicopter | VehicleClass::Airship => r.air += 1,
                VehicleClass::Ship | VehicleClass::Submarine => r.sea += 1,
                VehicleClass::Tank | VehicleClass::Car | VehicleClass::Walker | VehicleClass::Other => {
                    r.on_ground += 1;
                    if x.class == VehicleClass::Walker || is_walker(&x.template) {
                        r.walkers += 1;
                    }
                }
                VehicleClass::Fixed => {}
            }
        }
        r
    }
}

/// Flags farther apart than this, on average: you do not walk.
const FAR_M: f32 = 550.0;
/// Flags closer than this: city.
const NEAR_M: f32 = 150.0;

pub fn classify(r: &Summary) -> WeaponClass {
    if r.walkers >= 2 || r.neighbour_m > 2.0 * FAR_M {
        WeaponClass::Far
    } else if r.sea >= 3 {
        WeaponClass::AirSea
    } else if r.flags > 0 && r.neighbour_m < NEAR_M {
        WeaponClass::City
    } else {
        WeaponClass::Open
    }
}

/// The profile owed to each class (assumptions measured in tournaments).
pub fn profile(c: WeaponClass) -> MapProfile {
    match c {
        WeaponClass::City => MapProfile { foot_range: 70.0, vehicle_range: 120.0, p_board: 0.40, board_wait: 15.0, ..OPEN },
        WeaponClass::Open => OPEN,
        WeaponClass::AirSea => MapProfile { vehicle_range: 150.0, p_board: 0.47, board_wait: 14.0, ..OPEN },
        WeaponClass::Far => MapProfile { foot_range: 100.0, vehicle_range: 250.0, p_board: 0.97, board_wait: 6.0, ..OPEN },
    }
}

/// Whether the map belongs to a mod (DC_ or GC_): only there is what is seen consulted; base-game ones keep their profile.
pub fn is_mod_level(map: &str) -> bool {
    let m = map.to_ascii_lowercase();
    m.starts_with("dc_") || m.starts_with("gc_")
}


/// The team-level levers that apply to each class (bots2's go in [`profile`]).
pub fn levers(c: WeaponClass) -> Levers {
    let base = Levers::default();
    match c {
        // flags close together: urgency (B18) calls the mates in time, and the squad that stays on guard at the front flag
        // is people who do not attack. In DC_Al_Nas side 2 left one of four in Housing and never reached Complex. Measured:
        // bots3 against itself on Al_Nas, 16 seeds, bias +86 → −4; against bots2 on six City maps, 8 seeds, 35/60 →
        // 42/53 (Al_Nas +40 → +100, Basrahs −7 → +20)
        // (GC_Bespin, City with aircraft only: each base is a platform 260 m up; its squads land beside the flags in the six-seat
        // `cloudbus`, an airship: [`Pilot::fly_airship`])
        WeaponClass::City => Levers { guard: false, ..base },
        // where vehicles are the way to go, the squad looks for them farther (the base and the vehicles are not side by side)
        WeaponClass::Far => Levers { vehicle_near: 150.0, ..base },
        _ => base,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn r(flags: usize, neighbour: f32) -> Summary {
        Summary { flags, neighbour_m: neighbour, width_m: neighbour * 4.0, on_ground: 6, ..Summary::default() }
    }

    #[test]
    fn one_kind_per_map() {
        assert_eq!(classify(&r(6, 90.0)), WeaponClass::City);
        assert_eq!(classify(&r(5, 400.0)), WeaponClass::Open);
        assert_eq!(classify(&Summary { sea: 4, ..r(5, 400.0) }), WeaponClass::AirSea);
        assert_eq!(classify(&Summary { walkers: 3, ..r(5, 400.0) }), WeaponClass::Far);
        assert_eq!(classify(&r(4, 1500.0)), WeaponClass::Far);
    }

    #[test]
    fn walkers_by_template() {
        assert!(is_walker("AT-AT") && is_walker("atst_imp") && !is_walker("speederbike") && !is_walker("humvee"));
    }

    #[test]
    fn different_profiles_and_the_open_one_equal() {
        assert_eq!(profile(WeaponClass::Open), OPEN);
        assert!(profile(WeaponClass::City).foot_range < profile(WeaponClass::Far).vehicle_range);
        assert!(profile(WeaponClass::Far).p_board > profile(WeaponClass::City).p_board);
        assert!(is_mod_level("GC_Hoth") && is_mod_level("DC_Al_Nas") && !is_mod_level("Berlin"));
    }
}
