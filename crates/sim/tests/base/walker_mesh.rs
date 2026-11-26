//! The bots' walker on the walkable-surface mesh, with a soldier of the bench: synthetic obstacles on El_Alamein's desert
//! (a bridge, a block to drop off, a tower with a ladder), the mesh built over them, and one bot taken to a start and given a
//! goal. It must cross the bridge over the deck, come off the deck by a ramp (not over its edge), drop off the block, and
//! go up the ladder and down it, and jump out of a sealed room by its window, back in, and onto a ledge. Skipped without the
//! real data.
use bytefield_sim::bot_api::{BotView, Mind};
use bytefield_sim::bots::body::rng::Rng;
use bytefield_sim::bots::body::walker::Walker;
use bytefield_sim::bots::body::{pitch, Maps};
use bytefield_sim::navmesh::{NavMesh, Params};
use bytefield_sim::obstacle::Obstacle;
use bytefield_sim::soldier::axis;
use std::sync::{Arc, Mutex};

/// Where the obstacles stand: a flat stretch of El_Alamein's desert, each scenario 40 m east of the last.
const AT: [f32; 2] = [1100.0, 1100.0];
const APART: f32 = 40.0;

/// A ramp rising towards −z.
fn ramp_down(lo: [f32; 3], hi: [f32; 3]) -> Obstacle {
    let r = Obstacle::synthetic_ramp("ramp", lo, hi);
    let m = &r.lods[0];
    let verts = m.vertices.iter().map(|v| [v[0], v[1], lo[2] + hi[2] - v[2]]).collect();
    let tris: Vec<[u32; 3]> = m.faces.iter().map(|f| [f.v[0], f.v[2], f.v[1]]).collect();
    Obstacle::synthetic("ramp", verts, &tris)
}

/// The scenarios' obstacles, given the ground's height: at x0 a bridge (a deck 6 m up from z + 10 to z + 30, ramps at both
/// ends), at x1 a block 2.5 m high, at x2 a tower 6 m high with a ladder on its south face, at x3 a sealed room 8 m across
/// whose only way out is a window on its south wall (sill 0.8 m, gap 1.6 m, 1.2 m wide), and at x4 a ledge 0.95 m high (a jump link goes no higher than 1 m).
fn obstacles(g: f32) -> Vec<Obstacle> {
    let (x0, x1, x2, z) = (AT[0], AT[0] + APART, AT[0] + 2.0 * APART, AT[1]);
    let (x3, x4) = (AT[0] + 3.0 * APART, AT[0] + 4.0 * APART);
    vec![
        Obstacle::synthetic_box("north", [x3 - 4.4, g - 1.0, z + 18.0], [x3 + 4.4, g + 3.0, z + 18.4]),
        Obstacle::synthetic_box("east", [x3 + 4.0, g - 1.0, z + 10.0], [x3 + 4.4, g + 3.0, z + 18.0]),
        Obstacle::synthetic_box("west", [x3 - 4.4, g - 1.0, z + 10.0], [x3 - 4.0, g + 3.0, z + 18.0]),
        Obstacle::synthetic_box("south_w", [x3 - 4.4, g - 1.0, z + 9.6], [x3 - 0.6, g + 3.0, z + 10.0]),
        Obstacle::synthetic_box("south_e", [x3 + 0.6, g - 1.0, z + 9.6], [x3 + 4.4, g + 3.0, z + 10.0]),
        Obstacle::synthetic_box("sill", [x3 - 0.6, g - 1.0, z + 9.6], [x3 + 0.6, g + 0.8, z + 10.0]),
        Obstacle::synthetic_box("lintel", [x3 - 0.6, g + 2.4, z + 9.6], [x3 + 0.6, g + 3.0, z + 10.0]),
        Obstacle::synthetic_box("ledge", [x4 - 4.0, g - 1.0, z + 10.0], [x4 + 4.0, g + 0.95, z + 18.0]),
        Obstacle::synthetic_box("deck", [x0 - 3.0, g + 5.6, z + 10.0], [x0 + 3.0, g + 6.0, z + 30.0]),
        Obstacle::synthetic_ramp("ramp", [x0 - 3.0, g - 0.5, z], [x0 + 3.0, g + 6.0, z + 10.0]),
        ramp_down([x0 - 3.0, g - 0.5, z + 30.0], [x0 + 3.0, g + 6.0, z + 40.0]),
        Obstacle::synthetic_box("low", [x1 - 4.0, g - 1.0, z + 10.0], [x1 + 4.0, g + 2.5, z + 18.0]),
        Obstacle::synthetic_box("tower", [x2 - 4.0, g - 1.0, z + 10.0], [x2 + 4.0, g + 6.0, z + 18.0]),
        Obstacle::synthetic_ladder("ladder", [x2 - 0.5, g - 0.5, z + 9.7], [x2 + 0.5, g + 6.0, z + 10.0]),
    ]
}

