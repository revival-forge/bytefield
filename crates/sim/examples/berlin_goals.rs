//! The Berlin goals of the specification (§2), measured from outside, at 1 Hz and without the first 25 s except where not applicable.
//!   berlin_goals <game> <seed> [seconds]
//! One JSON line per round. With several seeds: `berlin_goals data/game 1`, `... 2`, and the median of each field.
use serde_json::json;
use std::collections::BTreeMap;

fn d3(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>().sqrt()
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed: u64 = a[2].parse().unwrap();
    let seconds: u64 = a.get(3).map_or(240, |x| x.parse().unwrap());
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), "bf1942", "Berlin", "conquest").expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.rng = bytefield_sim::players::Rng(s.rng.0 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    for t in [1, 2] {
        s.set_minds(t, bytefield_sim::bots2::maker("Berlin", seed));
    }
    for i in 0..32 {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    // who owns each soldier
    let owners = |s: &bytefield_sim::Sim| -> BTreeMap<usize, usize> { s.players.iter().enumerate().filter_map(|(k, p)| p.soldier.map(|i| (i, k))).collect() };
    let flags: Vec<[f32; 3]> = s.modes.conquest.as_ref().unwrap().points.iter().map(|p| p.pos).collect();
    let (b1, b3, b4) = (0, 2, 3);
    let n = s.players.len();
    let (mut shots, mut on_foot, mut shot_dist, mut over_60) = (0u32, 0u32, Vec::new(), 0u32);
    let mut per_bot = vec![0u32; n];
    let mut first: Option<f32> = None;
    let mut first_per_bot: Vec<Option<f32>> = vec![None; n];
    let mut seen = 0;
    let (mut on_foot_40_s, mut on_foot_40_shooting_s) = (0u32, 0u32);
    let mut shot_this_second = vec![false; n];
    let (mut alive_s, mut in_vehicle_s) = (0u32, 0u32);
    let mut near = [[0u32; 4]; 2];
    let mut on_foot_samples = [0u32; 2];
    let (mut lives, mut lives_boarding) = (0u32, 0u32);
    let mut life: Vec<Option<(f32, bool)>> = vec![None; n];
    let mut lifetimes = Vec::new();
    let mut deaths_before = vec![0u32; n];
    let (mut first_death, mut deaths) = (None, 0u32);
    let mut b3_taken_at: Option<f32> = None;
    let mut b4_taken = false;
    let (mut still_samples, mut calm_samples, mut mate_15, mut on_foot_s) = (0u32, 0u32, 0u32, 0u32);
    let mut mate_dist = Vec::new();
    let mut calm_speed = Vec::new();
    let mut still_since: Vec<Option<(f32, [f32; 3])>> = vec![None; n];
    let (mut stops_30, mut stop_max) = (0u32, 0f32);
    for step in 0..seconds * 30 {
        s.step();
        let t = (step + 1) as f32 / 30.0;
        let owner_of = owners(&s);
        let alive_pos: Vec<Option<[f32; 3]>> = s
            .players
            .iter()
            .map(|p| {
                p.soldier
                    .filter(|_| p.alive)
                    .map(|i| s.vehicles[i].entry.inside.map_or(s.vehicles[i].craft.body.pos, |c| s.vehicles[c].craft.body.pos))
            })
            .collect();
        let enemy_dist = |k: usize, pos: [f32; 3]| (0..n).filter(|&j| s.players[j].team != s.players[k].team).filter_map(|j| alive_pos[j].map(|q| d3(pos, q))).fold(f32::MAX, f32::min);
        for f in s.journal.fired.get(seen..).unwrap_or(&[]) {
            let Some(k) = owner_of
                .get(&f.1)
                .copied()
                .or_else(|| s.players.iter().position(|p| p.soldier.is_some_and(|i| s.vehicles[i].entry.inside == Some(f.1))))
            else {
                continue;
            };
            let Some(pos) = alive_pos[k] else { continue };
            shots += 1;
            per_bot[k] += 1;
            first.get_or_insert(t);
            first_per_bot[k].get_or_insert(t);
            shot_this_second[k] = true;
            if s.vehicles[f.1].is_soldier() {
                on_foot += 1;
            }
            let d = enemy_dist(k, pos);
            if d < f32::MAX {
                shot_dist.push(d);
                if d > 60.0 {
                    over_60 += 1;
                }
            }
        }
        seen = s.journal.fired.len();
        for k in 0..n {
            let p = &s.players[k];
            if p.deaths > deaths_before[k] {
                deaths += p.deaths - deaths_before[k];
                deaths_before[k] = p.deaths;
                first_death.get_or_insert(t);
                if let Some((born, boarded)) = life[k].take() {
                    lifetimes.push(t - born);
                    lives += 1;
                    lives_boarding += boarded as u32;
                }
            }
            if let Some(i) = p.soldier.filter(|_| p.alive) {
                let inside = s.vehicles[i].entry.inside.is_some();
                let e = life[k].get_or_insert((t, false));
                e.1 |= inside;
            }
        }
        if let Some(c) = s.modes.conquest.as_ref() {
            if b3_taken_at.is_none() && c.points[b3].team == 2 {
                b3_taken_at = Some(t);
            }
            b4_taken |= c.points[b4].team == 2;
        }
        if (step + 1) % 30 != 0 || t <= 25.0 {
            continue;
        }
        for k in 0..n {
            let p = &s.players[k];
            let shooting = std::mem::take(&mut shot_this_second[k]);
            let Some(pos) = alive_pos[k] else {
                still_since[k] = None;
                continue;
            };
            alive_s += 1;
            let i = p.soldier.unwrap();
            let inside = s.vehicles[i].entry.inside.is_some();
            if inside {
                in_vehicle_s += 1;
                still_since[k] = None;
                continue;
            }
            let eq = (p.team as usize).saturating_sub(1).min(1);
            on_foot_samples[eq] += 1;
            // near which, being the nearest
            let nearest = (0..flags.len()).min_by(|&x, &y| d3(pos, flags[x]).total_cmp(&d3(pos, flags[y])));
            if let Some(b) = nearest
                && d3(pos, flags[b]) < 60.0
            {
                near[eq][b] += 1;
            }
            let e = enemy_dist(k, pos);
            on_foot_s += 1;
            if e < 40.0 {
                on_foot_40_s += 1;
                on_foot_40_shooting_s += shooting as u32;
            }
            let vel = s.vehicles[i].craft.body.vel;
            let v = vel[0].hypot(vel[2]);
            let calm = e >= 55.0;
            if calm {
                calm_samples += 1;
                if v < 0.5 {
                    still_samples += 1;
                } else {
                    calm_speed.push(v);
                }
            }
            let c = (0..n)
                .filter(|&j| j != k && s.players[j].team == p.team)
                .filter_map(|j| alive_pos[j].filter(|_| s.players[j].soldier.is_some_and(|q| s.vehicles[q].entry.inside.is_none())).map(|q| d3(pos, q)))
                .fold(f32::MAX, f32::min);
            if c < f32::MAX {
                mate_dist.push(c);
                mate_15 += (c < 15.0) as u32;
            }
            // stops of ≥30 s
            match still_since[k] {
                Some((t0, anchor)) if d3(pos, anchor) < 2.0 => {
                    let dur = t - t0;
                    if dur >= 30.0 && dur < 31.0 {
                        stops_30 += 1;
                    }
                    if dur >= 30.0 {
                        stop_max = stop_max.max(dur);
                    }
                }
                _ => still_since[k] = Some((t, pos)),
            }
        }
    }
    let total_deaths = deaths;
    let mut dt = shot_dist.clone();
    let med_d = median(&mut dt);
    let p90 = if dt.is_empty() { 0.0 } else { dt[dt.len() * 9 / 10] };
    let mut pb: Vec<f32> = per_bot.iter().map(|&x| x as f32).collect();
    let mut pb1: Vec<f32> = first_per_bot.iter().flatten().copied().collect();
    let dur = median(&mut lifetimes);
    let frac = |a: u32, b: u32| if b > 0 { a as f32 / b as f32 } else { 0.0 };
    let on_foot_share = |e: usize, b: usize| frac(near[e][b], on_foot_samples[e]);
    println!(
        "{}",
        json!({"seed": seed, "f1_shots": shots, "f2_per_bot": median(&mut pb), "f3_bots": per_bot.iter().filter(|&&x| x > 0).count(), "f4_on_foot": frac(on_foot, shots),
            "f5_dist": med_d, "f6_p90": p90, "f7_shooting_40": frac(on_foot_40_shooting_s, on_foot_40_s), "f8_first": first, "f9_first_per_bot": median(&mut pb1), "f10_per_death": frac(shots, total_deaths),
            "f11_first_death": first_death, "f12_deaths": total_deaths, "f13_mmj": total_deaths as f32 / (seconds as f32 / 60.0) / 32.0, "f14_life": dur, "f15_vehicle": frac(in_vehicle_s, alive_s), "f16_board": frac(lives_boarding, lives),
            "f17_e2_b3": on_foot_share(1, b3), "f18_e1_b3": on_foot_share(0, b3), "f18_e1_b1": on_foot_share(0, b1), "f18_e1_b4": on_foot_share(0, b4), "f19_stops": stops_30, "f20_stop_max": stop_max,
            "lim_over_60": over_60, "lim_b3": b3_taken_at, "lim_b4": b4_taken, "calm_speed": median(&mut calm_speed), "still_calm": frac(still_samples, calm_samples), "mate": median(&mut mate_dist), "mate_15": frac(mate_15, on_foot_s)})
    );
}
