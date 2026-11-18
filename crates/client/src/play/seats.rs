//! Seats and views: the scouts' artillery camera, seat eye points, and the chase (third-person) view.

use super::*;

/// The artillery's view from the scouts' marks (level 2): in a seat with `artPos`,
/// the right button (`c_PIAltFire`) goes to the team's first mark, then to the next, and after the last back to
/// the seat's own view; the camera is the mark's projectile (30 m over the scout, looking at the point). The mark
/// gone (`checkIfLocalPlayersArtCameraIsRemoved`) or the seat left, back to the seat's view. The gun stays the
/// player's: the mouse and the keys still aim and fire it.
// (In the original, with one mark: the presses go mark, seat, mark, seat; the mouse turns the gun,
// not the view.) TODO: with two scouts, whether it starts from the newest mark; the view's HUD, as the original's:
// the binoculars' `setScopeIcon` (binocular.tga) and `setSightIcon` (scout_ring_128x128.tga), the seat's `DirBar*`
// bars, «<scout>: <seconds left>».
pub(super) fn scout_toggle(g: &mut Game, pressed: bool) {
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let Some(i) = p.soldier.filter(|&i| p.alive && sim.art_seat(i)) else {
        g.scout_view = None;
        return;
    };
    let marks = sim.team_scout_cameras(sim.vehicles[i].team);
    let now = g.scout_view.and_then(|k| marks.iter().position(|&m| m == k));
    g.scout_view = match (pressed, now) {
        (false, Some(_)) => g.scout_view,
        (false, None) => None,
        (true, None) => marks.first().copied(),
        (true, Some(n)) => marks.get(n + 1).copied(),
    };
    if pressed {
        info!("scout view: {:?} of the marks {marks:?}", g.scout_view);
    }
}

/// The `outsideHudOffset` of the seat's `Camera`, if it isn't zero: what gives a seat the nose camera (planes, mostly).
pub(super) fn nose_offset(g: &Game, v: &bytefield_sim::mover::Vehicle, seat: usize) -> Option<[f32; 3]> {
    let c = v.craft.parts.iter().find(|q| matches!(q.kind, Kind::Camera) && q.seat == seat)?;
    let a = g.session.sim.world.db.object_template(&c.name)?.get("outsidehudoffset")?.first()?.clone();
    let mut n = a.split('/').map(|x| x.trim().parse::<f32>().unwrap_or(0.0));
    let o = [n.next()?, n.next()?, n.next()?];
    (o != [0.0; 3]).then_some(o)
}

/// The seat's `Camera` part (the body without one): its axes and position.
pub(crate) fn seat_camera(v: &bytefield_sim::mover::Vehicle, seat: usize) -> ([[f32; 3]; 3], [f32; 3]) {
    let frames = v.craft.frames();
    match v.craft.parts.iter().position(|q| matches!(q.kind, Kind::Camera) && q.seat == seat) {
        Some(c) => (frames[c].child_rows, frames[c].pos),
        None => (v.craft.body.rows, v.craft.body.pos),
    }
}

/// The outside views a soldier's `Camera` allows on foot (chase, front chase, fly-by: `CVMChase`, `CVMFrontChase`, `CVMFlyBy`, the
/// camera among its `addTemplate`s). bf1942's `SoldierCamera` has them off, GC's on.
pub(super) fn foot_views(sim: &bytefield_sim::Sim, soldier: &str) -> [bool; 3] {
    let db = &sim.world.db;
    let Some(t) = db.object_template(soldier) else { return [false; 3] };
    let camera = t.all("addTemplate").iter().filter_map(|a| db.object_template(a.first()?)).find(|c| c.kind.eq_ignore_ascii_case("Camera"));
    let on = |name: &str| camera.and_then(|c| c.get(name)).and_then(|v| v.first()).is_some_and(|v| v.trim() == "1");
    [on("CVMChase"), on("CVMFrontChase"), on("CVMFlyBy")]
}

