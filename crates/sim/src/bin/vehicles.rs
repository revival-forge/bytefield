//! `vehicles <game_dir> <mod> [--show <template>] [--json out.json]`: every PlayerControlObject
//! the mod defines, with a physics summary; `--show` prints one vehicle's component tree.

use std::path::Path;
use std::process::ExitCode;

use bytefield_con::session;
use bytefield_sim::vehicle::{self, Component, Value};

fn print(c: &Component, depth: usize) {
    let pad = "  ".repeat(depth);
    let pick = ["mass", "drag", "inertiamodifier", "enginetype", "settorque", "setdifferential", "setmaxspeed", "setacceleration", "setwinglift", "setflaplift", "setinputtopitch", "setinputtoyaw", "setinputtoroll", "setstrength", "setdamping", "grip", "gravitymodifier"];
    let ps: Vec<String> = pick
        .iter()
        .filter_map(|k| {
            c.param(k).map(|v| {
                let s = match v {
                    Value::Number(n) => format!("{n}"),
                    Value::Vector(v) => v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("/"),
                    Value::Text(t) => t.clone(),
                    Value::List(l) => l.join(" "),
                };
                format!("{}={s}", k.trim_start_matches("set"))
            })
        })
        .collect();
    println!("{pad}{} [{}] @{:?} {}", c.name, c.kind, c.position, ps.join(" "));
    for k in &c.children {
        print(k, depth + 1);
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    if args.len() < 2 {
        eprintln!("usage: vehicles <game_dir> <mod> [--show <template>] [--json out.json]");
        return ExitCode::FAILURE;
    }
    let (_, vfs) = match session::mount(Path::new(&args[0]), &args[1]) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("vehicles: {e}");
            return ExitCode::FAILURE;
        }
    };
    let it = session::load(&vfs, None, "conquest");
    let db = &it.host.db;
    if let Some(name) = opt("--show") {
        match vehicle::model(db, &name) {
            Some(c) => {
                print(&c, 0);
                if let Some(j) = opt("--json") {
                    std::fs::write(j, serde_json::to_string_pretty(&c).unwrap()).unwrap();
                }
                return ExitCode::SUCCESS;
            }
            None => {
                eprintln!("vehicles: no template {name}");
                return ExitCode::FAILURE;
            }
        }
    }
    let mut list: Vec<_> = db
        .templates
        .values()
        .filter(|t| t.namespace == "objecttemplate" && t.kind.eq_ignore_ascii_case("playercontrolobject"))
        .filter_map(|t| vehicle::model(db, &t.name))
        .map(|c| vehicle::summary(&c))
        .collect();
    list.sort_by_key(|s| s.name.to_lowercase());
    for s in &list {
        println!(
            "{:28} mass {:>7} drag {:>5} comps {:>3} engines {:?} wings {} springs {} gear {} float {}",
            s.name,
            s.mass.map_or("-".into(), |m| m.to_string()),
            s.drag.map_or("-".into(), |d| d.to_string()),
            s.components,
            s.engines,
            s.wings,
            s.springs,
            s.landing_gear,
            s.floating
        );
    }
    eprintln!("{} PlayerControlObjects", list.len());
    if let Some(j) = opt("--json") {
        std::fs::write(j, serde_json::to_string_pretty(&list).unwrap()).unwrap();
    }
    ExitCode::SUCCESS
}
