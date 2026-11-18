//! What should never be true of a world, whatever anyone does (the detector of `examples/bots`, `BOTS_CHECK`,
//! and of `crates/server/examples/monkey.rs`). Not the original's: ours, for finding what is missing or wrong.
use crate::Sim;

/// One thing wrong: its kind, the object's template and where it is.
pub type Odd = (&'static str, String, [i64; 3]);

/// Everything wrong with `s` now.
pub fn scan(s: &Sim) -> Vec<Odd> {
    let mut out: Vec<Odd> = Vec::new();
    let w = s.world;
    for (k, v) in s.vehicles.iter().enumerate() {
        let b = &v.craft.body;
        let (q, u) = (b.pos, b.vel);
        let at = q.map(|x| if x.is_finite() { x as i64 } else { i64::MAX });
        if v.in_world() {
            let speed = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
            let finite = q.iter().chain(u.iter()).all(|x| x.is_finite());
            let ground = if finite { w.height_at(q[0] as f64, q[2] as f64) } else { None };
            let kind = if !finite {
                Some("not finite")
            } else if speed > 700.0 {
                Some("over 700 m/s")
            } else if ground.is_none() {
                Some("off the map")
            } else if !v.fixed && b.underwater <= 0.0 && (q[1] as f64) < ground.unwrap() - 3.0 {
                Some("under the terrain")
            } else if v.armor.as_ref().is_some_and(|a| a.hp > a.max_hp + 0.01) {
                Some("health over its maximum")
            } else {
                None
            };
            if let Some(kind) = kind {
                out.push((kind, v.template.clone(), at));
            }
            if v.armor.as_ref().is_some_and(|a| !a.hp.is_finite()) {
                out.push(("health not finite", v.template.clone(), at));
            }
            for (_, f) in &v.weapons {
                // (-1: unlimited)
                if f.mags.iter().any(|&m| m < -1) {
                    out.push(("negative magazine", f.template.name.clone(), at));
                }
                if !f.dev.is_finite() || !f.overheat.is_finite() {
                    out.push(("weapon state not finite", f.template.name.clone(), at));
                }
            }
        }
        for (seat, o) in v.entry.occupants.iter().enumerate() {
            let Some(o) = *o else { continue };
            match s.vehicles.get(o) {
                None => out.push(("occupant does not exist", v.template.clone(), at)),
                Some(m) if m.entry.inside != Some(k) => out.push(("occupant not inside the vehicle", format!("{} in {}", m.template, v.template), at)),
                Some(m) if m.entry.position != seat => out.push(("occupant in another seat", format!("{} in {}", m.template, v.template), at)),
                _ => {}
            }
            // not when the occupant went with it: `clear_world` at the round's end removes them all at once (DC_Basrahs_Edge
            // at 679 s gave three of these)
            if !v.in_world() && s.vehicles.get(o).is_some_and(|m| !m.removed) {
                let who = s.vehicles.get(o).map_or("?", |m| m.template.as_str());
                let alive = s.players.iter().any(|p| p.alive && p.soldier == Some(o));
                let wreck = v.armor.as_ref().is_some_and(|a| a.destroyed);
                out.push(("occupant of a vehicle out of the world", format!("{who} (alive {alive}) in {} (removed {}, destroyed {wreck})", v.template, v.removed), at));
            }
        }
        if let Some(i) = v.entry.inside {
            match s.vehicles.get(i) {
                None => out.push(("inside a vehicle that does not exist", v.template.clone(), at)),
                Some(m) if m.entry.occupants.get(v.entry.position).copied().flatten() != Some(k) => out.push(("inside but not its occupant", format!("{} in {}", v.template, m.template), at)),
                _ => {}
            }
        }
    }
    let mut bodies = std::collections::BTreeMap::new();
    for p in &s.players {
        let Some(i) = p.soldier else {
            if p.alive {
                out.push(("alive with no soldier", p.template.clone(), [0; 3]));
            }
            continue;
        };
        let Some(v) = s.vehicles.get(i) else {
            out.push(("soldier does not exist", p.template.clone(), [0; 3]));
            continue;
        };
        // (one inside a vehicle is out of the world: the entry's enter)
        if p.alive && !v.in_world() && v.entry.inside.is_none() {
            out.push(("alive out of the world", p.template.clone(), v.craft.body.pos.map(|x| x as i64)));
        }
        if p.alive && *bodies.entry(i).and_modify(|n| *n += 1).or_insert(1) > 1 {
            out.push(("two players with one soldier", p.template.clone(), v.craft.body.pos.map(|x| x as i64)));
        }
        if !p.spawn_time.is_finite() {
            out.push(("spawn time not finite", p.template.clone(), [0; 3]));
        }
    }
    out
}
