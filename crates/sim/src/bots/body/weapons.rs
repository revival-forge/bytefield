//! What class of weapon the bot carries and at what rhythm it uses it (specification, «The rhythm depends on the weapon»).
use super::rng::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponClass {
    /// the trigger is held: short bursts
    Automatic,
    /// one shot every 2.9 s at least
    Bolt,
    /// at least 6.7 s between shots
    Rocket,
    Pistol,
    Granada,
    /// knife, medkit, wrench, explosives…: not fired
    Another,
}

/// The class from the weapon's name and from whether each press gives a single shot.
pub fn class(name: &str, one_shot: bool) -> WeaponClass {
    let n = name.to_ascii_lowercase();
    let has = |l: &[&str]| l.iter().any(|x| n.contains(x));
    if has(&["grenade"]) {
        WeaponClass::Granada
    } else if has(&["knife", "medpack", "repairpack", "binocular", "landmine", "exppack", "detonator", "explosive"]) {
        WeaponClass::Another
    } else if has(&["bazooka", "panzershreck", "panzerschreck", "piat", "rocket"]) {
        WeaponClass::Rocket
    } else if has(&["colt", "walter", "luger", "tokarev", "nambu", "pistol", "tt33"]) {
        WeaponClass::Pistol
    } else if one_shot {
        WeaponClass::Bolt
    } else {
        WeaponClass::Automatic
    }
}

/// bots: whether a weapon is anti-tank (a shoulder-fired launcher), by name: those of BF1942, DC and GC.
pub fn is_anti_tank(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["bazooka", "panzershreck", "panzerschreck", "piat", "rocket", "rpg", "at4", "smaw", "carlgustav", "carl_gustav", "law", "plx", "flechette"].iter().any(|x| n.contains(x)) && !n.contains("grenade")
}

/// bots: whether a weapon is a pistol, by name (those of BF1942 and DC, which [`class`] does not know).
pub fn is_pistol(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    class(name, true) == WeaponClass::Pistol || ["beretta", "browning", "hipo", "makarov", "glock", "m1911", "dh-17", "dl44"].iter().any(|x| n.contains(x))
}

/// The game's weapon slots (`c_PIWeapon1..`): the primary, the pistol and the grenade.
pub const PRIMARY_SLOT: usize = 3;
pub const PISTOL_SLOT: usize = 2;
pub const GRENADE_SLOT: usize = 4;

/// How many shots the next burst of an automatic weapon fires: median 2 and 90th percentile between 5 and 7.
pub fn burst(a: &mut Rng) -> u32 {
    1 + (a.exponential(2.8) as u32).min(11)
}

/// Minimum seconds between one shot and the next, by class.
pub fn interval(c: WeaponClass, a: &mut Rng) -> f32 {
    match c {
        WeaponClass::Bolt => (a.lognormal(4.8, 0.35)).max(2.9),
        WeaponClass::Rocket => (a.lognormal(8.0, 0.25)).max(6.7),
        WeaponClass::Pistol => (a.lognormal(1.2, 0.4)).max(0.4),
        _ => 0.0,
    }
}

/// Mean pause between two bursts by distance to the target: the closer, the more it fires.
/// `scale` adjusts the whole (it is what gives the shots per round).
pub fn pause(distance: f32, reach: f32, scale: f32, a: &mut Rng) -> f32 {
    // the specification's table gives 21 % of seconds with a shot at <10 m and 5.5 % at 40–55 m: about four times fewer; 2.5 gives the 90th percentile of firing distance (row 6)
    let x = (distance / reach.max(1.0)).clamp(0.0, 1.0);
    let mean = scale * (1.0 + 2.5 * x);
    a.lognormal(mean, 0.5)
}

/// `bots:fire-rhythm`: a burst by distance, as a person fires: about 10 rounds at point blank, 7 at half the reach and the
/// original's (median 2) at the limit; at most 20.
/// `near` scales how much longer it is near (the named parameter `rhythm_burst`, 1 by default).
pub fn burst_near(distance: f32, reach: f32, near: f32, a: &mut Rng) -> u32 {
    let x = (distance / reach.max(1.0)).clamp(0.0, 1.0);
    1 + (a.exponential(2.8 + near * 6.0 * (1.0 - x)) as u32).min(19)
}

/// `bots:fire-rhythm`: the pause between bursts, a quarter of a second at point blank to 1.75 s at the limit (the default's, 1 to
/// 3.5 s); `scale` as in [`pause`].
pub fn pause_near(distance: f32, reach: f32, scale: f32, a: &mut Rng) -> f32 {
    let x = (distance / reach.max(1.0)).clamp(0.0, 1.0);
    a.lognormal(scale * (0.25 + 1.5 * x), 0.5)
}

