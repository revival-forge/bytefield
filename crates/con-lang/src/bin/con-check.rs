//! `con`: run the scripts of a game or mod and report what does not add up.
//!
//!     con lexstats <dir>                      command/keyword counts (diffed against Python)
//!     con levels <game_dir> <mod>             each level and the game modes it has
//!     con load <game_dir> <mod> [--level L] [--mode conquest] [--json out.json]
//!
//! `load` follows `bytefield_con::session` (the load order and its hypotheses are documented there).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use bytefield_con::{DirFs, ScriptFs, lexer, session};
use serde::Serialize;

fn lexstats(dir: &Path) -> serde_json::Value {
    let fs = DirFs::new(dir).expect("read dir");
    let mut paths: Vec<&String> = fs.paths().filter(|p| p.ends_with(".con") || p.ends_with(".inc")).collect();
    paths.sort();
    let (mut lines, mut commands, mut keywords) = (0usize, BTreeMap::<String, usize>::new(), BTreeMap::<String, usize>::new());
    const KW: &[&str] = &["run", "include", "echo", "if", "elseif", "else", "endif", "while", "endwhile", "var", "const", "return", "endrem"];
    for p in &paths {
        let bytes = fs.read(p).unwrap();
        for l in lexer::lines(&lexer::decode(&bytes)) {
            lines += 1;
            let low = l.command.to_lowercase();
            if KW.contains(&low.as_str()) {
                *keywords.entry(low).or_default() += 1;
            } else if low.contains('.') && !low.starts_with("v_") && !low.starts_with("c_") {
                *commands.entry(low).or_default() += 1;
            }
        }
    }
    serde_json::json!({ "files": paths.len(), "lines": lines, "commands": commands, "keywords": keywords })
}

#[derive(Serialize)]
struct Report {
    mod_paths: Vec<String>,
    scripts_run: usize,
    lines_run: usize,
    templates: usize,
    census: Vec<(String, usize)>,
    instances: usize,
    /// instances whose template does not exist
    missing_instance_templates: BTreeMap<String, usize>,
    /// `addTemplate X` where no ObjectTemplate X exists
    missing_children: BTreeMap<String, usize>,
    recreated: usize,
    orphan_props: usize,
    muted_props: usize,
    runs: bytefield_con::RunStats,
    diagnostics_by_kind: Vec<(String, usize)>,
    diagnostics: Vec<bytefield_con::Diagnostic>,
}

fn load(game: &Path, mod_name: &str, level: Option<&str>, mode: &str, json: Option<&Path>) -> Result<(), String> {
    let (mod_paths, vfs) = session::mount(game, mod_name)?;
    eprintln!("mod paths: {}", mod_paths.join(" -> "));
    let it = session::load(&vfs, level, mode);

    let db = &it.host.db;
    let mut missing_instance_templates = BTreeMap::new();
    for i in &db.instances {
        if db.object_template(&i.template).is_none() {
            *missing_instance_templates.entry(i.template.to_lowercase()).or_default() += 1;
        }
    }
    let mut missing_children = BTreeMap::new();
    for t in db.templates.values().filter(|t| t.namespace == "objecttemplate") {
        for a in t.all("addtemplate") {
            if let Some(c) = a.first()
                && db.object_template(c).is_none()
            {
                *missing_children.entry(c.to_lowercase()).or_default() += 1;
            }
        }
    }
    let report = Report {
        mod_paths,
        scripts_run: it.scripts_run,
        lines_run: it.lines_run,
        templates: db.templates.len(),
        census: db.census().into_iter().map(|((ns, k), n)| (format!("{ns}.{k}"), n)).collect(),
        instances: db.instances.len(),
        missing_instance_templates,
        missing_children,
        recreated: db.recreated.len(),
        orphan_props: db.orphan_props,
        muted_props: db.muted_props,
        runs: it.runs.clone(),
        diagnostics_by_kind: it.summary().into_iter().map(|(k, n)| (format!("{k:?}"), n)).collect(),
        diagnostics: it.diagnostics.clone(),
    };
    println!(
        "{} scripts, {} lines, {} templates, {} instances ({} with no template), {} addTemplate targets missing, {} recreated ({} lines after them dropped), {} orphan props",
        report.scripts_run,
        report.lines_run,
        report.templates,
        report.instances,
        report.missing_instance_templates.values().sum::<usize>(),
        report.missing_children.values().sum::<usize>(),
        report.recreated,
        report.muted_props,
        report.orphan_props
    );
    println!("runs: {:?}", report.runs);
    println!("diagnostics: {:?}", report.diagnostics_by_kind);
    if let Some(j) = json {
        std::fs::write(j, serde_json::to_string_pretty(&report).unwrap()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str);
    let result = match args.first().map(String::as_str) {
        Some("lexstats") if args.len() == 2 => {
            println!("{}", serde_json::to_string_pretty(&lexstats(Path::new(&args[1]))).unwrap());
            Ok(())
        }
        Some("levels") if args.len() == 3 => session::mount(Path::new(&args[1]), &args[2]).map(|(_, vfs)| {
            for (level, modes) in session::levels(&vfs) {
                println!("{level}\t{}", modes.join(","));
            }
        }),
        Some("load") if args.len() >= 3 => load(Path::new(&args[1]), &args[2], opt("--level"), opt("--mode").unwrap_or("conquest"), opt("--json").map(Path::new)),
        _ => Err("usage: con lexstats <dir> | con levels <game_dir> <mod> | con load <game_dir> <mod> [--level L] [--mode M] [--json out]".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("con: {e}");
            ExitCode::FAILURE
        }
    }
}
