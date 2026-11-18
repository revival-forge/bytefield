//! Measures, from outside and at 1 Hz, `bots2`'s specification figures over several rounds:
//!   bots2_measure <game> <mod> <level> <players> <seconds> <rounds> [--mind bots2] [--both] [--seed0 N] [--out f.json]
//! Time is shifted so the first to spawn does so at t=21, as in the reference recordings (here spawning is at t≈0).
use bytefield_sim::{Sim, World};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Default, Clone)]
struct Round(BTreeMap<String, f64>);

impl Round {
    fn put(&mut self, k: &str, v: f64) {
        self.0.insert(k.to_string(), v);
    }
}

fn q(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}
fn d3(a: [f32; 3], b: [f32; 3]) -> f64 {
    (0..3).map(|k| ((a[k] - b[k]) as f64).powi(2)).sum::<f64>().sqrt()
}
fn d2(a: [f32; 3], b: [f32; 3]) -> f64 {
    ((a[0] - b[0]) as f64).hypot((a[2] - b[2]) as f64)
}

#[derive(Clone)]
struct Sample {
    alive: bool,
    pos: [f32; 3],
    veh: Option<(String, usize, bool)>,
    team: u32,
}

/// (t, player, on foot, weapon, 3D distance to the nearest enemy)
struct Shot(f64, usize, bool, String, f64);

