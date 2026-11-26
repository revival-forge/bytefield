//! The kit's active parts: `ActiveKitPart`, XPack2's
//! rocket pack (`GermanElite_RocketPack`), GC's sprint (`Sprint`, `Sprint2`) and the `nochute` of GC
//! and DC. Every frame the soldier's player input handling updates each part (the kit part's update:
//! its state, its heat) and adds its acceleration (`getAcceleration`) in the soldier's axes. A
//! part with `OverrideAirMovementInhibitations` keeps the soldier out of the free fall (so no
//! parachute) and its least `Damping` softens its falls (`getDamageDampingFromActiveKitParts`).

use std::sync::Arc;

use crate::anim::Input;

use crate::vecmath::V3;

/// `c_PINone`: no input (the template's default trigger and activater).
pub const NO_INPUT: usize = 0x37;
/// An input is pressed over 0.5 (`_DAT_086b05e8`).
const PRESSED: f32 = 0.5;
/// `_DAT_086d1a2c`: the burst's countdown goes down by this every update, whatever the frame's
/// length (a 30 Hz burst, 1/30, is one frame over it: the pack fires every other frame).
const BURST_STEP: f32 = 0.0333;
/// `hasRoomForJump`: the ray from the soldier's position straight up, m.
pub const ROOM: f32 = 5.0;
/// The template's per-second rates are kept per frame (`_DAT_086b01b4`).
const FPS: f32 = 30.0;

/// `ActiveKitPartState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum State {
    /// the trigger fires it: the active acceleration
    Active = 0,
    /// between bursts: the passive one, while there was room
    Passive = 1,
    /// off: overheated, or a pose its masks rule out
    Off = 3,
    /// held off by its activater
    Held = 4,
}

/// `ActiveKitPartTemplate`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct KitPartTemplate {
    pub name: String,
    /// the acceleration when active and passive
    pub active_acc: V3,
    pub passive_acc: V3,
    /// `c_PI*` ids
    pub trigger: usize,
    pub activater: usize,
    /// `c_Asm*` state flags (and 0x80) all needed, none allowed
    pub positive_mask: u32,
    pub negative_mask: u32,
    /// per frame (the con gives them per second)
    pub active_heat: f32,
    pub passive_heat: f32,
    pub cooling: f32,
    /// 1 / `burstFrequency`, s (0 without it)
    pub burst_period: f32,
    /// the damping (1 without it)
    pub damping: f32,
    /// whether it overrides the air control
    pub override_air: bool,
}

/// `ActiveKitPart`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct KitPart {
    #[serde(skip)]
    pub t: Arc<KitPartTemplate>,
    /// its state
    pub state: State,
    /// 0..1, off at 1
    pub heat: f32,
    /// until the next burst, s
    pub burst: f32,
    /// the last burst found room above (`hasRoomForJump`); true at first
    pub room: bool,
}

impl KitPart {
    pub fn new(t: Arc<KitPartTemplate>) -> Self {
        let state = if t.activater != NO_INPUT { State::Held } else { State::Off };
        KitPart {
            t,
            state,
            heat: 0.0,
            burst: -1.0,
            room: true,
        }
    }

    /// The kit part's update: `flags` the soldier's animation state flags
    /// (with 0x80); `room` asks `hasRoomForJump` (only when the trigger would fire it).
    pub fn update(&mut self, input: &Input, flags: u32, room: &mut dyn FnMut() -> bool) {
        let t = self.t.clone();
        let pressed = |i: usize| i < NO_INPUT && input.mapped(i) && input.get(i) > PRESSED;
        if self.state == State::Held {
            if pressed(t.activater) {
                self.state = State::Off;
            }
        } else {
            if flags & t.negative_mask != 0 || flags & t.positive_mask != t.positive_mask || self.heat >= 1.0 {
                self.state = State::Off;
            } else if pressed(t.trigger) && self.burst <= 0.0 && {
                self.room = room();
                self.room
            } {
                self.state = State::Active;
                self.heat += t.active_heat;
                self.burst = t.burst_period;
            } else {
                self.state = State::Passive;
                self.heat += t.passive_heat;
            }
            if pressed(t.activater) {
                self.state = State::Held;
            }
        }
        self.burst -= BURST_STEP;
        self.heat -= t.cooling;
        if self.heat < 0.0 {
            self.heat = 0.0;
        }
    }

    /// `getAcceleration`: in the soldier's axes.
    pub fn acceleration(&self) -> V3 {
        match self.state {
            State::Active => self.t.active_acc,
            State::Passive if self.room => self.t.passive_acc,
            _ => [0.0; 3],
        }
    }
}

