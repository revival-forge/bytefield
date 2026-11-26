//! Does a shot from an aircraft hit another aircraft in flight? A target flying straight and level and a shooter behind it at the
//! same speed, its guns held down. Both are kept on their rails each frame: the question is the shot against something that
//! moves fast, not the flying.
use bytefield_sim::soldier::axis;

/// What came of the burst.
#[derive(Debug, Default)]
pub struct TailShots {
    /// projectiles the shooter fired
    pub shots: usize,
    /// of those, the ones that hit the target
    pub hits: usize,
    /// what the target lost
    pub damage: f32,
    /// where each shot crossed the target's plane, off its centre: right and up, m
    pub crossings: Vec<[f32; 2]>,
}

/// `shooter` flies `gap` m behind `target`, both at `speed` m/s along +z, 300 m over the middle of the map, and fires its
/// pilot's guns for `seconds`. `offset`: how far to the right and above the target's line the shooter flies, m.
pub fn tail_shots(w: &bytefield_sim::World, shooter: &str, target: &str, gap: f32, offset: [f32; 2], speed: f32, seconds: f32) -> TailShots {
    let mut s = bytefield_sim::Sim::new(w, 1.0 / 30.0);
    s.round.playing = true;
    let (x, z) = (1024.0f32, 1024.0f32);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 300.0;
    let t = s.spawn_vehicle(target, [x, y, z], [0.0; 3]).unwrap_or_else(|| panic!("{target}"));
    let a = s.spawn_vehicle(shooter, [x + offset[0], y + offset[1], z - gap], [0.0; 3]).unwrap_or_else(|| panic!("{shooter}"));
    for (team, craft) in [(1, a), (2, t)] {
        let soldier = s.spawn_vehicle("USSoldier", [x + 40.0, y, z], [0.0; 3]).expect("a soldier");
        s.vehicles[soldier].team = team;
        s.enter(soldier, craft, 0);
        let keys: &[(usize, f32)] = if craft == a { &[(axis::THROTTLE, 1.0), (axis::FIRE, 1.0)] } else { &[(axis::THROTTLE, 1.0)] };
        s.vehicles[soldier].soldier_mut().unwrap().set_input(keys);
    }
    let rails = [(t, [x, y]), (a, [x + offset[0], y + offset[1]])];
    let mut out = TailShots::default();
    let mut before: Vec<Option<[f32; 3]>> = Vec::new();
    for _ in 0..(seconds * 30.0) as usize {
        for (craft, at) in rails {
            let b = &mut s.vehicles[craft].craft.body;
            b.rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            (b.pos[0], b.pos[1]) = (at[0], at[1]);
            (b.vel, b.rotvel) = ([0.0, 0.0, speed], [0.0; 3]);
        }
        s.step();
        let tp = s.vehicles[t].craft.body.pos;
        before.resize(s.projectiles.len(), None);
        for (k, p) in s.projectiles.iter().enumerate().filter(|(_, p)| p.owner == Some(a)) {
            if let Some(q) = before[k]
                && q[2] < tp[2]
                && p.pos[2] >= tp[2]
            {
                let f = (tp[2] - q[2]) / (p.pos[2] - q[2]).max(1e-3);
                out.crossings.push([q[0] + (p.pos[0] - q[0]) * f - tp[0], q[1] + (p.pos[1] - q[1]) * f - tp[1]]);
            }
            before[k] = Some(p.pos);
        }
    }
    let mine = || s.projectiles.iter().filter(|p| p.owner == Some(a));
    out.shots = mine().count();
    out.hits = mine().filter(|p| p.impacts.iter().any(|i| i.vehicle == Some(t))).count();
    out.damage = s.journal.damage_log.iter().filter(|e| e.vehicle == t).map(|e| e.amount).sum();
    out
}