/// A bot that walks to the goal it is given, with the walker alone (no flags, no fight).
struct GoTo {
    walker: Walker,
    rng: Rng,
    maps: Arc<Maps>,
    goal: Arc<Mutex<Option<[f32; 3]>>>,
}

impl Mind for GoTo {
    fn think(&mut self, v: &BotView) -> Vec<(usize, f32)> {
        let Some(goal) = *self.goal.lock().unwrap() else { return vec![(axis::THROTTLE, 0.0)] };
        let me = &v.me;
        let f = me.view[2];
        let step = self.walker.next_step(v, &mut self.rng, &self.maps, true, goal, 1.0, true, false);
        let mut axes = vec![(axis::MOUSE_Y, self.walker.look(v).unwrap_or((pitch(f) / 2.5).clamp(-4.0, 4.0)))];
        let dir = step.map(|p| [p[0] - me.pos[0], p[1] - me.pos[2]]);
        self.walker.walk(f, dir, &mut axes);
        self.walker.climb(v, &mut axes);
        self.walker.leap(v, &self.maps, &mut axes);
        if let Some(flight) = self.walker.unjam(v, &mut self.rng, me.player, step, dir.is_some(), true, &mut axes) {
            return flight;
        }
        if std::env::var("WALK_TRACE").is_ok() && ((v.time * 30.0).round() as u64).is_multiple_of(15) {
            eprintln!("    path {:?} axes {axes:?}", self.walker.path.iter().take(4).map(|p| p.map(|c| (c * 10.0).round() / 10.0)).collect::<Vec<_>>());
        }
        axes
    }
}

/// What a walk came to: where the soldier ended, the highest its feet went over the ground, its health (0..1) and when it
/// got within 3 m of the goal across and 1 m in height.
#[derive(Debug)]
struct Walk {
    end: [f32; 3],
    top: f32,
    health: f32,
    arrived_s: Option<f32>,
}

struct Bench<'w> {
    sim: bytefield_sim::Sim<'w>,
    player: usize,
    goal: Arc<Mutex<Option<[f32; 3]>>>,
    ground: f32,
}

/// El_Alamein with the scenarios' obstacles and their mesh, and the ground's height there.
fn world() -> (bytefield_sim::World, f32) {
    let g = bytefield_testdata::base().unwrap();
    let mut w = bytefield_sim::World::load(&g, "bf1942", "El_Alamein", "conquest").unwrap();
    let ground = w.height_at(AT[0] as f64, AT[1] as f64).unwrap() as f32;
    w.push_obstacles(obstacles(ground));
    let mesh = NavMesh::build_in(&w, &Params::infantry(&w), 4, Some([AT[0] - 40.0, AT[1] - 40.0, AT[0] + 5.0 * APART + 40.0, AT[1] + 80.0]));
    assert!(mesh.links.iter().any(|l| l.kind == bytefield_sim::navmesh::LinkKind::Ladder), "the ladder's link");
    if std::env::var("WALK_TRACE").is_ok() {
        let q = bytefield_sim::navmesh::query::Query::new(std::sync::Arc::new(mesh.clone()));
        let (x2, z) = (AT[0] + 2.0 * APART, AT[1]);
        for zz in [5.0f32, 20.0, 35.0, -10.0] {
            for p in q.polys_at(AT[0], z + zz) {
                eprintln!("bridge z+{zz}: poly {p} h {:.2} ground {} zone {}", q.mesh.height(p, AT[0], z + zz) - ground, q.mesh.polys[p as usize].ground, q.zone[p as usize]);
            }
        }
        for p in q.polys_at(x2, z + 14.0) {
            eprintln!("top poly {p}: h {:.2} ground {} zone {}", q.mesh.height(p, x2, z + 14.0) - ground, q.mesh.polys[p as usize].ground, q.zone[p as usize]);
        }
        eprintln!("locate_2d {:?}; at the start {:?} zone {:?}", q.locate_2d(x2, z + 14.0), q.locate([x2, ground, z], 1.0, 1.5), q.locate([x2, ground, z], 1.0, 1.5).map(|p| q.zone[p as usize]));
        for l in mesh.links.iter().filter(|l| l.kind == bytefield_sim::navmesh::LinkKind::Ladder) {
            eprintln!("ladder {l:?} zones {} {}", q.zone[l.a as usize], q.zone[l.b as usize]);
        }
    }
    assert!(w.set_navmesh(mesh));
    (w, ground)
}

