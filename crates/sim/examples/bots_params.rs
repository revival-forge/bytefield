//! The bots' named parameters (`bytefield_sim::bots::params`), for the tools of the learning loop:
//!   bots_params specs       every parameter's name, default, range, unit (and whether it is a count), scope and why, as JSON
//!   bots_params defaults    a layer file with every parameter at its default (an identity check: it must change nothing)
use bytefield_sim::bots::params::{Scope, SPECS};

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("specs") => {
            let l: Vec<serde_json::Value> = SPECS
                .iter()
                .map(|s| serde_json::json!({"name": s.name, "default": s.default, "min": s.min, "max": s.max, "unit": s.unit, "integer": s.integer(), "scope": if s.scope == Scope::Team { "team" } else { "individual" }, "why": s.why.split_whitespace().collect::<Vec<_>>().join(" ")}))
                .collect();
            println!("{}", serde_json::to_string_pretty(&l).unwrap());
        }
        Some("defaults") => {
            println!("about = \"every named parameter at its default, written out\"\n[params]");
            for s in SPECS {
                println!("{} = {:?}", s.name, s.default as f32);
            }
        }
        _ => {
            eprintln!("bots_params specs|defaults");
            std::process::exit(2);
        }
    }
}
