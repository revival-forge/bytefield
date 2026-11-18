//! `bytefield-content`: looks at and changes `content.cfg`.
//!
//!     bytefield-content list                      the install, the mods, the packs, the profile
//!     bytefield-content install <dir>             where BF1942 is
//!     bytefield-content add <dir>                 a pack or a mod, whichever it is (as if dropped in a content folder)
//!     bytefield-content mods-dir add|rm <dir>     a folder with more mods (`<x>/init.con`)
//!     bytefield-content pack add <dir> [name]     a graphics pack (a tree like the install's), on
//!     bytefield-content pack on|off|rm|up|down <name>
//!     bytefield-content graphics original|enhanced
//!     bytefield-content set <key> <value|->   (graphics::KEYS: aniso, aa, bloom, look…)
//!     bytefield-content missing                   what the catalog knows of and is not there
//!     bytefield-content verify                    the install's files against the versions we know (reads them all: seconds)
//!
//! `BYTEFIELD_CONTENT` names another file. What is in the content folders (`discover`) is added, turned on, at each start
//! of this tool and of the client.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bytefield_content::detect::{self, Scan};
use bytefield_content::discover;
use bytefield_content::{Config, Graphics, Pack, Profile};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let code = match run(&a) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bytefield-content: {e}");
            ExitCode::FAILURE
        }
    };
    // Opened with a double click: the list, how to change it, and a pause, or the window closes before it can be read.
    if own_console() {
        println!("\nThis is a command-line tool: to change what is listed, open a terminal in this folder and run, for example,");
        println!("  .\\bytefield-content add <folder>      a mod or a graphics pack from any folder");
        println!("  .\\bytefield-content pack off <name>   turn a pack off (on, up, down, rm)");
        println!("  .\\bytefield-content verify            check the game's files");
        println!("Mods go in the game's Mods folder and graphics packs in a `content` folder next to bytefield.exe;");
        println!("both are found on their own. To play, open bytefield.exe.");
        wait_for_enter();
    }
    code
}

/// Whether this process is alone in its console: Windows made the console for it, as on a double click in Explorer.
#[cfg(windows)]
fn own_console() -> bool {
    let mut ids = [0u32; 2];
    // SAFETY: the buffer and its length match; the call only writes into it.
    unsafe { windows_sys::Win32::System::Console::GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) == 1 }
}

#[cfg(not(windows))]
fn own_console() -> bool {
    false
}

fn wait_for_enter() {
    println!("\nPress Enter to close.");
    let _ = std::io::stdin().read_line(&mut String::new());
}

fn abs(p: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(p);
    let p = if p.is_absolute() { p } else { std::env::current_dir().map_err(|e| e.to_string())?.join(p) };
    if !p.is_dir() {
        return Err(format!("{}: not a folder", p.display()));
    }
    Ok(p)
}

fn run(a: &[&str]) -> Result<(), String> {
    let file = bytefield_content::config::path().ok_or("no config folder (HOME or APPDATA unset)")?;
    let mut cfg = Config::load_from(&file).unwrap_or_default();
    // (none saved: the one the client would pick on its own, as it does at start; saved with the next change)
    let found = cfg.install.is_none().then(|| bytefield_content::first_run::resolve(Some(&cfg), &bytefield_content::first_run::candidates())).flatten();
    if let Some(f) = &found {
        cfg.install = Some(f.clone());
    }
    let content_dirs = discover::content_dirs();
    let ch = discover::apply(&mut cfg, &content_dirs);
    for n in &ch.packs {
        println!("found pack {n} (on)");
    }
    for d in &ch.mods_dirs {
        println!("found mods in {}", d.display());
    }
    for n in &ch.gone {
        println!("pack {n}: its folder has gone");
    }
    if ch.any() {
        cfg.save_to(&file)?;
    }
    let changed = match a {
        [] | ["list"] => {
            list(&cfg, &file);
            if found.is_some() {
                println!("\n(the install was found, not saved: bytefield saves it when it starts, or bytefield-content install <dir>)");
            }
            false
        }
        ["verify"] => {
            let install = cfg.install.clone().ok_or("no install yet: bytefield-content install <dir>")?;
            verify(&install);
            false
        }
        ["missing"] => {
            let install = cfg.install.clone().ok_or("no install yet: bytefield-content install <dir>")?;
            let s = detect::scan(&cfg, &install);
            for e in detect::missing(&s.mods, &s.packs) {
                println!("{:<14} {:<10} {}", e.id, e.kind, e.name);
                for l in &e.links {
                    println!("{:<25} {l}", "");
                }
                if !e.note.is_empty() {
                    println!("{:<25} {}", "", e.note);
                }
                println!("{:<25} ({})", "", e.license);
            }
            false
        }
        ["install", dir] => {
            let d = abs(dir)?;
            if bytefield_con::session::resolve_ci(&d, "Mods/bf1942/init.con").is_none() {
                return Err(format!("{}: no Mods/bf1942/init.con", d.display()));
            }
            cfg.install = Some(d);
            true
        }
        ["add", dir] => {
            println!("{}", cfg.add_dir(&abs(dir)?)?);
            true
        }
        ["mods-dir", "add", dir] => {
            let d = abs(dir)?;
            if !cfg.mods_dirs.contains(&d) {
                cfg.mods_dirs.push(d);
            }
            true
        }
        ["mods-dir", "rm", dir] => {
            let before = cfg.mods_dirs.len();
            cfg.mods_dirs.retain(|d| d != Path::new(dir) && abs(dir).ok().as_ref() != Some(d));
            if before == cfg.mods_dirs.len() {
                return Err(format!("{dir}: not a mods_dir"));
            }
            true
        }
        ["pack", "add", dir, rest @ ..] => {
            let d = abs(dir)?;
            if bytefield_con::session::resolve_ci(&d, "Mods").is_none() {
                return Err(format!("{}: not a pack (no Mods/ inside)", d.display()));
            }
            let name = rest.first().map(|s| s.to_string()).unwrap_or_else(|| detect::pack_name(&d));
            if cfg.pack(&name).is_some() {
                return Err(format!("a pack named {name} is there already"));
            }
            println!("{name}: {}", d.display());
            cfg.packs.push(Pack { name, path: d, on: true });
            true
        }
        ["pack", op @ ("on" | "off" | "rm" | "up" | "down"), name] => {
            let i = cfg.packs.iter().position(|p| p.name.eq_ignore_ascii_case(name)).ok_or(format!("no pack {name}"))?;
            match *op {
                "on" => cfg.packs[i].on = true,
                "off" => cfg.packs[i].on = false,
                "rm" if content_dirs.iter().any(|d| cfg.packs[i].path.starts_with(d)) => {
                    return Err(format!("{name} is in a content folder: it comes back at the next start; turn it off (pack off {name}) or delete its folder"));
                }
                "rm" => drop(cfg.packs.remove(i)),
                // up: under the others (earlier); down: over them
                "up" if i > 0 => cfg.packs.swap(i, i - 1),
                "down" if i + 1 < cfg.packs.len() => cfg.packs.swap(i, i + 1),
                _ => {}
            }
            true
        }
        ["graphics", p] => {
            cfg.graphics = Profile::parse(p).ok_or(format!("{p}: original or enhanced"))?;
            true
        }
        ["set", key, v] => {
            cfg.overrides.set(key, v)?;
            true
        }
        _ => return Err(format!("usage: list | missing | verify | install <dir> | add <dir> | mods-dir add|rm <dir> | pack add <dir> [name] | pack on|off|rm|up|down <name> | graphics original|enhanced | set <key> <v|-> (keys: {})", bytefield_content::graphics::KEYS.join(", "))),
    };
    if changed {
        cfg.save_to(&file)?;
        list(&cfg, &file);
    }
    Ok(())
}

