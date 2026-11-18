//! The level's spawners whose `Object.setTeam` and the team of their `setOSId` control point
//! disagree:
//! `cargo run --release -p bytefield-sim --example spawner_teams -- <game> <mod> <level>...`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    for level in &a[3..] {
        let Ok(w) = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], level, "conquest") else {
            println!("{level}: does not load");
            continue;
        };
        let db = &w.db;
        let last = |i: &bytefield_con::templates::Instance, n: &str| i.props.iter().rev().find(|p| p.name == n).and_then(|p| p.args.first()).cloned();
        let arg = |t: &bytefield_con::templates::Template, n: &str| t.get(n).and_then(|a| a.first()).cloned();
        let mut flags = Vec::new();
        for i in &db.instances {
            if let Some(t) = db.object_template(&i.template).filter(|t| t.kind.eq_ignore_ascii_case("controlpoint")) {
                flags.push((t.name.clone(), arg(t, "objectSpawnerId"), arg(t, "team"), last(i, "setteam")));
            }
        }
        let (mut n, mut both, mut bad) = (0, 0, 0);
        for i in &db.instances {
            let Some(t) = db.object_template(&i.template).filter(|t| t.kind.eq_ignore_ascii_case("objectspawner")) else { continue };
            n += 1;
            let (Some(st), Some(os)) = (last(i, "setteam"), last(i, "setosid")) else { continue };
            both += 1;
            let f = flags.iter().find(|f| f.1.as_deref() == Some(os.as_str()));
            let ft = f.and_then(|f| f.2.clone()).unwrap_or_default();
            if ft != st {
                bad += 1;
                println!("{level}: {} at {:?} setTeam {st} setOSId {os} flag {:?}", t.name, last(i, "absoluteposition"), f);
            }
        }
        println!("{level}: {n} spawners, {both} with setTeam and setOSId, {bad} disagree");
    }
}