impl crate::World {
    /// An `ActiveKitPart` template by name (None for any other kind).
    pub fn kit_part_template(&self, name: &str) -> Option<KitPartTemplate> {
        let t = self.db.object_template(name)?;
        if !t.kind.eq_ignore_ascii_case("ActiveKitPart") {
            return None;
        }
        let first = |n: &str| t.get(n).and_then(|a| a.first().cloned());
        let num = |n: &str, d: f32| first(n).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
        let vec = |n: &str| {
            first(n)
                .and_then(|v| bytefield_world::parse_vec(&v))
                .filter(|v| v.len() == 3)
                .map_or([0.0; 3], |v| [v[0] as f32, v[1] as f32, v[2] as f32])
        };
        let input = |n: &str| first(n).and_then(|v| crate::input::player_input(&v)).map_or(NO_INPUT, |v| v as usize);
        // The soldier template's asm flags by name: crouched, the original's pack does nothing,
        // so the bare words the data write are the flags
        let mask = |n: &str| t.all(n).iter().filter_map(|a| a.first()).map(|v| asm_flag(v)).fold(0, |a, b| a | b);
        let freq = num("burstfrequency", 0.0);
        Some(KitPartTemplate {
            name: t.name.clone(),
            active_acc: vec("setactiveacceleration"),
            passive_acc: vec("setpassiveacceleration"),
            trigger: input("settrigger"),
            activater: input("setactivater"),
            positive_mask: mask("addtopossitivemask"),
            negative_mask: mask("addtonegativemask"),
            active_heat: num("activeheatincrement", 0.0) / FPS,
            passive_heat: num("passiveheatincrement", 0.0) / FPS,
            cooling: num("coolingfactor", 0.0) / FPS,
            burst_period: if freq != 0.0 { 1.0 / freq } else { 0.0 },
            damping: num("damping", 1.0),
            override_air: num("overrideairmovementinhibitations", 0.0) != 0.0,
        })
    }

    /// The active parts of a `Kit` (its `addTemplate`s that are `ActiveKitPart`s), as
    /// `addKitPart` gives them to the soldier.
    pub fn kit_parts(&self, kit: &str) -> Vec<KitPart> {
        let Some(t) = self.db.object_template(kit) else { return Vec::new() };
        t.all("addtemplate").into_iter().filter_map(|a| self.kit_part_template(a.first()?)).map(|t| KitPart::new(Arc::new(t))).collect()
    }
}

/// A `c_Asm*` flag by name, or by the bare word of `addToNegativeMask` (`Climbing`, `Crouching`,
/// `Swiming`, `Lying`); 0 for any other word.
fn asm_flag(v: &str) -> u32 {
    match v.to_ascii_lowercase().as_str() {
        "climbing" => crate::anim::CLIMBING,
        "crouching" => crate::anim::CROUCHING,
        "swiming" | "swimming" => crate::anim::SWIMMING,
        "lying" => crate::anim::LYING,
        "c_asmhideweapon" => crate::anim::HIDE_WEAPON,
        "c_asmlockfreelook" => crate::anim::LOCK_FREE_LOOK,
        "c_asmisswimming" => crate::anim::SWIMMING,
        "c_asmisclimbing" => crate::anim::CLIMBING,
        "c_asmiscrouching" => crate::anim::CROUCHING,
        "c_asmislying" => crate::anim::LYING,
        n => n.parse().unwrap_or(0),
    }
}

