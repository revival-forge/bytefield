//! Sweeps the sound scripts of an extracted tree: `ssc <dir> [--show <file.ssc>]`.
//! Prints the words it did not know and the samples whose wav is missing under `<dir>/sound`.
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Case-insensitive lookup, as the archives are.
fn find(base: &Path, rel: &str) -> Option<PathBuf> {
    let mut cur = base.to_path_buf();
    for part in rel.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".") {
        if part == ".." {
            cur.pop();
            continue;
        }
        let hit = std::fs::read_dir(&cur).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?;
        cur = hit.path();
    }
    Some(cur)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let show = args.iter().position(|a| a == "--show").map(|i| args[i + 1].clone());
    let mut files = Vec::new();
    walk(&root, &mut files);
    let wavs: HashSet<String> = files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")))
        .map(|p| p.file_name().unwrap().to_string_lossy().to_lowercase())
        .collect();
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
    let mut sources: BTreeMap<String, usize> = BTreeMap::new();
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    let (mut n, mut patches, mut samples) = (0, 0, 0);
    for f in files.iter().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ssc"))) {
        let text = String::from_utf8_lossy(&std::fs::read(f).unwrap()).into_owned();
        let dir = f.parent().unwrap().to_path_buf();
        let s = bytefield_formats::ssc::parse(&text, &mut |p| {
            find(&dir, p).and_then(|q| std::fs::read(q).ok()).map(|b| String::from_utf8_lossy(&b).into_owned())
        });
        n += 1;
        patches += s.patches.len();
        for w in &s.unknown {
            *unknown.entry(w.clone()).or_default() += 1;
        }
        for smp in s.patches.iter().flat_map(|p| &p.samples) {
            samples += 1;
            if !wavs.contains(&smp.file_name().to_lowercase()) {
                *missing.entry(smp.path.clone()).or_default() += 1;
            }
            for e in &smp.effects {
                *sources.entry(format!("{} <- {} ({}, {} params)", e.destination, e.source, e.envelope, e.params.len())).or_default() += 1;
            }
        }
        if show.as_deref().is_some_and(|w| f.to_string_lossy().to_lowercase().ends_with(&w.to_lowercase())) {
            println!("{}: {:#?}", f.display(), s);
        }
    }
    println!("{n} scripts, {patches} patches, {samples} samples");
    println!("effects:");
    for (k, v) in &sources {
        println!("  {v:5} {k}");
    }
    println!("unknown words: {}", unknown.len());
    for (k, v) in unknown.iter().take(30) {
        println!("  {v:4} {k}");
    }
    println!("missing wavs: {}", missing.len());
    for (k, v) in missing.iter().take(30) {
        println!("  {v:4} {k}");
    }
}
