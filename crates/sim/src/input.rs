//! A player's input axes: what every player's goes through, a person's or a bot's, and their names in the `.con` scripts.

/// One of the first six axes (yaw, pitch, roll, throttle and the two of looking) after a player's action carries it: a 12-bit
/// fixed point over ±16, as it travels, then rounded to hundredths. The other axes are buttons and go through as they are.
pub fn through_action(v: f32) -> f32 {
    let t = (v as f64 / 16.0).clamp(-1.0, 1.0);
    let s = ((t + 1.0) * 0.5) as f32;
    let q = (s as f64 * 4095.0) as i64;
    let a = ((q as f64 / 4095.0) * 2.0 - 1.0) * 16.0;
    let b = (a as f32 as f64 * 100.0) as f32;
    ((b.round_ties_even() as i32) as f64 / 100.0) as f32
}

/// A player's axes after [`through_action`].
pub fn through_actions(axes: &[(usize, f32)]) -> Vec<(usize, f32)> {
    axes.iter().map(|&(a, v)| (a, if a < 6 { through_action(v) } else { v })).collect()
}

/// An input by its script name (`PIMenuSelect2`, with or without `c_`): the number the game gives it.
pub fn player_input(name: &str) -> Option<i32> {
    const T: [(&str, i32); 41] = [
        ("PIYaw", 0), ("PIPitch", 1), ("PIRoll", 2), ("PIThrottle", 3), ("PIMouseLookX", 4), ("PIMouseLookY", 5), ("PICameraX", 6),
        ("PICameraY", 7), ("PIFire", 8), ("PIAction", 9), ("PIUse", 10), ("PIMouseLook", 11), ("PIWalk", 12), ("PIRun", 13),
        ("PIMenuSelect1", 14), ("PIMenuSelect2", 15), ("PIMenuSelect3", 16), ("PIMenuSelect4", 17), ("PIMenuSelect5", 18),
        ("PIMenuSelect6", 19), ("PIMenuSelect7", 20), ("PIMenuSelect8", 21), ("PIMenuSelect9", 22), ("PIAltFire", 23), ("PIReload", 24),
        ("PIDrop", 25), ("PIToggleCameraMode", 26), ("PIToggleCamera", 27), ("PILie", 28), ("PICrouch", 29), ("PISayAll", 44),
        ("PISayTeam", 45), ("PINextItem", 46), ("PIPrevItem", 47), ("PICommunication", 48), ("PIMap", 50), ("PIZoomMap", 51),
        ("PINone", 55), ("PIFire2", 8), ("PIScreenShot", -1), ("PIChangeView", -1),
    ];
    let n = name.strip_prefix("c_").unwrap_or(name);
    T.iter().find(|(t, v)| *v >= 0 && t.eq_ignore_ascii_case(n)).map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_six_axes_are_packed_and_the_buttons_are_not() {
        assert_eq!((through_action(20.0), through_action(-20.0)), (16.0, -16.0));
        assert_eq!(through_actions(&[(0, 20.0), (8, 20.0)]), vec![(0, 16.0), (8, 20.0)]);
    }

    #[test]
    fn inputs_by_name() {
        assert_eq!((player_input("c_PIFire"), player_input("PIReload"), player_input("PIScreenShot"), player_input("nope")), (Some(8), Some(24), None, None));
    }
}