/// The outside views in a vehicle, a tick on, as the original's camera places them (level 1 for chase and
/// front chase): 1.2 × the vehicle's bounding radius behind (in front), 0.3 × that up along the body's axes, back
/// by 0.6 s of its velocity (ahead, from the front); it goes there by 1 − e^(−2 t), at least 1 m over the
/// terrain, looking at the seat's camera with the world's up. It starts from where the camera was (from inside,
/// out of the seat). The fly-by (level 3): a point 3.2 × as far ahead (and 0.3 s of the velocity), at least 1 m
/// up, where it stays looking at the vehicle until this one has passed it by as far again.
// TODO: the fly-by's frames are not done:
// when the original moves its point is a guess. The `CVM*` flags of each `Camera` template (all on in the data but
// the traces, used by DC's guided bombs) are not read: neither are the trace views.
// TODO: on foot (GC's soldier) chase and front match the original's, but lying the front view comes nearer (his body some 1.4 times as
// wide as the original's, GC_Hoth): whether the original looks at another point than the eyes, or keeps its reach.
// TODO: the mortar's F10/F11 come nearer than the original's with the same radius (2.354, the server's): the reach or the centre of a
// `c_SeatIsOutside` seat?
pub(super) fn update_chase(g: &mut Game) {
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let seat = p.soldier.filter(|_| p.alive).and_then(|i| sim.vehicles[i].entry.inside.map(|k| (k, Some(sim.vehicles[i].entry.position))));
    // on foot with the soldier's own views (not `--third-person`'s): the same cameras about the soldier, looking at his eyes
    let foot = p.soldier.filter(|&i| p.alive && sim.vehicles[i].entry.inside.is_none() && !g.third_on_foot).map(|i| (i, None));
    let Some((k, seat)) = seat.or(foot).filter(|_| g.third_person && g.chase_yaw == 0.0 && g.fixed.is_none()) else {
        g.chase = None;
        return;
    };
    let v = &sim.vehicles[k];
    let at = match seat {
        Some(seat) => seat_camera(v, seat).1,
        None => on_foot_view(g, Vec2::ZERO).map_or(v.craft.body.pos, |(_, pos)| pos),
    };
    let at = assets::to_bevy(at);
    let body = &v.craft.body;
    let (fwd, up, vel) = (assets::to_bevy(body.rows[2]), assets::to_bevy(body.rows[1]), assets::to_bevy(body.vel));
    let mut c = match g.chase {
        Some(c) if c.vehicle == k => c,
        _ => Chase { vehicle: k, reach: sim.world.bounding_radius(&v.template) * 1.2, off: g.camera.1.translation - at, flyby: None, eye: at, look: at },
    };
    let r = c.reach;
    let ground = |e: Vec3, over: f32| sim.world.height_at(e.x as f64, -e.z as f64).map_or(e.y, |h| e.y.max(h as f32 + over));
    match g.outside {
        Outside::Chase | Outside::Front => {
            let side = if g.outside == Outside::Front { 1.0 } else { -1.0 };
            let mut to = side * (fwd * r + vel * 0.6) + up * r * 0.3;
            if g.orbit != Vec2::ZERO {
                let right = Vec3::new(to.x, 0.0, to.z).cross(Vec3::Y).normalize_or(Vec3::X);
                to = Quat::from_rotation_y(g.orbit.x) * (Quat::from_axis_angle(right, g.orbit.y) * to);
            }
            let off = c.off + (to - c.off) * (1.0 - (-2.0 * TICK as f32).exp());
            let mut eye = at + off;
            eye.y = ground(eye, 1.0);
            (c.off, c.eye, c.look, c.flyby) = (eye - at, eye, at, None);
        }
        Outside::FlyBy => {
            let far = r * 3.2;
            let passed = |p: Vec3| (at - p).dot(fwd) > 0.0 && at.distance(p) > far;
            let point = match c.flyby {
                Some(p) if !passed(p) => p,
                _ => {
                    let mut p = at + (fwd * r + vel * 0.3) * 3.2;
                    p.y = ground(p, 1.0);
                    p
                }
            };
            let mut eye = point;
            eye.y = ground(eye, 0.5);
            (c.flyby, c.off, c.eye, c.look) = (Some(point), eye - at, eye, at);
        }
    }
    g.chase = Some(c);
}

/// The player's soldier on foot and alive: its eyes' axes and position with `look` more mouse
/// units to come ([`bytefield_sim::soldier::Movement::view`]).
pub(super) fn on_foot_view(g: &Game, look: Vec2) -> Option<([[f32; 3]; 3], [f32; 3])> {
    let p = &g.session.sim.players[g.player];
    let s = &g.session.sim.vehicles[p.soldier.filter(|_| p.alive)?];
    if s.entry.inside.is_some() {
        return None;
    }
    Some(s.movement.as_ref()?.view(&s.craft.body, [look.x, look.y]))
}