impl<'w> Bench<'w> {
    fn new(w: &'w bytefield_sim::World, ground: f32) -> Bench<'w> {
        let maps = Maps::prepare(w);
        let mut sim = bytefield_sim::Sim::new(w, 1.0 / 30.0);
        sim.round.playing = true;
        sim.start_conquest();
        sim.enable_bots();
        let goal: Arc<Mutex<Option<[f32; 3]>>> = Arc::new(Mutex::new(None));
        let (m, gl) = (maps.clone(), goal.clone());
        sim.set_minds(1, Box::new(move |k| Box::new(GoTo { walker: Walker { seeks_way_out: true, ..Walker::default() }, rng: Rng::new(k as u64 + 1), maps: m.clone(), goal: gl.clone() })));
        let player = sim.join(1, true);
        let mut guard = 0;
        while !sim.players[player].alive && guard < 900 {
            sim.step();
            guard += 1;
        }
        assert!(sim.players[player].alive, "the bot spawned");
        Bench { sim, player, goal, ground }
    }

    /// Puts the soldier at `from` (its feet), tells it `to`, and lets it walk for `seconds`.
    fn walk(&mut self, from: [f32; 3], to: [f32; 3], seconds: f32) -> Walk {
        let s = &mut self.sim;
        let i = s.players[self.player].soldier.unwrap();
        *self.goal.lock().unwrap() = None;
        s.vehicles[i].craft.body.pos = [from[0], from[1] + 1.0, from[2]];
        s.vehicles[i].craft.body.vel = [0.0; 3];
        for _ in 0..15 {
            s.step();
        }
        *self.goal.lock().unwrap() = Some(to);
        let mut w = Walk { end: [0.0; 3], top: f32::MIN, health: 1.0, arrived_s: None };
        for k in 0..(seconds * 30.0) as usize {
            s.step();
            let p = s.vehicles[i].craft.body.pos;
            if std::env::var("WALK_TRACE").is_ok() && k % 15 == 0 {
                eprintln!("  t {:.1} feet {:.1} {:.1} {:.1} ladder {}", k as f32 / 30.0, p[0], p[1] - 1.0 - self.ground, p[2], s.vehicles[i].soldier().is_some_and(|m| m.climbing()));
            }
            w.top = w.top.max(p[1] - 1.0 - self.ground);
            if w.arrived_s.is_none() && (p[0] - to[0]).hypot(p[2] - to[2]) < 3.0 && (p[1] - 1.0 - to[1]).abs() < 1.0 {
                w.arrived_s = Some(k as f32 / 30.0);
            }
        }
        if std::env::var("WALK_TRACE").is_ok() { eprintln!("walk {from:?} -> {to:?}"); }
        let v = &s.vehicles[i];
        w.end = v.craft.body.pos;
        w.health = v.armor.as_ref().map_or(1.0, |a| a.hp / a.max_hp);
        if std::env::var("WALK_TRACE").is_ok() {
            eprintln!("  {w:?}");
        }
        w
    }
}

#[test]
fn the_walker_on_the_mesh() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let (w, ground) = world();
    let mut b = Bench::new(&w, ground);
    let g = b.ground;
    let (x0, x1, x2, z) = (AT[0], AT[0] + APART, AT[0] + 2.0 * APART, AT[1]);
    // across the bridge, ground to ground: over the deck (straight on, the ramps are walls)
    let w = b.walk([x0, g, z - 10.0], [x0, g, z + 50.0], 40.0);
    assert!(w.arrived_s.is_some() && w.top > 5.0, "across the bridge over its deck: {w:?}");
    // off the deck to the ground beside it: down a ramp, not over the 6 m edge
    let w = b.walk([x0, g + 6.0, z + 20.0], [x0 + 15.0, g, z + 20.0], 40.0);
    assert!(w.arrived_s.is_some() && w.health > 0.99, "off the deck by a ramp: {w:?}");
    // off the block: over its edge, unharmed
    let w = b.walk([x1, g + 2.5, z + 14.0], [x1, g, z - 5.0], 20.0);
    assert!(w.arrived_s.is_some() && w.health > 0.99, "off the block: {w:?}");
    // up the tower's ladder
    let w = b.walk([x2, g, z], [x2, g + 6.0, z + 14.0], 40.0);
    assert!(w.arrived_s.is_some(), "up the ladder: {w:?}");
    // and down it: 6 m is too high to drop
    let w = b.walk([x2, g + 6.0, z + 14.0], [x2, g, z], 40.0);
    assert!(w.arrived_s.is_some() && w.health > 0.99, "down the ladder: {w:?}");
    // out of the sealed room through its window, jumping
    let (x3, x4) = (AT[0] + 3.0 * APART, AT[0] + 4.0 * APART);
    let w = b.walk([x3, g, z + 14.0], [x3, g, z], 30.0);
    assert!(w.arrived_s.is_some() && w.health > 0.99, "out of the room by the window: {w:?}");
    // and back in
    let w = b.walk([x3 + 3.0, g, z + 2.0], [x3, g, z + 14.0], 30.0);
    assert!(w.arrived_s.is_some() && w.health > 0.99, "into the room by the window: {w:?}");
    // onto the ledge, jumping
    let w = b.walk([x4, g, z], [x4, g + 0.95, z + 14.0], 30.0);
    assert!(w.arrived_s.is_some(), "onto the ledge: {w:?}");
}