impl crate::Sim<'_> {
    /// `hasRoomForJump(position, soldier)`: nothing of the level's objects 5 m straight over the
    /// soldier's position (the `objectManager`'s ray, not the
    /// soldier itself; not the terrain).
    pub(crate) fn room_for_jump(world: &crate::World, pos: V3) -> bool {
        let dir = [0.0, ROOM, 0.0];
        !world.obstacles().iter().any(|o| {
            crate::projectile::segment_near_sphere(pos, dir, o.center, o.radius)
                && crate::obstacle::pick_lod(&o.lods, 1)
                    .and_then(|m| crate::obstacle::segment_on(m, &o.rows, o.pos, o.scale, pos, dir, true))
                    .is_some()
        })
    }

    /// Before the soldiers' input: whether each one with an active part has room over it.
    pub(crate) fn kit_rooms(&mut self) {
        for v in self.vehicles.iter_mut().filter(|v| v.in_world()) {
            let pos = v.craft.body.pos;
            if let Some(m) = v.soldier_mut().filter(|m| !m.kit_parts.is_empty()) {
                m.kit_room = Self::room_for_jump(self.world, pos);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> KitPart {
        KitPart::new(Arc::new(KitPartTemplate {
            name: "GermanElite_RocketPack".into(),
            active_acc: [0.0, 72.0, 0.0],
            passive_acc: [0.0, 7.7, 0.0],
            trigger: 9,
            activater: NO_INPUT,
            positive_mask: 0,
            negative_mask: 0,
            active_heat: 2.25 / FPS,
            passive_heat: 0.0,
            cooling: 0.07 / FPS,
            burst_period: 1.0 / 30.0,
            damping: 0.0,
            override_air: true,
        }))
    }

    /// Held, the pack fires every other frame (the countdown 1/30 less 0.0333 is still over 0) and
    /// overheats after 15 bursts; then it fires once each time it cools under 1.
    #[test]
    fn rocket_pack_bursts_every_other_frame_and_overheats() {
        let mut p = pack();
        let mut input = Input::default();
        input.mask |= 1 << 9;
        input.set(9, 1.0);
        let mut states = Vec::new();
        for _ in 0..60 {
            p.update(&input, 0, &mut || true);
            states.push(p.state);
        }
        assert_eq!(&states[..4], &[State::Active, State::Passive, State::Active, State::Passive]);
        let bursts = states.iter().take_while(|s| **s != State::Off).filter(|s| **s == State::Active).count();
        assert_eq!(bursts, 15);
        assert_eq!(p.acceleration(), [0.0; 3]);
        // released, it is passive again (7.7 up) once cool enough
        input.set(9, 0.0);
        for _ in 0..30 {
            p.update(&input, 0, &mut || true);
        }
        assert_eq!(p.state, State::Passive);
        assert_eq!(p.acceleration(), [0.0, 7.7, 0.0]);
    }
}


#[cfg(test)]
mod nochute {
    /// DC's `nochute`: `DC_LostVillage_nopara` runs the level's own kits, which carry it;
    /// `DC_LostVillage` has every one of those lines as `remrun` (not run): the global kits, so a
    /// soldier that falls does go into the free fall. Bench only, by the GC test's rule. Skipped without DC.
    #[test]
    fn dc_lost_village_nochute() {
        let Some(game) = bytefield_testdata::with_mod("DC_Final") else { return; };
        for (level, chute) in [("DC_LostVillage", true), ("DC_LostVillage_nopara", false)] {
            let w = crate::World::load(&game, "DC_Final", level, "conquest").unwrap();
            for kit in ["Us_Assault", "Iraq_Assault"] {
                let no = w.kit_parts(kit).iter().any(|p| p.t.name.eq_ignore_ascii_case("nochute") && p.t.override_air);
                assert_eq!(no, !chute, "{level} {kit}");
            }
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            let (x, z) = (1000.0, 1000.0);
            let h = w.level.engine_height_and_normal(x, z).0;
            let sol = s.spawn_vehicle("USSoldier", [x, h + 120.0, z], [0.0; 3]).unwrap();
            let m = s.vehicles[sol].soldier_mut().unwrap();
            m.has_player = true;
            m.kit_parts = w.kit_parts("Us_Assault");
            let mut free = false;
            for _ in 0..300 {
                s.vehicles[sol].soldier_mut().unwrap().set_input(&[]);
                s.step();
                free |= s.vehicles[sol].soldier().unwrap().in_state(&["Lb_ParachuteFall"]);
            }
            assert_eq!(free, chute, "{level}");
        }
    }

    /// GC's `NoChute` (`OverrideAirMovementInhibitations 1`, no acceleration): a soldier falling from
    /// 120 m never goes into `Lb_ParachuteFall` (so `c_PIAction` cannot open a parachute: message 18
    /// wants that state), and still lands. Without it, it does. Skipped without GC.
    #[test]
    fn gc_nochute_keeps_out_of_the_free_fall() {
        let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
        let w = crate::World::load(&game, "GCMOD", "GC_Alaris_Prime", "conquest").unwrap();
        let parts = w.kit_parts("Rebel_Trooper_No_Nades");
        assert_eq!(parts.iter().map(|p| (p.t.name.as_str(), p.t.override_air)).collect::<Vec<_>>(), [("NoChute", true)]);
        assert_eq!(parts[0].acceleration(), [0.0; 3]);
        let fell = |with: bool| {
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            let (x, z) = (1000.0, 1000.0);
            let h = w.level.engine_height_and_normal(x, z).0;
            let sol = s.spawn_vehicle("EndorSoldier", [x, h + 120.0, z], [0.0; 3]).unwrap();
            let m = s.vehicles[sol].soldier_mut().unwrap();
            m.has_player = true;
            if with {
                m.kit_parts = w.kit_parts("Rebel_Trooper_No_Nades");
            }
            let mut free = false;
            for _ in 0..300 {
                s.vehicles[sol].soldier_mut().unwrap().set_input(&[]);
                s.step();
                free |= s.vehicles[sol].soldier().unwrap().in_state(&["Lb_ParachuteFall"]);
            }
            (free, s.vehicles[sol].craft.body.pos[1] - h)
        };
        let (free, _) = fell(false);
        assert!(free);
        let (free, over) = fell(true);
        assert!(!free);
        assert!(over < 2.0, "{over}");
    }
}
