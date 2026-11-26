//! A still person among bots: a person with no input spawns and stays where it is put, life after life, and it is measured how long the bots take to shoot at it, hit it and kill it, and who:
//!   idle_person <game> <mod> <level> <players> <seconds> [--mind bots[:variant]] [--seed N] [--team 1|2] [--at x,y,z] [--out x.json]
//! Without `--at`, each life puts it beside the takeable flag nearest the centre of the flags (where the fighting is). The given mind
//! plays on both sides. Prints one line per life (seconds since it spawns until the first enemy shot heading its way,
//! within 2 m of its chest, until the first hit and until death, and who kills it) and a summary (medians; a life that does not end
//! counts with how long it lasted: it is a bound).
use bytefield_sim::bot_api::MindMaker;
use std::path::Path;

fn mind_of(name: &str, level: &str, seed: u64) -> MindMaker {
    match name {
        n if bytefield_sim::bots::is_name(n) => bytefield_sim::bots::mind(n, level, seed),
        other => panic!("no mind called {other}"),
    }
}

#[derive(Default, Debug, Clone, serde::Serialize)]
struct Life {
    spawned: f64,
    /// the first living enemy within 40 m (ignoring walls: the example does not see `Sim::hidden`)
    near: Option<f64>,
    shot_at: Option<f64>,
    hit: Option<f64>,
    dead: Option<f64>,
    lasted: f64,
    killer: Option<usize>,
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    (!v.is_empty()).then(|| v[v.len() / 2])
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 6 {
        eprintln!("idle_person <game> <mod> <level> <players> <seconds> [--mind bots[:variant]] [--seed N] [--team 1|2] [--at x,y,z] [--out x.json]");
        std::process::exit(2);
    }
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let (players, seconds): (usize, f64) = (a[4].parse().unwrap(), a[5].parse().unwrap());
    let w = bytefield_sim::World::load(Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    let seed: u64 = opt("--seed").map_or(0, |n| n.parse().unwrap());
    if seed != 0 {
        s.rng = bytefield_sim::players::Rng(s.rng.0 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    let mind = opt("--mind").unwrap_or("bots");
    s.set_minds(1, mind_of(mind, &a[3], seed));
    s.set_minds(2, mind_of(mind, &a[3], seed));
    for i in 0..players {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    let team: u32 = opt("--team").map_or(1, |t| t.parse().unwrap());
    let person = s.join_person(team);
    // where: as asked, or beside the takeable flag nearest the centre of all of them
    let at: [f32; 3] = match opt("--at") {
        Some(x) => {
            let v: Vec<f32> = x.split(',').map(|q| q.parse().expect("--at x,y,z")).collect();
            [v[0], v[1], v[2]]
        }
        None => {
            let c = s.modes.conquest.as_ref().expect("conquest");
            let n = c.points.len().max(1) as f32;
            let centre = [c.points.iter().map(|p| p.pos[0]).sum::<f32>() / n, c.points.iter().map(|p| p.pos[2]).sum::<f32>() / n];
            let d = |p: [f32; 3]| (p[0] - centre[0]).hypot(p[2] - centre[1]);
            let f = c.points.iter().filter(|p| !p.template.unable_to_change_team).min_by(|x, y| d(x.pos).total_cmp(&d(y.pos))).expect("a takeable flag");
            let (x, z) = (f.pos[0] + 3.0, f.pos[2] + 3.0);
            let h = s.world.height_at(x as f64, z as f64).unwrap_or(f.pos[1] as f64) as f32;
            [x, h + 1.0, z]
        }
    };
    let mut lives: Vec<Life> = Vec::new();
    let mut placed = None;
    let (mut fired_seen, mut damage_seen) = (0usize, 0usize);
    let steps = (seconds * 30.0) as u64;
    for _ in 0..steps {
        s.step();
        let t = s.clock.steps as f64 / 30.0;
        let alive = s.players[person].soldier.filter(|_| s.players[person].alive);
        // each life, in its place
        if let Some(i) = alive
            && placed != Some(i)
        {
            placed = Some(i);
            s.vehicles[i].craft.body.pos = at;
            lives.push(Life { spawned: t, ..Default::default() });
        }
        let player_of = |v: usize| s.players.iter().position(|p| p.alive && p.soldier.is_some_and(|i| i == v || s.vehicles[i].entry.inside == Some(v)));
        // (this step's, even if it died in it: the soldier of the life it is on)
        if let (Some(i), Some(life)) = (placed, lives.last_mut().filter(|v| v.dead.is_none())) {
            let p = s.vehicles[i].craft.body.pos;
            let chest = [p[0], p[1] + 0.3, p[2]];
            if life.near.is_none()
                && s.players.iter().any(|q| {
                    q.alive
                        && q.team != team
                        && (1..=2).contains(&q.team)
                        && q.soldier.is_some_and(|j| {
                            let e = s.vehicles[j].craft.body.pos;
                            (e[0] - p[0]).hypot(e[2] - p[2]) < 40.0
                        })
                })
            {
                life.near = Some(t - life.spawned);
            }
            for f in s.journal.fired.get(fired_seen..).unwrap_or(&[]) {
                let Some(k) = player_of(f.1) else { continue };
                if s.players[k].team == team {
                    continue;
                }
                let (o, d) = (f.2.pos, f.2.vel);
                let ld = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                if ld <= 0.0 {
                    continue;
                }
                let q = [chest[0] - o[0], chest[1] - o[1], chest[2] - o[2]];
                let along = (q[0] * d[0] + q[1] * d[1] + q[2] * d[2]) / ld;
                if !(0.0..150.0).contains(&along) {
                    continue;
                }
                let off2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] - along * along;
                if off2 < 4.0 && life.shot_at.is_none() {
                    life.shot_at = Some(t - life.spawned);
                }
            }
            for e in s.journal.damage_log.get(damage_seen..).unwrap_or(&[]) {
                if e.vehicle != i || e.amount <= 0.0 {
                    continue;
                }
                life.hit.get_or_insert(t - life.spawned);
                if e.hp <= 0.0 {
                    life.killer = e.by.and_then(player_of);
                }
            }
            life.lasted = t - life.spawned;
        }
        if alive.is_none()
            && let Some(life) = lives.last_mut().filter(|v| v.dead.is_none())
        {
            life.dead = Some(t - life.spawned);
            life.lasted = t - life.spawned;
        }
        fired_seen = s.journal.fired.len();
        damage_seen = s.journal.damage_log.len();
    }
    for (n, v) in lives.iter().enumerate() {
        let f = |x: Option<f64>| x.map_or("-".to_string(), |x| format!("{x:.1}"));
        println!(
            "life {n}: spawns at {:.0} s; enemy within 40 m {} s, shot at {} s, hit {} s, dead {} s (lasted {:.0} s), killer {:?}",
            v.spawned,
            f(v.near),
            f(v.shot_at),
            f(v.hit),
            f(v.dead),
            v.lasted,
            v.killer
        );
    }
    let med = |g: fn(&Life) -> Option<f64>| median(lives.iter().map(|v| g(v).unwrap_or(v.lasted)).collect());
    // since it has an enemy within 40 m until the first shot towards it (the lives with both)
    let reaction = median(lives.iter().filter_map(|v| Some((v.shot_at? - v.near?).max(0.0))).collect());
    let r = serde_json::json!({"level": a[3], "mind": mind, "seed": seed, "at": at, "lives": lives.len(), "median_s": {"shot_at": med(|v| v.shot_at), "hit": med(|v| v.hit), "dead": med(|v| v.dead), "near_to_shot": reaction}, "lives_detail": lives});
    println!(
        "summary {} {} seed {}: {} lives; median shot at {:.1} s, hit {:.1} s, dead {:.1} s, enemy within 40 m to shot {:.1} s",
        a[3],
        mind,
        seed,
        lives.len(),
        med(|v| v.shot_at).unwrap_or(f64::NAN),
        med(|v| v.hit).unwrap_or(f64::NAN),
        med(|v| v.dead).unwrap_or(f64::NAN),
        reaction.unwrap_or(f64::NAN)
    );
    if let Some(out) = opt("--out") {
        std::fs::write(out, serde_json::to_string_pretty(&r).unwrap()).expect("--out");
    }
}
