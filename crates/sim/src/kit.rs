//! The kit as an object: every soldier carries one
//! (`spawnPlayer` makes it and hands it over by `handlePickup`), it falls where its soldier dies
//! (`killPlayer` → the soldier's drop kit) with the weapons as they were, and a soldier on foot picks
//! one up with `c_PIDrop` (the server's player triggers check → `findKitObject` → `pickupKit`),
//! leaving its own in its place. A fallen kit goes 30 s later (the kit's timer); one a spawner made stays until it is taken. The bots never pick one up
//! (no call from the AI).

use crate::Sim;
use crate::vecmath::V3;

/// `findKitObject(position, 1.1)`: the sphere about the soldier's position the `objectManager`
/// looks in; a kit is found when its bounding sphere meets it (`getBoundingRadius`: 2.46 m for a
/// scout's kit, whose bag lies 1 m under the soldier).
pub const PICKUP_RADIUS: f32 = 1.1;
/// The time between two pickups of a player, s.
pub const PICKUP_EVERY: f64 = 2.0;
/// A fallen kit's life, s.
pub const KIT_LIFE: f64 = 30.0;
/// `c_PIDrop` (the G key).
pub const DROP: usize = 25;

impl crate::World {
    /// Where `dropKit` lays a kit fallen from a soldier at `pos` with axes `rows`: down onto the
    /// terrain, or onto the level object right under it when that is nearer in height (the
    /// `objectManager`'s ray 1000 m down, and no
    /// `PlayerControlObject`); its y axis is the normal there, z = the soldier's x × the normal,
    /// x = the normal × z (not normalised).
    pub fn kit_place(&self, pos: V3, rows: [V3; 3]) -> (V3, [V3; 3]) {
        let (h, n0) = self.level.engine_height_and_normal(pos[0], pos[2]);
        let (mut dy, mut n) = (h - pos[1], n0);
        let dir = [0.0, -1000.0, 0.0];
        for o in self.obstacles() {
            if !crate::projectile::segment_near_sphere(pos, dir, o.center, o.radius) {
                continue;
            }
            let Some(hit) = crate::obstacle::pick_lod(&o.lods, 1).and_then(|m| crate::obstacle::segment_on(m, &o.rows, o.pos, o.scale, pos, dir, false)) else { continue };
            let d = hit.point[1] - pos[1];
            if d.abs() < dy.abs() {
                (dy, n) = (d, hit.normal);
            }
        }
        let x = rows[0];
        let z = [x[1] * n[2] - x[2] * n[1], x[2] * n[0] - x[0] * n[2], x[0] * n[1] - x[1] * n[0]];
        let x = [n[1] * z[2] - n[2] * z[1], n[2] * z[0] - n[0] * z[2], n[0] * z[1] - n[1] * z[0]];
        ([pos[0], pos[1] + dy, pos[2]], [x, n, z])
    }
}

