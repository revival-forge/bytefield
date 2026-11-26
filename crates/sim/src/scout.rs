//! The scout's mark and the artillery's camera. Read from the original game: the weapon's fire
//! with `magType` 2 (the binoculars) calls the weapon's place scout camera instead of firing;
//! The game's place scout camera, `removeScoutCamera` and the list; the projectile's destroy and
//! `resetProjectile` take a projectile with `damageType` 3 out of the list.
//!
//! `placeScoutCamera`, with a player, on the server: it sets off the projectile of the weapon's last mark, makes
//! the new one at the fire matrix and casts a ray 10 km along it against the objects and the terrain. If it meets
//! something nearer, the projectile is moved 30 m over where it was fired from, looking at the point; it keeps the
//! point, the player and the team, and goes on the game's list. Without a hit the projectile stays where it was
//! made, off the list, for its time to live. The list loses it when the projectile ends (`BinocularsProjectile`:
//! `damageType 3`, 120 s, or the next mark of the same binoculars).

use crate::projectile::{End, Projectile, ScoutMark};

use crate::vecmath::V3;

/// How far the binoculars' ray reaches (`_DAT_086c04a8`).
pub const MARK_REACH: f32 = 10000.0;
/// How far over the scout's fire point the camera goes (`_DAT_086b01b4`, added to y).
pub const CAMERA_HEIGHT: f32 = 30.0;

/// `calcLookAtMatrix(eye, at, up)`: the rows (x, y, z) of a matrix at `eye` with z towards `at`, y up.
pub fn look_at(eye: V3, at: V3, up: V3) -> [V3; 3] {
    let norm = |v: V3| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.0 { v.map(|c| c / l) } else { v }
    };
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let z = norm([at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]]);
    let x = norm(cross(up, z));
    let y = cross(z, x);
    [x, y, z]
}

impl crate::Sim<'_> {
    /// The weapon's place scout camera for every soldier whose binoculars fired this frame.
    pub(crate) fn place_scout_marks(&mut self) {
        for i in 0..self.vehicles.len() {
            let Some(m) = self.vehicles[i].soldier_mut() else { continue };
            let Some(shot) = m.mark.take() else { continue };
            // (`Fire` with no player returns before it)
            if !m.has_player {
                continue;
            }
            let Some(w) = m.weapon.as_mut() else { continue };
            let template = w.template.projectile.clone();
            if let Some(k) = w.mark.take()
                && self.projectiles.get(k).is_some_and(|p| p.alive())
            {
                self.projectiles[k].end = Some(End::Detonated);
                let p = self.projectiles[k].clone();
                self.projectile_detonated(&p);
            }
            let Some(k) = self.add_projectile(&template, &shot) else { continue };
            if let Some(w) = self.vehicles[i].soldier_mut().and_then(|m| m.weapon.as_mut()) {
                w.mark = Some(k);
            }
            // its own velocity is never set: `createProjectile` and the matrix only
            self.projectiles[k].vel = [0.0; 3];
            self.projectiles[k].owner = Some(i);
            let z = shot.rows[2];
            let dir = z.map(|c| c * MARK_REACH);
            // TODO: the server's object ray takes `ObjectFlagPredicator(0x200)`; here, what a bullet would meet,
            // the scout's own soldier left out
            let Some(hit) = Projectile::sight(&self.world.level, self.obstacles(), &self.vehicles, shot.pos, dir, Some(i)) else { continue };
            let d = [hit[0] - shot.pos[0], hit[1] - shot.pos[1], hit[2] - shot.pos[2]];
            if (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() >= MARK_REACH {
                continue;
            }
            let eye = [shot.pos[0], shot.pos[1] + CAMERA_HEIGHT, shot.pos[2]];
            let p = &mut self.projectiles[k];
            p.pos = eye;
            p.rows = look_at(eye, hit, [0.0, 1.0, 0.0]);
            p.scout = Some(ScoutMark { target: hit, player: i, team: self.vehicles[i].team });
            // the scout's radio, «Artillery needed» to its team (level 3: seen in the original's chat on marking;
            // the original game sends nothing there, its client does)
            if let Some(k) = self.players.iter().position(|q| q.soldier == Some(i)) {
                self.say(k, crate::radio::ARTILLERY_SUPPORT, None);
            }
        }
    }

    /// The game's scout camera list lookup: the projectiles that are scout cameras now, oldest first (`placeScoutCamera`
    /// appends; the end of a projectile with `damageType` 3 takes it out).
    pub fn scout_cameras(&self) -> impl Iterator<Item = (usize, &Projectile)> {
        self.projectiles.iter().enumerate().filter(|(_, p)| p.alive() && p.scout.is_some())
    }

    /// Whether the soldier `i` sits where it may look from the marks: its seat's PlayerControlObject has `artPos`
    /// (The server's toggle scout camera checks it before moving the camera; `artPos` 0, nothing happens).
    pub fn art_seat(&self, i: usize) -> bool {
        let e = &self.vehicles[i].entry;
        e.inside.and_then(|k| self.vehicles[k].entry.seats.get(e.position)).is_some_and(|s| s.art)
    }

    /// The marks a player of `team` may look from, oldest first.
    // TODO: whether the original's client keeps the other team's marks out of its list (the server's toggle does
    // not look at the team; the projectile keeps it). Level 3: only one's own, as the guides tell it.
    pub fn team_scout_cameras(&self, team: u32) -> Vec<usize> {
        self.scout_cameras().filter(|(_, p)| p.scout.as_ref().is_some_and(|m| m.team == team)).map(|(k, _)| k).collect()
    }
}