/// `bots:fire-rhythm`: seconds between shots of a one-shot weapon at its own cycle, a little longer far off to aim: a bolt-action
/// or a pump about 1.4 to 2.2 s, a pistol 0.4 to 0.8 s (the simulation keeps the weapon's own rate). A rocket keeps [`interval`].
pub fn interval_near(c: WeaponClass, distance: f32, reach: f32, a: &mut Rng) -> f32 {
    let x = (distance / reach.max(1.0)).clamp(0.0, 1.0);
    match c {
        WeaponClass::Bolt => a.lognormal(1.4 + 0.8 * x, 0.25).max(1.1),
        WeaponClass::Pistol => a.lognormal(0.4 + 0.4 * x, 0.3).max(0.3),
        _ => interval(c, a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_of_berlin_weapons() {
        assert_eq!(class("Mp40", false), WeaponClass::Automatic);
        assert_eq!(class("DP", false), WeaponClass::Automatic);
        assert_eq!(class("K98", true), WeaponClass::Bolt);
        assert_eq!(class("No4Sniper", true), WeaponClass::Bolt);
        assert_eq!(class("Panzershreck", true), WeaponClass::Rocket);
        assert_eq!(class("Bazooka", true), WeaponClass::Rocket);
        assert_eq!(class("WalterP38", true), WeaponClass::Pistol);
        assert_eq!(class("Colt", true), WeaponClass::Pistol);
        assert_eq!(class("GrenadeAxis", true), WeaponClass::Granada);
        assert_eq!(class("KnifeAllies", true), WeaponClass::Another);
        assert_eq!(class("MedPack", false), WeaponClass::Another);
        assert_eq!(class("ExpPack", true), WeaponClass::Another);
    }

    #[test]
    fn anti_tanks() {
        assert!(is_anti_tank("Bazooka") && is_anti_tank("Panzershreck") && is_anti_tank("RPG7") && is_anti_tank("AT4"));
        assert!(is_pistol("Colt") && is_pistol("M9_beretta") && is_pistol("Browninghipo") && !is_pistol("M249"));
        assert!(!is_anti_tank("Mp40") && !is_anti_tank("Colt") && !is_anti_tank("GrenadeAllies") && !is_anti_tank("K98"));
    }

    #[test]
    fn bursts_like_the_observed_ones() {
        let mut a = Rng::new(7);
        let mut v: Vec<u32> = (0..2000).map(|_| burst(&mut a)).collect();
        v.sort();
        assert_eq!(v[1000], 2, "median");
        assert!((5..=7).contains(&v[1800]), "p90 {}", v[1800]);
    }

    #[test]
    fn minimum_between_shots() {
        let mut a = Rng::new(3);
        for _ in 0..500 {
            assert!(interval(WeaponClass::Bolt, &mut a) >= 2.9);
            assert!(interval(WeaponClass::Rocket, &mut a) >= 6.7);
        }
    }

    #[test]
    fn a_person_fires_more_near() {
        let mut a = Rng::new(11);
        let mean_burst = |d: f32, a: &mut Rng| (0..2000).map(|_| burst_near(d, 55.0, 1.0, a) as f32).sum::<f32>() / 2000.0;
        let (near, far) = (mean_burst(3.0, &mut a), mean_burst(55.0, &mut a));
        assert!(near > 7.0 && far < 4.5 && far > 3.0, "{near} {far}");
        let mean_pause = |d: f32, a: &mut Rng| (0..2000).map(|_| pause_near(d, 55.0, 1.0, a)).sum::<f32>() / 2000.0;
        assert!(mean_pause(3.0, &mut a) < 0.5 && mean_pause(55.0, &mut a) < 2.2);
        for _ in 0..500 {
            assert!(interval_near(WeaponClass::Bolt, 5.0, 55.0, &mut a) >= 1.1);
            assert!(interval_near(WeaponClass::Rocket, 5.0, 55.0, &mut a) >= 6.7);
        }
    }

    #[test]
    fn the_closer_the_shorter_the_pause() {
        let mean = |d: f32| {
            let mut a = Rng::new(5);
            (0..2000).map(|_| pause(d, 55.0, 2.0, &mut a)).sum::<f32>() / 2000.0
        };
        assert!(mean(5.0) * 2.0 < mean(50.0));
    }
}