fn round(w: &World, players: usize, seconds: f64, mind: Option<&str>, both: bool, seed: u64, level: &str) -> Round {
    let mut s = Sim::new(w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.rng = bytefield_sim::players::Rng(s.rng.0 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    if mind == Some("bots2") {
        s.set_minds(1, bytefield_sim::bots2::maker(level, seed));
        if both {
            s.set_minds(2, bytefield_sim::bots2::maker(level, seed));
        }
    }
    for i in 0..players {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    let mut history: Vec<Vec<Sample>> = Vec::new();
    let mut shots: Vec<Shot> = Vec::new();
    let mut seen = 0;
    let mut watcher = bytefield_sim::dumb::Watcher::new();
    let mut flags_owner: Vec<Vec<u32>> = Vec::new();
    let steps = (seconds * 30.0) as u64;
    for _ in 0..steps {
        s.step();
        watcher.step(&s);
        let alive_now = |s: &Sim| -> Vec<(u32, [f32; 3])> {
            s.players
                .iter()
                .filter(|p| p.alive)
                .filter_map(|p| Some((p.team, s.vehicles.get(p.soldier?)?)))
                .map(|(t, v)| (t, v.entry.inside.map_or(v.craft.body.pos, |c| s.vehicles[c].craft.body.pos)))
                .collect()
        };
        let v = alive_now(&s);
        for f in s.journal.fired.get(seen..).unwrap_or(&[]) {
            let seat = f.3.map(|i| s.projectiles[i].seat);
            let inside = |p: &bytefield_sim::players::Player| p.alive && p.soldier.is_some_and(|i| s.vehicles[i].entry.inside == Some(f.1) && seat.is_none_or(|k| s.vehicles[i].entry.position == k));
            let Some(k) = s.players.iter().position(|p| p.soldier == Some(f.1)).or_else(|| s.players.iter().position(inside)) else {
                continue;
            };
            let on_foot = s.players[k].soldier == Some(f.1);
            let eq = s.players[k].team;
            let d = v.iter().filter(|x| x.0 != eq).map(|x| d3(x.1, f.2.pos)).fold(f64::MAX, f64::min);
            if d > 55.0 && d < 1e6 && std::env::var_os("MEASURE_FAR").is_some() {
                eprintln!(
                    "far seed {seed} t {:.2} p{k} on_foot {on_foot} {} d {d:.1} muzzle {:?}",
                    f.0 as f64 / 30.0,
                    f.3.map_or(String::new(), |i| s.projectiles[i].template.name.clone()),
                    f.2.pos.map(|x| x as i32)
                );
            }
            shots.push(Shot(f.0 as f64 / 30.0, k, on_foot, f.3.map_or(String::new(), |i| s.projectiles[i].template.name.clone()), d));
        }
        seen = s.journal.fired.len();
        if s.clock.steps % 30 == 0 {
            history.push(
                s.players
                    .iter()
                    .map(|p| {
                        let sol = p.soldier.filter(|_| p.alive).and_then(|i| s.vehicles.get(i));
                        let ins = sol.and_then(|v| v.entry.inside);
                        Sample {
                            alive: sol.is_some(),
                            pos: sol.map_or([0.0; 3], |v| ins.map_or(v.craft.body.pos, |c| s.vehicles[c].craft.body.pos)),
                            veh: ins.map(|c| (s.vehicles[c].template.clone(), c, s.vehicles[c].fixed)),
                            team: p.team,
                        }
                    })
                    .collect(),
            );
            flags_owner.push(s.modes.conquest.as_ref().map_or(Vec::new(), |c| c.points.iter().map(|p| p.team).collect()));
        }
        if s.round.round_end.is_some() {
            break;
        }
    }
    let report = watcher.finish(serde_json::json!({}));
    let t0 = history.iter().position(|m| m.iter().any(|x| x.alive)).unwrap_or(0);
    let tr = |i: usize| i as f64 - t0 as f64 + 21.0; // second of sample i
    let n = s.players.len();
    let mut r = Round::default();
    let end = history.len();
    let dur_t = tr(end);
    r.put("t end", dur_t);
    let enemy_dist = |m: &[Sample], k: usize| -> f64 {
        m.iter()
            .enumerate()
            .filter(|(j, x)| *j != k && x.alive && x.team != m[k].team)
            .map(|(_, x)| d3(x.pos, m[k].pos))
            .fold(f64::MAX, f64::min)
    };

    // ---- disparos
    let tt = |t: f64| t + 1.0 - (t0 as f64 + 1.0) + 21.0; // real t to reference t (sample 0 is t=1 s)
    r.put("01 shots", shots.len() as f64);
    let mut per_bot = vec![0.0; n];
    let mut first = vec![f64::MAX; n];
    for x in &shots {
        per_bot[x.1] += 1.0;
        first[x.1] = first[x.1].min(tt(x.0));
    }
    r.put("03 bots that fire", per_bot.iter().filter(|x| **x > 0.0).count() as f64);
    r.put("02 shots per bot", q(&mut per_bot.clone(), 0.5));
    r.put("04 fraction on foot", shots.iter().filter(|x| x.2).count() as f64 / (shots.len().max(1)) as f64);
    let mut ds: Vec<f64> = shots.iter().filter(|x| x.4 < 1e6).map(|x| x.4).collect();
    r.put("05 dist when firing", q(&mut ds, 0.5));
    r.put("06 p90 dist", q(&mut ds, 0.9));
    r.put("shot limit >60m", ds.iter().filter(|d| **d > 60.0).count() as f64);
    r.put("shot limit >55m", ds.iter().filter(|d| **d > 55.0).count() as f64);
    r.put("max shot dist", ds.iter().copied().fold(0.0, f64::max));
    if !shots.is_empty() {
        r.put("08 first shot", shots.iter().map(|x| tt(x.0)).fold(f64::MAX, f64::min));
    }
    let mut pr: Vec<f64> = first.iter().filter(|x| **x < f64::MAX).copied().collect();
    r.put("09 first shot per bot", q(&mut pr, 0.5));

    // ---- muertes, vidas
    let mut deaths = 0.0;
    let mut first_death = f64::MAX;
    let mut lifetimes: Vec<f64> = Vec::new();
    let mut lives = 0.0;
    let mut lives_boarding = 0.0;
    let mut lives_without_firing = 0.0;
    let mut since_spawn: Vec<f64> = Vec::new();
    let mut board_wait: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut stays: BTreeMap<String, (Vec<f64>, [u32; 3])> = BTreeMap::new();
    for k in 0..n {
        let mut born: Option<usize> = None;
        let mut boarded = false;
        let mut inside: Option<(String, usize)> = None;
        for i in 0..end {
            let m = &history[i][k];
            if m.alive && born.is_none() {
                born = Some(i);
                boarded = false;
                inside = None;
                lives += 1.0;
            }
            if m.alive {
                let v = m.veh.as_ref().map(|x| x.0.clone());
                match (&inside, &v) {
                    (None, Some(t)) => {
                        inside = Some((t.clone(), i));
                        if !boarded {
                            lives_boarding += 1.0;
                            board_wait.entry(t.clone()).or_default().push((i - born.unwrap()) as f64);
                        }
                        boarded = true;
                    }
                    (Some((t, d)), None) => {
                        let e = stays.entry(t.clone()).or_default();
                        e.0.push((i - d) as f64);
                        e.1[0] += 1;
                        inside = None;
                    }
                    _ => {}
                }
            }
            if !m.alive
                && let Some(b) = born.take()
            {
                deaths += 1.0;
                first_death = first_death.min(tr(i));
                lifetimes.push((i - b) as f64);
                if let Some((t, d)) = inside.take() {
                    let e = stays.entry(t).or_default();
                    e.0.push((i - d) as f64);
                    e.1[1] += 1;
                }
                let (ta, tb) = (b as f64 + 1.0, i as f64 + 1.0);
                let first = shots.iter().filter(|x| x.1 == k && x.0 >= ta && x.0 < tb).map(|x| x.0).fold(f64::MAX, f64::min);
                if first == f64::MAX {
                    lives_without_firing += 1.0;
                } else {
                    since_spawn.push(first - ta);
                }
            }
        }
        if let Some((t, d)) = inside.take() {
            let e = stays.entry(t).or_default();
            e.0.push((end - d) as f64);
            e.1[2] += 1;
        }
    }
    r.put("12 deaths", deaths);
    r.put("10 shots per death", shots.len() as f64 / f64::max(deaths, 1.0));
    if first_death < f64::MAX {
        r.put("11 first death", first_death);
    }
    let alive_s: f64 = history.iter().map(|m| m.iter().filter(|x| x.alive).count() as f64).sum();
    r.put("13 deaths/min/player", deaths / (n as f64 * (dur_t - 21.0) / 60.0));
    r.put("14 life duration", q(&mut lifetimes, 0.5));
    let in_vehicle: f64 = history.iter().map(|m| m.iter().filter(|x| x.alive && x.veh.is_some()).count() as f64).sum();
    r.put("15 time in vehicle", in_vehicle / alive_s.max(1.0));
    r.put("16 lives that board", lives_boarding / f64::max(lives, 1.0));
    r.put("lives without firing", lives_without_firing / f64::max(deaths, 1.0));
    r.put("first shot since spawn", q(&mut since_spawn, 0.5));
    for (t, mut e) in board_wait {
        r.put(&format!("wait to board {t}"), q(&mut e, 0.5));
    }
    for (t, (mut e, c)) in stays {
        r.put(&format!("stay {t} med"), q(&mut e, 0.5));
        r.put(&format!("stay {t} leaves/dies/cut"), (c[0] * 10000 + c[1] * 100 + c[2]) as f64);
    }

    // ---- on foot, per second (discarding the first 25 reference seconds)
    let from = t0 + 4;
    let fired_in = |k: usize, i: usize| shots.iter().any(|x| x.1 == k && x.2 && x.0 >= i as f64 && x.0 < i as f64 + 1.0);
    let (mut cal, mut cal_q, mut enemy_near, mut enemy_near_still, mut dis, mut dis_q) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let mut speeds: Vec<f64> = Vec::new();
    let (mut s40, mut s40_firing) = (0.0, 0.0);
    let mut buckets = [[0.0f64; 2]; 5];
    let (mut mate_d, mut mate15, mut mates15_3): (Vec<f64>, f64, f64) = (Vec::new(), 0.0, 0.0);
    let (mut still_calm_flag, mut still_calm_total) = (0.0, 0.0);
    let flags: Vec<[f32; 3]> = s.modes.conquest.as_ref().map_or(Vec::new(), |c| c.points.iter().map(|p| p.pos).collect());
    for i in from.max(1)..end {
        for k in 0..n {
            let (a, b) = (&history[i - 1][k], &history[i][k]);
            if !a.alive || !b.alive || b.veh.is_some() || a.veh.is_some() {
                continue;
            }
            let v = d2(a.pos, b.pos);
            let e = enemy_dist(&history[i], k);
            let firing = fired_in(k, i);
            let still = v < 1.0;
            if e > 55.0 {
                cal += 1.0;
                if still {
                    cal_q += 1.0;
                    still_calm_total += 1.0;
                    if flags.iter().any(|f| d3(*f, b.pos) < 30.0) {
                        still_calm_flag += 1.0;
                    }
                } else {
                    speeds.push(v);
                }
            } else {
                enemy_near += 1.0;
                enemy_near_still += still as i32 as f64;
            }
            if firing {
                dis += 1.0;
                dis_q += still as i32 as f64;
            }
            if e < 40.0 {
                s40 += 1.0;
                s40_firing += firing as i32 as f64;
            }
            let c = if e < 10.0 {
                0
            } else if e < 25.0 {
                1
            } else if e < 40.0 {
                2
            } else if e < 55.0 {
                3
            } else if e < 70.0 {
                4
            } else {
                9
            };
            if c < 5 {
                buckets[c][0] += 1.0;
                buckets[c][1] += firing as i32 as f64;
            }
            let mates: Vec<f64> = history[i]
                .iter()
                .enumerate()
                .filter(|(j, x)| *j != k && x.alive && x.team == b.team && x.veh.is_none())
                .map(|(_, x)| d3(x.pos, b.pos))
                .collect();
            if !mates.is_empty() {
                mate_d.push(mates.iter().copied().fold(f64::MAX, f64::min));
                mate15 += mates.iter().any(|x| *x < 15.0) as i32 as f64;
                mates15_3 += (mates.iter().filter(|x| **x < 15.0).count() >= 3) as i32 as f64;
            }
        }
    }
    r.put("07 s with shot <40m", s40_firing / f64::max(s40, 1.0));
    r.put("still calm", cal_q / f64::max(cal, 1.0));
    r.put("still enemy<55", enemy_near_still / f64::max(enemy_near, 1.0));
    r.put("still firing", dis_q / f64::max(dis, 1.0));
    r.put("calm speed moving", q(&mut speeds, 0.5));
    r.put("calm speed p90", q(&mut speeds, 0.9));
    r.put("still calm near flag", still_calm_flag / f64::max(still_calm_total, 1.0));
    for (c, name) in ["<10", "10-25", "25-40", "40-55", "55-70"].iter().enumerate() {
        r.put(&format!("s with shot {name}"), buckets[c][1] / f64::max(buckets[c][0], 1.0));
    }
    let mate_total = mate_d.len() as f64;
    r.put("teammate median", q(&mut mate_d, 0.5));
    r.put("teammate <15m", mate15 / mate_total.max(1.0));
    r.put("teammates >=3 <15m", mates15_3 / mate_total.max(1.0));

    // ---- stops ≥30 s on foot, and 20 s windows
    let (mut stops, mut stop_max) = (0.0, 0.0f64);
    let (mut windows, mut windows_bad, mut nets) = (0.0, 0.0, Vec::new());
    for k in 0..n {
        let mut anchor: Option<(usize, [f32; 3])> = None;
        for i in from..end {
            let m = &history[i][k];
            if !m.alive || m.veh.is_some() {
                if let Some((a, _)) = anchor.take()
                    && i - a >= 30
                {
                    stops += 1.0;
                    stop_max = stop_max.max((i - a) as f64);
                }
                continue;
            }
            match anchor {
                Some((a, p)) if d2(p, m.pos) > 2.0 => {
                    if i - a >= 30 {
                        if std::env::var_os("MEASURE_STOPS").is_some() {
                            eprintln!("stop p{k} t{} {} s at {:?}", a, i - a, p.map(|x| x as i32));
                        }
                        stops += 1.0;
                        stop_max = stop_max.max((i - a) as f64);
                    }
                    anchor = Some((i, m.pos));
                }
                None => anchor = Some((i, m.pos)),
                _ => {}
            }
            if i >= from + 20 && (i - 20..=i).all(|j| history[j][k].alive && history[j][k].veh.is_none()) {
                let travelled: f64 = (i - 19..=i).map(|j| d2(history[j - 1][k].pos, history[j][k].pos)).sum();
                if travelled >= 30.0 {
                    let net = d2(history[i - 20][k].pos, m.pos) / travelled;
                    windows += 1.0;
                    nets.push(net);
                    windows_bad += (net < 0.2) as i32 as f64;
                }
            }
        }
        if let Some((a, p)) = anchor
            && end - a >= 30
        {
            if std::env::var_os("MEASURE_STOPS").is_some() {
                eprintln!("stop p{k} t{} {} s at {:?}", a, end - a, p.map(|x| x as i32));
            }
            stops += 1.0;
            stop_max = stop_max.max((end - a) as f64);
        }
    }
    r.put("19 stops >=30s", stops);
    if stops > 0.0 {
        r.put("20 longest stop", stop_max);
    }
    r.put("windows net<0.2", windows_bad / f64::max(windows, 1.0));
    r.put("windows net median", q(&mut nets, 0.5));

    // ---- reaction: an enemy enters 55 m after ≥10 s out
    let (mut reactions, mut reactions_none, mut reactions_n): (Vec<f64>, f64, f64) = (Vec::new(), 0.0, 0.0);
    for k in 0..n {
        let mut outside = 0;
        for i in 0..end {
            let m = &history[i][k];
            if !m.alive {
                outside = 0;
                continue;
            }
            let e = enemy_dist(&history[i], k);
            if e > 55.0 {
                outside += 1;
                continue;
            }
            if outside >= 10 && end - i >= 20 && m.veh.is_none() {
                reactions_n += 1.0;
                let t = i as f64 + 1.0;
                match shots.iter().filter(|x| x.1 == k && x.0 >= t && x.0 < t + 20.0).map(|x| x.0 - t).fold(f64::MAX, f64::min) {
                    x if x == f64::MAX => reactions_none += 1.0,
                    x => reactions.push(x),
                }
            }
            outside = 0;
        }
    }
    r.put("reaction median", q(&mut reactions, 0.5));
    r.put("reaction no shot in 20 s", reactions_none / f64::max(reactions_n, 1.0));

    // ---- rhythm per weapon
    let mut per_weapon: BTreeMap<(usize, String), Vec<f64>> = BTreeMap::new();
    for x in &shots {
        per_weapon.entry((x.1, x.3.clone())).or_default().push(x.0);
    }
    let mut results: BTreeMap<String, (f64, Vec<f64>, Vec<f64>)> = BTreeMap::new();
    for ((_, w), t) in per_weapon {
        let e = results.entry(w).or_default();
        e.0 += t.len() as f64;
        let mut burst = 1.0;
        for ab in t.windows(2) {
            let dt = ab[1] - ab[0];
            if dt < 0.3 {
                burst += 1.0;
            } else {
                e.1.push(burst);
                burst = 1.0;
                e.2.push(dt);
            }
        }
        e.1.push(burst);
    }
    for (w, (nn, mut ra, mut iv)) in results {
        r.put(&format!("weapon {w} n"), nn);
        r.put(&format!("weapon {w} burst med"), q(&mut ra, 0.5));
        r.put(&format!("weapon {w} burst p90"), q(&mut ra, 0.9));
        r.put(&format!("weapon {w} interval min"), iv.iter().copied().fold(f64::MAX, f64::min).min(999.0));
        r.put(&format!("weapon {w} interval med"), q(&mut iv, 0.5));
    }
    let mut gr: Vec<f64> = shots.iter().filter(|x| x.3.to_ascii_lowercase().contains("grenade")).map(|x| x.4).collect();
    r.put("grenades n", gr.len() as f64);
    r.put("grenades dist med", q(&mut gr, 0.5));

    // ---- Berlin's flags
    if level.eq_ignore_ascii_case("berlin") && flags.len() == 4 {
        let (b1, b3, b4) = (0usize, 2usize, 3usize);
        let (mut alive2, mut alive1) = (0.0, 0.0);
        let (mut e2b, mut e1b) = ([0.0; 4], [0.0; 4]);
        let mut near_b2 = 0.0;
        for i in from..end {
            for x in &history[i] {
                if !x.alive {
                    continue;
                }
                let (mut bj, mut bd) = (0, f64::MAX);
                for (j, f) in flags.iter().enumerate() {
                    let d = d2(*f, x.pos);
                    if d < bd {
                        (bj, bd) = (j, d);
                    }
                }
                if x.team == 2 {
                    alive2 += 1.0;
                    if bd < 60.0 {
                        e2b[bj] += 1.0;
                    }
                } else {
                    alive1 += 1.0;
                    if bd < 60.0 {
                        e1b[bj] += 1.0;
                    }
                    if d2(flags[1], x.pos) < 60.0 {
                        near_b2 += 1.0;
                    }
                }
            }
        }
        r.put("17 eq2 B3", e2b[b3] / f64::max(alive2, 1.0));
        r.put("eq2 B1", e2b[b1] / f64::max(alive2, 1.0));
        r.put("eq2 B4", e2b[b4] / f64::max(alive2, 1.0));
        r.put("18 eq1 B3", e1b[b3] / f64::max(alive1, 1.0));
        r.put("18 eq1 B1", e1b[b1] / f64::max(alive1, 1.0));
        r.put("18 eq1 B4", e1b[b4] / f64::max(alive1, 1.0));
        r.put("eq1 near B2", near_b2 / f64::max(alive1, 1.0));
        let first_capture = |j: usize, t: u32| flags_owner.iter().position(|b| b[j] == t).map(tr);
        r.put("B3 taken", first_capture(b3, 2).is_some() as i32 as f64);
        if let Some(t) = first_capture(b3, 2) {
            r.put("B3 first capture t", t);
        }
        r.put("B3 taken at end", (flags_owner.last().map(|b| b[b3]) == Some(2)) as i32 as f64);
        r.put("B1 taken", first_capture(b1, 2).is_some() as i32 as f64);
        r.put("B4 taken", first_capture(b4, 2).is_some() as i32 as f64);
    }
    let tot = &report["totals"];
    for k in [
        "friendly hits per 100 enemy hp",
        "circling %",
        "stuck foot %",
        "still foot %",
        "still mounted %",
        "blocked % of shots",
        "spray % of shots",
        "ignored flag %",
        "flags taken",
        "short rides % of rides",
    ] {
        r.put(&format!("meter: {k}"), tot[k].as_f64().unwrap_or(f64::NAN));
    }
    if let Some(c) = s.modes.conquest.as_ref() {
        r.put("tickets 1", c.tickets[0] as f64);
        r.put("tickets 2", c.tickets[1] as f64);
    }
    r
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let (jug, seg, rounds): (usize, f64, usize) = (a[4].parse().unwrap(), a[5].parse().unwrap(), a[6].parse().unwrap());
    let mind = opt("--mind");
    let both = a.iter().any(|x| x == "--both");
    let seed0: u64 = opt("--seed0").map_or(0, |n| n.parse().unwrap());
    let w = World::load(Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let w = &w;
    let a3 = &a[3];
    let rs: Vec<Round> = std::thread::scope(|sc| {
        let threads: Vec<_> = (0..rounds).map(|i| sc.spawn(move || round(w, jug, seg, mind, both, seed0 + i as u64 + 1, a3))).collect();
        threads.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut keys: Vec<&String> = rs.iter().flat_map(|r| r.0.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut output = serde_json::Map::new();
    for k in keys {
        let mut v: Vec<f64> = rs.iter().filter_map(|r| r.0.get(k).copied()).filter(|x| !x.is_nan()).collect();
        if v.is_empty() {
            continue;
        }
        let (mn, md, mx) = (q(&mut v, 0.0), q(&mut v, 0.5), q(&mut v, 1.0));
        println!("{k:40} {md:9.2}  [{mn:.2}–{mx:.2}] n={}", v.len());
        output.insert(k.clone(), serde_json::json!({"median": md, "min": mn, "max": mx}));
    }
    if let Some(o) = opt("--out") {
        std::fs::write(o, serde_json::to_string_pretty(&output).unwrap()).unwrap();
    }
}