impl Sim<'_> {
    /// `spawnPlayer`'s kit: made where soldier `i` is and handed to it (`handlePickup`; the
    /// weapons it gives are `give_kit`'s).
    /// A soldier made from outside (the original game's, `bytefield-client`'s `orig`) takes a kit's weapons and parts
    /// as at a spawn, without the kit object.
    pub fn arm(&mut self, i: usize, kit: &str) -> bool {
        let ws = self.world.kit_weapons(kit);
        let armed = self.vehicles[i].give_kit(ws, None);
        if let Some(m) = self.vehicles[i].soldier_mut() {
            m.kit_parts = self.world.kit_parts(kit);
        }
        armed
    }

    pub(crate) fn hand_kit(&mut self, i: usize, kit: &str) {
        let (pos, rows) = (self.vehicles[i].craft.body.pos, self.vehicles[i].craft.body.rows);
        let Some(crate::create::Made::Item(k)) = self.instantiate(kit, pos, rows) else { return };
        self.items[k].carrier = Some(i);
        if let Some(m) = self.vehicles[i].soldier_mut() {
            m.kit = Some(k);
        }
    }

    /// The soldier's drop kit: soldier `i`'s kit falls ([`crate::World::kit_place`]) with its
    /// weapons as they are, and goes in 30 s. `keep`: the soldier keeps a copy of them (a dead
    /// one, whose hands the client still draws).
    pub(crate) fn drop_kit(&mut self, i: usize, keep: bool) {
        let v = &mut self.vehicles[i];
        let (pos, rows) = (v.craft.body.pos, v.craft.body.rows);
        let Some(m) = v.soldier_mut() else { return };
        // (a copy that follows the server, `Prediction`: it knows the items on the ground, not the carried ones)
        let Some(k) = m.kit.take().filter(|&k| k < self.items.len()) else { return };
        let weapons = if keep { m.weapons_in_kit_order() } else { m.take_weapons() };
        let parts = std::mem::take(&mut m.kit_parts);
        let (at, axes) = self.world.kit_place(pos, rows);
        let expires = self.clock.steps + (KIT_LIFE / self.clock.dt).round() as u64;
        let item = &mut self.items[k];
        item.carrier = None;
        item.weapons = weapons;
        item.parts = parts;
        item.pos = at;
        item.rows = axes;
        item.expires = Some(expires);
    }

    /// `checkPlayerTriggers` with `c_PIDrop`: a player's soldier on foot, not firing nor
    /// recoiling, 2 s after its last pickup, takes the nearest kit whose bounding sphere meets 1.1
    /// m about it (any team's; a carried one is not on the ground), dropping its own first
    /// (`pickupKit`). A kit never carried
    /// (a spawner's) brings its template's weapons; else the ones it lies with, as they were.
    pub(crate) fn pickup_step(&mut self, i: usize) {
        let now = self.clock.time;
        let v = &self.vehicles[i];
        let Some(m) = v.soldier().filter(|m| m.has_player && m.driven && !v.removed && v.entry.inside.is_none()) else { return };
        if !(m.input.mapped(DROP) && m.input.get(DROP) > crate::entry::PRESSED) {
            return;
        }
        if m.recoil.0 != 0 || m.weapon.as_ref().is_some_and(|w| !w.not_firing()) || now - m.kit_picked_at <= PICKUP_EVERY {
            return;
        }
        let at = v.craft.body.pos;
        let near = |p: V3| {
            let d = [p[0] - at[0], p[1] - at[1], p[2] - at[2]];
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
        };
        let Some(k) = (0..self.items.len())
            .filter(|&k| {
                let t = &self.items[k];
                let r = PICKUP_RADIUS + self.world.bounding_radius(&t.template);
                !t.removed && t.carrier.is_none() && t.kind == "kit" && near(t.pos) < r * r
            })
            .min_by(|&a, &b| near(self.items[a].pos).total_cmp(&near(self.items[b].pos)))
        else {
            return;
        };
        self.drop_kit(i, false);
        let weapons = if self.items[k].weapons.is_empty() {
            self.world.kit_weapons(&self.items[k].template).into_iter().map(|w| crate::firearms::FireArms::new(std::sync::Arc::new(w))).collect()
        } else {
            std::mem::take(&mut self.items[k].weapons)
        };
        let parts = if self.items[k].weapons.is_empty() && self.items[k].parts.is_empty() {
            self.world.kit_parts(&self.items[k].template)
        } else {
            std::mem::take(&mut self.items[k].parts)
        };
        let item = &mut self.items[k];
        item.carrier = Some(i);
        item.expires = None;
        let m = self.vehicles[i].soldier_mut().unwrap();
        m.kit = Some(k);
        m.kit_picked_at = (now as f32) as f64;
        m.hold_weapons(weapons);
        m.kit_parts = parts;
        self.journal.kit_log.push((self.clock.steps, i, k));
    }

    /// Every frame: a carried kit is where its soldier is; a fallen one goes when its time is up
    /// (The kit's message handling → destroy).
    pub(crate) fn update_items(&mut self) {
        for t in self.items.iter_mut().filter(|t| !t.removed) {
            if let Some(i) = t.carrier {
                let b = &self.vehicles[i].craft.body;
                (t.pos, t.rows) = (b.pos, b.rows);
            } else if t.expires.is_some_and(|e| self.clock.steps >= e) {
                t.removed = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::soldier::axis;

    /// a reference run of the original: an assault soldier of El_Alamein with
    /// the scout kit it had at its feet fires, picks the scout kit up (its own falls there), picks
    /// its own back with the rounds it fired gone, cannot pick again within 2 s; the scout kit,
    /// dropped, goes 900 frames later, the carried one stays.
    /// The monkey in GC_BonusRace, seed 2: a kit that this copy of the world does not have.
    #[test]
    fn dropping_a_kit_this_copy_does_not_have() {
        let Some(game) = bytefield_testdata::base() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let y = w.height_at(1672.3, 851.7).unwrap() as f32;
        let sol = s.spawn_vehicle("GermanDesertSoldier", [1672.3, y + 1.0, 851.7], [0.0; 3]).unwrap();
        s.vehicles[sol].soldier_mut().unwrap().kit = Some(s.items.len() + 50);
        s.drop_kit(sol, false);
        assert_eq!(s.vehicles[sol].soldier().unwrap().kit, None);
    }

    /// a reference run of the original: a US soldier of DC_Basrahs_Edge with an
    /// `Iraq_Assault` and the `US_Sniper` at its feet: the other side's kit is taken like any other,
    /// the rounds travel with the kit (AK47 `[20, 30, …]`) and the GP30 stays the kit's last weapon.
    #[test]
    fn dc_swap_across_teams_and_back() {
        let Some(game) = bytefield_testdata::with_mod("DC_Final") else { return; };
        let w = crate::World::load(&game, "DC_Final", "DC_Basrahs_Edge", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        let (x, z) = (721.85, 719.78);
        let y = w.height_at(x, z).unwrap() as f32;
        let sol = s.spawn_vehicle("USSoldier", [x as f32, y + 1.0, z as f32], [0.0; 3]).unwrap();
        s.vehicles[sol].soldier_mut().unwrap().has_player = true;
        assert!(s.vehicles[sol].give_kit(w.kit_weapons("US_Sniper"), None));
        s.hand_kit(sol, "US_Sniper");
        s.drop_kit(sol, false);
        assert!(s.vehicles[sol].give_kit(w.kit_weapons("Iraq_Assault"), None));
        s.hand_kit(sol, "Iraq_Assault");
        let (sniper, assault) = (s.items.len() - 2, s.items.len() - 1);
        let names = |s: &crate::Sim| -> Vec<String> { s.vehicles[sol].soldier().unwrap().weapons_in_kit_order().iter().map(|w| w.template.name.clone()).collect() };
        let first = |s: &crate::Sim| s.vehicles[sol].soldier().unwrap().weapons_in_kit_order()[0].mags.clone();
        assert_eq!(names(&s), ["AK47", "Browninghipo", "KnifeAxis", "GrenadeAllies", "Binoculars", "AK47GP30"]);
        for f in 0..300 {
            let mut axes = vec![];
            if (10..40).contains(&f) || (80..95).contains(&f) {
                axes.push((axis::FIRE, 1.0));
            }
            if [60, 61, 150, 151, 200, 201].contains(&f) {
                axes.push((super::DROP, 1.0));
            }
            s.vehicles[sol].soldier_mut().unwrap().set_input(&axes);
            s.step();
            match f {
                59 => assert_eq!(first(&s)[..2], [20, 30], "the original's ten rounds"),
                61 => assert_eq!(names(&s), ["M25Sniper", "M9_beretta", "KnifeAllies", "Binoculars", "GrenadeAllies"]),
                151 | 299 => {
                    assert_eq!(names(&s)[0], "AK47", "frame {f}");
                    assert_eq!(first(&s)[..2], [20, 30], "frame {f}");
                }
                _ => {}
            }
        }
        // dropped at 150, the original destroys it at 1050
        for f in 300..1060 {
            s.vehicles[sol].soldier_mut().unwrap().set_input(&[]);
            s.step();
            if f == 1040 {
                assert!(!s.items[sniper].removed);
            }
        }
        assert!(s.items[sniper].removed);
        assert!(!s.items[assault].removed);
    }

    #[test]
    fn el_alamein_swap_and_back() {
        let Some(game) = bytefield_testdata::base() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        let y = w.height_at(1672.3, 851.7).unwrap() as f32;
        let sol = s.spawn_vehicle("GermanDesertSoldier", [1672.3, y + 1.0, 851.7], [0.0; 3]).unwrap();
        s.vehicles[sol].soldier_mut().unwrap().has_player = true;
        assert!(s.vehicles[sol].give_kit(w.kit_weapons("German_Scout_Desert"), None));
        s.hand_kit(sol, "German_Scout_Desert");
        s.drop_kit(sol, false);
        assert!(s.vehicles[sol].give_kit(w.kit_weapons("German_Assault_Desert"), None));
        s.hand_kit(sol, "German_Assault_Desert");
        let (scout, assault) = (s.items.len() - 2, s.items.len() - 1);
        let names = |s: &crate::Sim| -> (Vec<String>, String) {
            let m = s.vehicles[sol].soldier().unwrap();
            (m.weapons_in_kit_order().iter().map(|w| w.template.name.clone()).collect(), m.weapon.as_ref().unwrap().template.name.clone())
        };
        let mags = |s: &crate::Sim| s.vehicles[sol].soldier().unwrap().weapons_in_kit_order()[0].mags.clone();
        let mut frame = |s: &mut crate::Sim, axes: &[(usize, f32)]| {
            s.vehicles[sol].soldier_mut().unwrap().set_input(axes);
            s.step();
        };
        for f in 0..60 {
            frame(&mut s, if (10..40).contains(&f) { &[(axis::FIRE, 1.0)] } else { &[] });
        }
        let fired = mags(&s);
        assert!(fired[0] < 30 && fired[1..].iter().all(|&n| n == 30), "{fired:?}");
        let g = |s: &mut crate::Sim, n: usize, frame: &mut dyn FnMut(&mut crate::Sim, &[(usize, f32)])| {
            for f in 0..n {
                frame(s, if f < 2 { &[(super::DROP, 1.0)] } else { &[] });
            }
        };
        g(&mut s, 90, &mut frame);
        assert_eq!(names(&s), (["K98Sniper", "WalterP38", "KnifeAxis", "Binoculars", "GrenadeAxis"].map(String::from).to_vec(), "K98Sniper".to_string()));
        assert_eq!((s.items[scout].carrier, s.items[assault].carrier), (Some(sol), None));
        let back = s.clock.steps;
        g(&mut s, 50, &mut frame);
        assert_eq!(names(&s).1, "Sg44");
        assert_eq!(mags(&s), fired);
        // within 2 s: nothing
        g(&mut s, 10, &mut frame);
        assert_eq!(s.items[assault].carrier, Some(sol));
        assert_eq!(s.journal.kit_log.len(), 2);
        while s.clock.steps < back + 900 + 5 {
            frame(&mut s, &[]);
            assert_eq!(s.items[scout].removed, s.clock.steps > back + 900, "step {}", s.clock.steps);
        }
        assert!(!s.items[assault].removed);
    }

    /// A kit on the ground goes to a copy of the world with the snapshot (`Modes.items`); one
    /// carried, or gone, is not there.
    #[test]
    fn el_alamein_the_fallen_kit_replicates() {
        let Some(game) = bytefield_testdata::base() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let y = w.height_at(1672.3, 851.7).unwrap() as f32;
        let sol = s.spawn_vehicle("GermanDesertSoldier", [1672.3, y + 1.0, 851.7], [0.0; 3]).unwrap();
        assert!(s.vehicles[sol].give_kit(w.kit_weapons("German_Scout_Desert"), None));
        s.hand_kit(sol, "German_Scout_Desert");
        let kit = s.items.len() - 1;
        let mut copy = crate::Sim::new(&w, 1.0 / 30.0);
        let shown = |c: &crate::Sim| c.items.iter().enumerate().filter(|(_, t)| !t.removed && t.carrier.is_none()).map(|(k, t)| (k, t.template.clone())).collect::<Vec<_>>();
        crate::replica::Snapshot::of(&s).write(&mut copy);
        assert_eq!(shown(&copy), []);
        s.drop_kit(sol, true);
        let snap = crate::replica::Snapshot::of(&s);
        let d = snap.delta(None);
        crate::replica::Snapshot::apply(None, &d).write(&mut copy);
        assert_eq!(shown(&copy), [(kit, "German_Scout_Desert".to_string())]);
        assert!((0..3).all(|i| (copy.items[kit].pos[i] - s.items[kit].pos[i]).abs() < 0.01));
        assert_eq!(copy.items[kit].kind, "kit");
        s.items[kit].removed = true;
        crate::replica::Snapshot::of(&s).write(&mut copy);
        assert_eq!(shown(&copy), []);
    }

    /// a reference run of the original: a scout's kit on the ground is taken by a soldier 1 m over
    /// the ground 1.99 m from it (1.72 m along the ground), not 2.46 m (2.25 m).
    #[test]
    fn el_alamein_reach() {
        let Some(game) = bytefield_testdata::base() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "coop").unwrap();
        for (along, taken) in [(1.72, true), (2.25, false)] {
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            let y = w.height_at(1672.3, 851.7).unwrap() as f32;
            let sol = s.spawn_vehicle("GermanDesertSoldier", [1672.3, y + 1.0, 851.7], [0.0; 3]).unwrap();
            s.vehicles[sol].soldier_mut().unwrap().has_player = true;
            assert!(s.vehicles[sol].give_kit(w.kit_weapons("German_Scout_Desert"), None));
            s.hand_kit(sol, "German_Scout_Desert");
            s.drop_kit(sol, false);
            let kit = s.items.len() - 1;
            // (on what lies there, as in the original: 60.943; the soldier stood at 61.943)
            let ky = s.items[kit].pos[1];
            assert!((ky - 60.943).abs() < 0.01, "{ky}");
            s.vehicles[sol].craft.body.pos = [1672.3 + along, ky + 1.0, 851.7];
            s.vehicles[sol].soldier_mut().unwrap().set_input(&[(super::DROP, 1.0)]);
            s.pickup_step(sol);
            assert_eq!(s.items[kit].carrier.is_some(), taken, "{along} m");
        }
    }
}
