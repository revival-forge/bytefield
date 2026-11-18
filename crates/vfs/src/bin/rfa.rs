//! `rfa`: list, check and extract Refractor archives.
//!
//!     rfa list  <file.rfa>
//!     rfa check <dir>                         decode every file of every archive under dir
//!     rfa extract <out> <mod_dir>... [--ext con,inc]
//!         the files of the mod paths as the game sees them (first mod path wins),
//!         like `game.addModPath`; e.g. data/game/Mods/XPack1 data/game/Mods/bf1942

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bytefield_vfs::{Vfs, rfa::Archive};

fn rfas(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rfas(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("rfa")) {
            out.push(p);
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("list") => {
            let a = Archive::open(args.get(1).ok_or("list <file.rfa>")?).map_err(|e| e.to_string())?;
            println!("# compressed: {}, {} files", a.compressed, a.entries.len());
            for e in &a.entries {
                println!("{:>10} {:>10} {}", e.ucsize, e.csize, e.name);
            }
        }
        Some("check") => {
            let mut list = Vec::new();
            rfas(Path::new(args.get(1).ok_or("check <dir>")?), &mut list);
            list.sort();
            let (mut files, mut bytes, mut bad) = (0usize, 0u64, 0usize);
            for p in &list {
                let a = match Archive::open(p) {
                    Ok(a) => a,
                    Err(e) => {
                        println!("BAD {}: {e}", p.display());
                        bad += 1;
                        continue;
                    }
                };
                for e in &a.entries {
                    match a.read(e) {
                        Ok(d) => {
                            files += 1;
                            bytes += d.len() as u64;
                        }
                        Err(err) => {
                            println!("BAD {err}");
                            bad += 1;
                        }
                    }
                }
            }
            println!("{} archives, {files} files, {} MB decoded, {bad} errors", list.len(), bytes >> 20);
            if bad > 0 {
                return Err("errors".into());
            }
        }
        Some("extract") => {
            let out = PathBuf::from(args.get(1).ok_or("extract <out> <mod_dir>...")?);
            let mut exts: Option<Vec<String>> = None;
            let mut prefix: Option<String> = None;
            let mut vfs = Vfs::new();
            let mut it = args[2..].iter();
            while let Some(a) = it.next() {
                if a == "--ext" {
                    exts = Some(it.next().ok_or("--ext list")?.split(',').map(|s| format!(".{}", s.to_lowercase())).collect());
                } else if a == "--prefix" {
                    prefix = Some(it.next().ok_or("--prefix dir/")?.to_lowercase());
                } else {
                    vfs.mount_mod(Path::new(a)).map_err(|e| format!("{a}: {e}"))?;
                }
            }
            let (mut n, mut skipped) = (0, 0);
            // archives spell the same directory differently (Bf1942/ vs bf1942/): on a
            // case-sensitive disk, keep the first spelling of each directory
            let mut dirs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            for name in vfs.list() {
                if let Some(exts) = &exts
                    && !exts.iter().any(|x| name.to_lowercase().ends_with(x.as_str()))
                {
                    continue;
                }
                if let Some(p) = &prefix
                    && !name.to_lowercase().starts_with(p.as_str())
                {
                    continue;
                }
                let data = match vfs.read(name) {
                    Ok(d) => d,
                    Err(e) => {
                        // a broken entry in a mod (DC_Final ships one): report it, keep going
                        eprintln!("skipped {name}: {e}");
                        skipped += 1;
                        continue;
                    }
                };
                let mut fixed = String::new();
                let parts: Vec<&str> = name.split('/').collect();
                for (i, part) in parts.iter().enumerate() {
                    let key = format!("{fixed}/{part}").to_lowercase();
                    let spelled = if i + 1 < parts.len() { dirs.entry(key).or_insert_with(|| part.to_string()).clone() } else { part.to_string() };
                    fixed = if fixed.is_empty() { spelled } else { format!("{fixed}/{spelled}") };
                }
                let name = fixed.as_str();
                let dst = out.join(name);
                if !dst.starts_with(&out) || name.contains("..") {
                    return Err(format!("refusing path {name}"));
                }
                std::fs::create_dir_all(dst.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(&dst, data).map_err(|e| e.to_string())?;
                n += 1;
            }
            println!("{n} files -> {}, {skipped} unreadable", out.display());
        }
        _ => return Err("usage: rfa list <file.rfa> | check <dir> | extract <out> <mod_dir>... [--ext con,inc] [--prefix menu/]".into()),
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rfa: {e}");
            ExitCode::FAILURE
        }
    }
}
