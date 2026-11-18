//! Where a template's bounding radius comes from: `World::bounding_radius` and the path that sets it, one line per
//! node whose reach is within 0.5 m of the top (or `MARGIN`). `cargo run --release -p bytefield-sim --example radius_tree -- data/game DC_Final DC_Basrahs_Edge BMP2 M2A3`
fn walk(w: &bytefield_sim::World, name: &str, base: f64, depth: usize, out: &mut Vec<(f64, usize, String, String, f64)>) {
    let Some(t) = w.db.object_template(name) else { out.push((base, depth, name.to_string(), "NO TEMPLATE".into(), 0.0)); return };
    if depth > 16 {
        return;
    }
    if ["effectbundle", "emitter"].iter().any(|k| t.kind.eq_ignore_ascii_case(k)) {
        // what `bounding_radius` leaves out, listed with the reach it would have
        out.push((base, depth, format!("EFFECT {} ({}) ttl={:?}", t.name, t.kind, t.get("timetolive")), String::new(), 0.0));
        for (child, local) in bytefield_world::scene::children(t) {
            let p = local.0[3];
            walk(w, &child, base + (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt(), depth + 1, out);
        }
        return;
    }
    let geometry = t.get("geometry").and_then(|a| a.first()).cloned().unwrap_or_default();
    let r = (!geometry.is_empty()).then(|| w.mesh(&geometry)).flatten().map_or(0.0, |m| {
        let c = |i: usize| m.bounds[0][i].abs().max(m.bounds[1][i].abs()) as f64;
        (c(0) * c(0) + c(1) * c(1) + c(2) * c(2)).sqrt()
    });
    out.push((base + r, depth, format!("{} ({})", t.name, t.kind), geometry, r));
    for (d, at) in shown(t) {
        walk(w, d, base + at, depth + 1, out);
    }
    for (child, local) in bytefield_world::scene::children(t) {
        let p = local.0[3];
        walk(w, &child, base + (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt(), depth + 1, out);
    }
}

/// (reach, under a `FireArms`?, name) of every effect in the tree
fn effects(w: &bytefield_sim::World, name: &str, base: f64, depth: usize, firearms: bool, out: &mut Vec<(f64, bool, String)>) {
    let Some(t) = w.db.object_template(name) else { return };
    if depth > 16 {
        return;
    }
    let is_effect = ["effectbundle", "emitter"].iter().any(|k| t.kind.eq_ignore_ascii_case(k));
    if is_effect {
        out.push((base, firearms, t.name.clone()));
    }
    let firearms = firearms || t.kind.eq_ignore_ascii_case("firearms");
    for (child, local) in bytefield_world::scene::children(t) {
        let p = local.0[3];
        effects(w, &child, base + (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt(), depth + 1, firearms, out);
    }
}

/// `--census`: the vehicles where an effect reaches beyond `bounding_radius`: which case tells the rules apart
fn census(w: &bytefield_sim::World) {
    let mut names: Vec<&String> = w.db.templates.iter().filter(|((ns, _), t)| ns == "objecttemplate" && t.kind.eq_ignore_ascii_case("playercontrolobject")).map(|(_, t)| &t.name).collect();
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names.dedup_by_key(|n| n.to_ascii_lowercase());
    for n in names {
        let r = w.bounding_radius(n) as f64;
        let mut out = Vec::new();
        effects(w, n, 0.0, 0, false, &mut out);
        let free = out.iter().filter(|x| !x.1).max_by(|a, b| a.0.total_cmp(&b.0));
        let fire = out.iter().filter(|x| x.1).max_by(|a, b| a.0.total_cmp(&b.0));
        if r > 0.0 && (free.is_some_and(|x| x.0 > r + 0.01) || fire.is_some_and(|x| x.0 > r + 0.01)) {
            println!("{n}\t{r:.3}\tfree {:.3} {}\tfirearms {:.3} {}", free.map_or(0.0, |x| x.0), free.map_or("", |x| &x.2), fire.map_or(0.0, |x| x.0), fire.map_or("", |x| &x.2));
        }
    }
}

/// What a FireArms shows and how far (as `World::bounding_radius`): the dummy missile at its farthest `addFireArmsPosition`,
/// the muzzle effect (`visibleBarrelTemplate`) at the first one.
fn shown(t: &bytefield_con::templates::Template) -> Vec<(&str, f64)> {
    let at: Vec<f64> = t.all("addfirearmsposition").iter().filter_map(|a| bytefield_world::parse_vec(a.first()?).filter(|v| v.len() == 3)).map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()).collect();
    let mut out = Vec::new();
    if let Some(d) = t.get("visibledummyprojectiletemplate").and_then(|a| a.first()) {
        out.push((d.trim(), at.iter().copied().fold(0.0, f64::max)));
    }
    if let Some(d) = t.get("visiblebarreltemplate").and_then(|a| a.first()) {
        out.push((d.trim(), at.first().copied().unwrap_or(0.0)));
    }
    out
}

/// A candidate rule for the server's radius, to try against the original's census:
/// `NOGEOM` (kinds whose own geometry does not count, comma-separated, e.g. "bundle,animatedbundle"), `LOD`
/// ("all": every child; "pick": the child the selector picks: `DistCompareSelector`/`CompareSelector` the first, `DistanceSelector`
/// `LOD_DIST` ("first" or "last"), and the rest every child). Prints each template whose rule misses the original by over 1 mm.
fn rule(w: &bytefield_sim::World, name: &str, depth: usize, firearms: bool) -> f64 {
    let Some(t) = w.db.object_template(name) else { return 0.0 };
    if depth > 16 || (firearms && ["effectbundle", "emitter"].iter().any(|k| t.kind.eq_ignore_ascii_case(k))) {
        return 0.0;
    }
    let firearms = firearms || t.kind.eq_ignore_ascii_case("firearms");
    let nogeom = std::env::var("NOGEOM").unwrap_or_default();
    let own = !nogeom.split(',').any(|k| !k.is_empty() && t.kind.eq_ignore_ascii_case(k));
    let mut r = if own {
        t.get("geometry").and_then(|a| a.first()).and_then(|g| w.mesh(g)).map_or(0.0, |m| {
            if std::env::var("MESH").as_deref() == Ok("sphere") {
                // the box's centre plus half its diagonal
                let c = |i: usize| ((m.bounds[0][i] + m.bounds[1][i]) / 2.0) as f64;
                let h = |i: usize| ((m.bounds[1][i] - m.bounds[0][i]) / 2.0) as f64;
                return (c(0) * c(0) + c(1) * c(1) + c(2) * c(2)).sqrt() + (h(0) * h(0) + h(1) * h(1) + h(2) * h(2)).sqrt();
            }
            let c = |i: usize| m.bounds[0][i].abs().max(m.bounds[1][i].abs()) as f64;
            (c(0) * c(0) + c(1) * c(1) + c(2) * c(2)).sqrt()
        })
    } else {
        0.0
    };
    for (d, at) in shown(t) {
        r = r.max(at + rule(w, d, depth + 1, firearms));
    }
    let mut children = bytefield_world::scene::children(t);
    if t.kind.eq_ignore_ascii_case("lodobject") && std::env::var("LOD").as_deref() == Ok("pick") && !children.is_empty() {
        let sel = t.get("lodselector").and_then(|a| a.first()).and_then(|s| w.db.get("lodselectortemplate", s)).map(|s| s.kind.to_ascii_lowercase()).unwrap_or_default();
        let last = std::env::var("LOD_DIST").as_deref() == Ok("last");
        children = match sel.as_str() {
            "distcompareselector" | "distcompareselector2" | "compareselector" => vec![children.swap_remove(0)],
            "distanceselector" if last => vec![children.pop().unwrap()],
            "distanceselector" => vec![children.swap_remove(0)],
            "" if std::env::var("LOD_NONE").as_deref() == Ok("first") => vec![children.swap_remove(0)],
            _ => children,
        };
    }
    for (child, local) in children {
        let p = local.0[3];
        r = r.max((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() + rule(w, &child, depth + 1, firearms));
    }
    r
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let margin: f64 = std::env::var("MARGIN").ok().and_then(|m| m.parse().ok()).unwrap_or(0.5);
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    // --rule: the candidate rule of this example; --engine: `World::bounding_radius`, the one the game uses
    if a.get(4).is_some_and(|x| x == "--rule" || x == "--engine") {
        let engine = a[4] == "--engine";
        let (mut ok, mut bad) = (0, 0);
        for l in std::fs::read_to_string(&a[5]).expect("census").lines() {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (Some(n), Some(Ok(o))) = (f.first(), f.get(1).map(|x| x.parse::<f64>())) else { continue };
            let r = if engine { w.bounding_radius(n) as f64 } else { rule(&w, n, 0, false) };
            if (r - o).abs() > 0.001 {
                bad += 1;
                println!("{n:20} original {o:8.3} rule {r:8.3}");
            } else {
                ok += 1;
            }
        }
        println!("{ok} match, {bad} miss");
        return;
    }
    if a.get(4).is_some_and(|x| x == "--all") {
        // every PlayerControlObject and its radius, to diff two versions of the rule
        let mut names: Vec<&String> = w.db.templates.iter().filter(|((ns, _), t)| ns == "objecttemplate" && t.kind.eq_ignore_ascii_case("playercontrolobject")).map(|(_, t)| &t.name).collect();
        names.sort_by_key(|n| n.to_ascii_lowercase());
        names.dedup_by_key(|n| n.to_ascii_lowercase());
        for n in names {
            println!("{n} {:.3}", w.bounding_radius(n));
        }
        return;
    }
    if a.get(4).is_some_and(|x| x == "--census") {
        census(&w);
        return;
    }
    for name in &a[4..] {
        let mut out = Vec::new();
        walk(&w, name, 0.0, 0, &mut out);
        let top = out.iter().filter(|x| !x.2.starts_with("EFFECT")).map(|x| x.0).fold(0.0, f64::max);
        println!("{name}: bounding_radius {:.3} x1.2 = {:.2}", w.bounding_radius(name), w.bounding_radius(name) * 1.2);
        for (reach, depth, node, geometry, r) in out.iter().filter(|x| x.0 > top - margin || x.3 == "NO TEMPLATE" || x.2.starts_with("EFFECT")) {
            println!("  {reach:7.3} {:depth$}{node} geometry={geometry} r={r:.3}", "");
        }
    }
}
