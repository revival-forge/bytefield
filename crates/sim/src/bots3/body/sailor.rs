//! Riding a boat: whoever has the helm goes straight to the zone as far as the shore (there is no water map), whoever does not
//! takes it if free, and on touching land lowers the landing craft's ramp and disembarks running towards the bow. Big ships
//! stay within range of the coast with their guns.
use super::walker::Walker;
use super::angle_between;
use crate::bot_api::{BotView, Inside, VehicleClass, VehicleSeen};
use crate::soldier::axis;

/// Size from which a boat does not disembark: it stays off the coast, m.
pub const BIG_SHIP_RADIUS: f32 = 25.0;
/// The axis that lowers a landing craft's ramp, and how long it takes: turns at 45 °/s up to 90°, s.
pub const RAMP_AXIS: usize = 1;
pub const RAMP_WAIT: f64 = 2.5;
/// Seconds it runs towards the bow on leaving a landing craft, to leave it behind.
pub const LEAVE_BY_BOW: f64 = 3.0;

/// How the first part of `arrive` ends.
#[derive(Debug, PartialEq)]
pub enum Arrival {
    /// must get off the boat
    Low,
    /// nothing more to do this tick
    Fin,
    /// stays aboard; `true` if it is on the shore
    Follows(bool),
}

#[derive(Debug, Default)]
pub struct Sailor {
    /// is on a boat: what is underneath on boarding is water
    pub boat: bool,
    /// since when it has been on the shore with the landing craft
    pub ramp_since: Option<f64>,
    /// got off a landing craft: where it runs to, and until when
    pub leave_by: Option<([f32; 2], f64)>,
}

impl Sailor {
    /// On boarding a vehicle: whether it is a boat (what is underneath is water).
    pub fn on_board(&mut self, v: &BotView, inside: &Inside) {
        self.boat = !matches!(inside.class, VehicleClass::Fixed | VehicleClass::Plane | VehicleClass::Helicopter | VehicleClass::Airship) && v.ground(inside.pos[0], inside.pos[2]).is_some_and(|h| h < v.water() - 0.5);
    }

    /// On leaving a vehicle: it is no longer on a boat.
    pub fn on_leaving(&mut self) {
        self.boat = false;
        self.ramp_since = None;
    }

    /// The shore and the ramp: on the shore, whoever has the helm lowers the ramp (it is its pitch axis: on a boat without a ramp it does nothing); without it
    /// you cannot leave the hull. It gets off when the ramp is open, and leaves by the bow. Whoever does not have the helm takes it if free.
    /// `vetoed`: the vehicles it will not return to.
    /// TODO: the shore is terrain under the hull: beside a flag on an object (a platform reached from the water) nobody gets off. Today a
    /// squad's trip never takes a boat there (DC_Sea_Rigs' platforms go by air); if one does, passengers off within `GET_OFF_NEAR` of a
    /// flag taken from the water.
    pub fn arrive(&mut self, v: &BotView, inside: &Inside, seen: Option<&VehicleSeen>, axes: &mut Vec<(usize, f32)>, vetoed: &mut Vec<usize>) -> Arrival {
        let pos = inside.pos;
        // a big boat looks big: it does not disembark
        let big = seen.is_some_and(|c| c.radius > BIG_SHIP_RADIUS);
        let vel = (inside.velocity[0].powi(2) + inside.velocity[2].powi(2)).sqrt();
        let ahead = [pos[0] + inside.rows[2][0] * 12.0, pos[2] + inside.rows[2][2] * 12.0];
        let bottom = v.ground(ahead[0], ahead[1]).unwrap_or(f32::MIN).max(v.ground(pos[0], pos[2]).unwrap_or(f32::MIN));
        let shore = bottom > v.water() - if big { 8.0 } else { 1.0 };
        if !big && shore {
            let since = *self.ramp_since.get_or_insert(v.time);
            if inside.seat == 0 {
                axes.push((RAMP_AXIS, 1.0));
            }
            if vel < 2.5 && v.time - since > RAMP_WAIT {
                self.leave_by = Some(([inside.rows[2][0], inside.rows[2][2]], v.time + LEAVE_BY_BOW));
                vetoed.push(inside.vehicle);
                return Arrival::Low;
            }
        } else {
            self.ramp_since = None;
        }
        if inside.seat != 0 {
            // with nobody at the helm it goes nowhere
            if seen.is_some_and(|c| c.driver_free) {
                axes.push((crate::entry::MENU_SELECT1, 1.0));
            }
            return Arrival::Fin;
        }
        Arrival::Follows(shore)
    }

    /// At the helm and far from the shore: straight towards `centre`, and reverse if stuck.
    pub fn timon(&mut self, v: &BotView, inside: &Inside, axes: &mut Vec<(usize, f32)>, centre: [f32; 3], cam: &mut Walker) {
        let pos = inside.pos;
        let g = angle_between(inside.rows[2], [centre[0] - pos[0], centre[2] - pos[2]]);
        let t = cam.vehicle_jam(v, pos);
        if t > 6.0 {
            axes.push((axis::THROTTLE, -1.0));
            axes.push((axis::YAW, -g.signum()));
            if t > 10.0 {
                cam.anchor_t = v.time;
            }
        } else {
            axes.push((axis::YAW, (g / 20.0).clamp(-1.0, 1.0)));
            axes.push((axis::THROTTLE, 1.0));
        }
    }
}