fn verify(install: &Path) {
    println!("{}", install.display());
    let checks = bytefield_content::fingerprint::check_install(install);
    for c in &checks {
        if c.matches() {
            println!("{:<10} {}: all {} files as we know them", c.folder, c.version, c.same);
            continue;
        }
        println!("{:<10} nearest is {}: {} the same, {} different, {} missing", c.folder, c.version, c.same, c.different.len(), c.missing.len());
        for (what, names) in [("different", &c.different), ("missing", &c.missing)] {
            for n in names.iter().take(10) {
                println!("{:<10} {what}: {n}", "");
            }
            if names.len() > 10 {
                println!("{:<10} {what}: and {} more", "", names.len() - 10);
            }
        }
    }
    if checks.is_empty() {
        println!("none of the folders we know is here");
    } else if checks.iter().any(|c| !c.matches()) {
        println!("(another edition or a patch we do not know looks like this too; it may play well)");
    }
}

fn list(cfg: &Config, file: &Path) {
    println!("config: {}{}", file.display(), if file.exists() { "" } else { " (none yet)" });
    let Some(install) = &cfg.install else {
        println!("install: none (bytefield-content install <dir>)");
        return;
    };
    println!("install: {}", install.display());
    for d in discover::content_dirs() {
        println!("content folder: {}  (packs and mods dropped here are found on their own)", d.display());
    }
    for d in &cfg.mods_dirs {
        println!("mods_dir: {}{}", d.display(), if d.is_dir() { "" } else { "  (NOT THERE)" });
    }
    let Scan { mods, packs } = detect::scan(cfg, install);
    let present = detect::present(&mods, &packs);
    println!("\nmods:");
    for m in &mods {
        let known = match (m.entry, m.version_known()) {
            (Some(e), true) => e.name.clone(),
            (Some(e), false) => format!("{} (version not in the catalog)", e.name),
            (None, _) => "unknown".into(),
        };
        let mut warn = Vec::new();
        if !m.missing.is_empty() {
            warn.push(format!("MISSING {}", m.missing.join(", ")));
        }
        if let Some(e) = m.entry {
            for n in e.needs.iter().filter(|n| !present.contains(n)) {
                warn.push(format!("needs {n}"));
            }
        }
        println!(
            "  {:<14} {:<7} {:<36} {}{}{}",
            m.folder,
            m.init.version.as_deref().unwrap_or("?"),
            m.chain().join(" → "),
            known,
            if m.external { "  [mods_dir]" } else { "" },
            if warn.is_empty() { String::new() } else { format!("  ! {}", warn.join("; ")) }
        );
    }
    println!("\npacks (the later over the earlier):");
    if packs.is_empty() {
        println!("  none");
    }
    for p in &packs {
        println!(
            "  [{}] {:<12} {} {}  {}{}",
            if p.pack.on { "on " } else { "off" },
            p.pack.name,
            p.entry.map_or("unknown", |e| e.name.as_str()),
            if p.folders.is_empty() { String::new() } else { format!("(over {})", p.folders.join(", ")) },
            p.pack.path.display(),
            if p.exists { "" } else { "  (NOT THERE)" }
        );
    }
    let g = Graphics::resolve(cfg.graphics, &cfg.overrides);
    println!("\ngraphics: {}", g.describe());
    let n = detect::missing(&mods, &packs).len();
    if n > 0 {
        println!("{n} more in the catalog: bytefield-content missing");
    }
}
