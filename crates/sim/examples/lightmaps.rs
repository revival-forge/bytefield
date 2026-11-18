//! Which placed statics have their own lightmap (`<level>/ObjectLightmaps/<geometry>_<x>-<y>-<z>.tga`,
//! `scene::Draw::lightmap`): how many of the level's TGAs the draws find, how many are a
//! destroyed geometry's (`_m2`, not drawn) and the others nothing finds (left over in the level's archive).
//! `cargo run --release -p bytefield-sim --example lightmaps -- data/game bf1942 El_Alamein`
//! With `all` for the level: per level of the mod, the TGAs no reader takes and the grey ones (type 3/11).
use std::collections::BTreeSet;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a[3] == "all" {
        let (_, vfs) = bytefield_con::session::mount(std::path::Path::new(&a[1]), &a[2]).expect("mount");
        let mut by: std::collections::BTreeMap<String, (usize, usize, usize)> = Default::default();
        for p in vfs.list() {
            let n = bytefield_vfs::normalize(p);
            let Some((dir, _)) = n.split_once("/objectlightmaps/").filter(|_| n.ends_with(".tga")) else { continue };
            let Ok(b) = vfs.read(p) else { continue };
            let e = by.entry(dir.to_string()).or_default();
            e.0 += 1;
            e.1 += (bytefield_formats::tga::indexed(&b).is_err() && bytefield_formats::tga::decode(&b).is_err()) as usize;
            e.2 += (b.len() > 2 && matches!(b[2], 3 | 11)) as usize;
        }
        for (dir, (n, unread, grey)) in by.iter().filter(|(_, v)| v.1 + v.2 > 0) {
            println!("{dir}: {n} TGAs, {unread} unread, {grey} grey");
        }
        return;
    }
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let dir = w.level.terrain.heightmap.rsplit_once('/').map_or("", |d| d.0).to_ascii_lowercase();
    let prefix = format!("{dir}/objectlightmaps/");
    let tgas: BTreeSet<String> = w.vfs.list().iter().map(|p| p.to_ascii_lowercase()).filter_map(|p| Some(p.strip_prefix(&prefix)?.strip_suffix(".tga")?.to_string())).collect();
    let exp = bytefield_world::scene::expand_level(&w.db);
    let found: BTreeSet<String> = exp.draws.iter().map(|d| d.lightmap()).filter(|k| tgas.contains(k)).collect();
    let (m2, others): (Vec<&String>, Vec<&String>) = tgas.iter().filter(|t| !found.contains(*t)).partition(|t| t.contains("_m2_"));
    // the placed pieces with no map of their own, by geometry: the ones the rule «Sin mapa» draws
    let mut bare: std::collections::BTreeMap<String, usize> = Default::default();
    for k in exp.draws.iter().map(|d| d.lightmap()).filter(|k| !tgas.contains(k)) {
        *bare.entry(k.rsplit_once('_').map_or(k.as_str(), |x| x.0).to_string()).or_default() += 1;
    }
    let mut by: Vec<(usize, &String)> = bare.iter().map(|(g, n)| (*n, g)).collect();
    by.sort_by(|a, b| b.cmp(a));
    println!("{} of {} draws with no map; most: {:?}", by.iter().map(|x| x.0).sum::<usize>(), exp.draws.len(), &by[..by.len().min(12)]);
    // NEAREST=1: the draw nearest each one nothing finds
    if std::env::var("NEAREST").is_ok() {
        for t in &others {
            let pos: Vec<f64> = t.rsplit('_').next().unwrap_or("").split('-').filter_map(|x| x.parse().ok()).collect();
            let d2 = |d: &bytefield_world::scene::Draw| pos.iter().enumerate().map(|(i, p)| (d.world.0[3][i] - p).powi(2)).sum::<f64>();
            if let (3, Some(d)) = (pos.len(), exp.draws.iter().min_by(|a, b| d2(a).total_cmp(&d2(b)))) {
                println!("{t}: nearest {} ({}) at {:.1} m", d.lightmap(), w.db.instances[d.instance].template, d2(d).sqrt());
            }
        }
    }
    // NEAR=x,z,r: each draw within r metres of (x, z), its kind, key and whether that key has a TGA
    if let Ok(near) = std::env::var("NEAR") {
        let v: Vec<f64> = near.split(',').filter_map(|x| x.parse().ok()).collect();
        for d in exp.draws.iter().filter(|d| (d.world.0[3][0] - v[0]).hypot(d.world.0[3][2] - v[1]) < v[2]) {
            let p = d.world.0[3];
            let has = if tgas.contains(&d.lightmap()) { "map" } else { "-" };
            println!("{:>4} {:7.1} {:5.1} {:7.1} {:12} {:4} {} ({})", has, p[0], p[1], p[2], d.kind, d.mesh, d.lightmap(), w.db.instances[d.instance].template);
        }
    }
    // the TGAs neither reader takes (GC_Podrace's grey ones were, and the client drew the stand-in for them)
    let unread: Vec<&String> = tgas.iter().filter(|t| w.vfs.read(&format!("{prefix}{t}.tga")).is_ok_and(|b| bytefield_formats::tga::indexed(&b).is_err() && bytefield_formats::tga::decode(&b).is_err())).collect();
    let grey = tgas.iter().filter(|t| w.vfs.read(&format!("{prefix}{t}.tga")).is_ok_and(|b| b.len() > 2 && matches!(b[2], 3 | 11))).count();
    println!("{} TGAs unread: {:?}; {grey} grey", unread.len(), unread.iter().take(6).collect::<Vec<_>>());
    println!("{} TGAs: {} found, {} a destroyed geometry's (_m2), {} nothing finds: {:?}", tgas.len(), found.len(), m2.len(), others.len(), others.iter().take(12).collect::<Vec<_>>());
}
