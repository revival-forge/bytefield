//! A static census of the sound a mod references and the vfs does not answer: every object template's
//! `loadSoundScript` (each index), the scripts' samples (`sound/<rate>/<path>`, as the client resolves them) and the
//! level's `Sounds/Environment.con`, over every level of the mod; plus the mod's `Game.set*MusicFilename`s.
//! `cargo run --release -p bytefield-sim --example sound_census -- data/game GCMOD [--out data/out/gc-sound]`
//! Writes `<out>/<mod>.json` and `<out>/<mod>.txt`; the unresolved ones come grouped by cause:
//!   script-missing   a `loadSoundScript` file found neither beside the template nor in `../Common/Sounds`
//!   wav-elsewhere    a sample the client's lookup misses but a file of the same name is under `sound/` (path or folder wrong)
//!   wav-other-ext    same stem under `sound/` with another extension
//!   wav-language     an `@Language/` sample (the soldier's radio language decides; not counted as missing)
//!   wav-absent       no file of that name anywhere under `sound/`
//!   music-absent     a `Game.set*MusicFilename` with no file in the mod's own folder (the original does not fall back either)
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use bytefield_formats::ssc::SoundScript;

/// The client's sample lookup ([`Sounds::wav`], [`Sample::wav_paths`]): the mod's `sound/<rate>/`, then the level's.
fn sample_found(vfs: &bytefield_vfs::Vfs, s: &bytefield_formats::ssc::Sample, sub: &str, level: &str) -> bool {
    s.wav_paths(sub, Some(level)).iter().any(|p| vfs.exists(p))
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let game = Path::new(&a[1]);
    let mod_name = a[2].as_str();
    let out = a.iter().position(|x| x == "--out").and_then(|i| a.get(i + 1)).map_or("data/out/gc-sound", |s| s.as_str());
    let (_, vfs0) = bytefield_con::session::mount(game, mod_name).expect("mount");
    // every file under sound/ by lowercase file name and by stem, for the "elsewhere" and "other extension" causes
    let mut by_name: HashMap<String, String> = HashMap::new();
    let mut by_stem: HashMap<String, String> = HashMap::new();
    for p in vfs0.list() {
        let n = bytefield_vfs::normalize(p);
        if n.starts_with("sound/") {
            let file = n.rsplit('/').next().unwrap().to_string();
            by_name.entry(file.clone()).or_insert_with(|| n.clone());
            by_stem.entry(file.rsplit_once('.').map_or(file.clone(), |(s, _)| s.to_string())).or_insert(n.clone());
        }
    }
    // cause -> key -> (levels, example)
    let bad: std::cell::RefCell<BTreeMap<&str, BTreeMap<String, (BTreeSet<String>, String)>>> = Default::default();
    let mut seen_ok = 0usize;
    let mut seen_all: BTreeSet<String> = BTreeSet::new();
    let note = |cause: &'static str, key: String, level: &str, example: String| {
        bad.borrow_mut().entry(cause).or_default().entry(key).or_insert_with(|| (BTreeSet::new(), example)).0.insert(level.to_string());
    };
    let levels = bytefield_con::session::levels(&vfs0);
    let mut done = 0;
    for (level, modes) in &levels {
        let Some(mode) = modes.first() else { continue };
        let Ok(w) = bytefield_sim::World::load(game, mod_name, level, mode) else {
            eprintln!("{level}: does not load");
            continue;
        };
        done += 1;
        let vfs = &w.vfs;
        let mut check = |text: String, base: &str, who: &str, level: &str| {
            let script: SoundScript = bytefield_formats::ssc::parse(&text, &mut |p| vfs.read(&format!("{base}/{}", p.trim_start_matches(['/', '\\']))).ok().map(|b| String::from_utf8_lossy(&b).into_owned()));
            for s in script.patches.iter().flat_map(|p| &p.samples) {
                let lang = s.path.to_ascii_lowercase().contains("@language");
                let key = s.path.replace('\\', "/");
                if lang {
                    // any language folder having the file is enough
                    let ok = by_name.contains_key(&s.file_name().to_ascii_lowercase());
                    if !ok {
                        note("wav-language", key, level, who.to_string());
                    }
                    continue;
                }
                if !seen_all.insert(key.to_ascii_lowercase()) {
                    continue;
                }
                if sample_found(vfs, s, "", &w.level.name) {
                    seen_ok += 1;
                    continue;
                }
                let file = s.file_name().to_ascii_lowercase();
                let stem = file.rsplit_once('.').map_or(file.clone(), |(s, _)| s.to_string());
                if let Some(p) = by_name.get(&file) {
                    note("wav-elsewhere", key, level, format!("{who}; exists as {p}"));
                } else if let Some(p) = by_stem.get(&stem) {
                    note("wav-other-ext", key, level, format!("{who}; exists as {p}"));
                } else {
                    note("wav-absent", key, level, who.to_string());
                }
            }
        };
        // object templates' scripts (the client's `load_script`: beside the creating file, then `Common/Sounds` one level up)
        for t in w.db.templates.iter().filter(|((ns, _), _)| ns.eq_ignore_ascii_case("objecttemplate")).map(|(_, t)| t) {
            for (n, args) in t.all("loadSoundScript").iter().enumerate() {
                let Some(rel) = args.first() else { continue };
                let file = t.origin.rsplit_once(':').map_or(t.origin.as_str(), |(f, _)| f);
                let dir = file.rsplit_once(['/', '\\']).map_or("", |(d, _)| d).to_string();
                let common = format!("{}/Common/Sounds", dir.rsplit_once(['/', '\\']).map_or("", |(d, _)| d));
                let rel = rel.trim_matches('"').trim_start_matches(['/', '\\']).to_string();
                let found = [&dir, &common].into_iter().find_map(|d| {
                    let p = format!("{d}/{rel}");
                    vfs.read(&p).ok().map(|b| (String::from_utf8_lossy(&b).into_owned(), p.rsplit_once(['/', '\\']).map_or(String::new(), |(d, _)| d.to_string())))
                });
                match found {
                    Some((text, base)) => check(text, &base, &format!("{}#{n}", t.name), level),
                    None => note("script-missing", format!("{dir}/{rel}"), level, format!("{}#{n}", t.name)),
                }
            }
        }
        // the level's own ambience
        let dir = format!("bf1942/levels/{}/sounds", w.level.name);
        if let Ok(b) = vfs.read(&format!("{dir}/environment.con")) {
            for line in String::from_utf8_lossy(&b).lines() {
                let wd: Vec<&str> = line.split_whitespace().collect();
                if wd.len() >= 2 && wd[0].eq_ignore_ascii_case("EnvironmentSound.load") {
                    match vfs.read(&format!("{dir}/{}", wd[1])) {
                        Ok(t) => check(String::from_utf8_lossy(&t).into_owned(), &dir, "environment", level),
                        Err(_) => note("script-missing", format!("{dir}/{}", wd[1]), level, "environment".into()),
                    }
                }
            }
        }
    }
    // the menu/load/win/lose/briefing music of the mod's init.con
    let init = bytefield_con::session::resolve_ci(game, &format!("Mods/{mod_name}/init.con")).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let mut music = Vec::new();
    for line in init.lines() {
        let wd: Vec<&str> = line.split_whitespace().collect();
        if wd.len() >= 2 && wd[0].to_ascii_lowercase().starts_with("game.set") && wd[0].to_ascii_lowercase().ends_with("musicfilename") {
            let rel = wd[1].trim_matches('"');
            let own = bytefield_con::session::resolve_ci(game, &format!("Mods/{mod_name}/{rel}")).is_some();
            let in_base = bytefield_con::session::resolve_ci(game, &format!("Mods/bf1942/{rel}")).is_some();
            music.push((wd[0].to_string(), rel.to_string(), own, in_base));
            if !own {
                note("music-absent", rel.to_string(), "init", format!("{} (bf1942 has it: {in_base})", wd[0]));
            }
        }
    }
    // reports
    let mut txt = format!("sound census: mod {mod_name}, {done} levels, {seen_ok} distinct samples resolved\n");
    let mut json = serde_json::Map::new();
    json.insert("mod".into(), mod_name.into());
    json.insert("levels".into(), done.into());
    json.insert("resolved_samples".into(), seen_ok.into());
    let mut total = 0;
    let bad = bad.into_inner();
    for (cause, items) in &bad {
        total += items.len();
        txt += &format!("\n== {cause}: {} distinct\n", items.len());
        let mut arr = Vec::new();
        for (k, (lv, ex)) in items {
            txt += &format!("  {k}  [{} levels] e.g. {ex}\n", lv.len());
            arr.push(serde_json::json!({"ref": k, "levels": lv, "example": ex}));
        }
        json.insert((*cause).into(), arr.into());
    }
    txt += &format!("\nunresolved (distinct, language ones included): {total}\n");
    json.insert("unresolved".into(), total.into());
    json.insert("music".into(), music.iter().map(|(c, r, o, b)| serde_json::json!({"command": c, "file": r, "in_mod": o, "in_bf1942": b})).collect::<Vec<_>>().into());
    std::fs::create_dir_all(out).unwrap();
    std::fs::write(format!("{out}/{mod_name}.txt"), &txt).unwrap();
    std::fs::write(format!("{out}/{mod_name}.json"), serde_json::to_string_pretty(&json).unwrap()).unwrap();
    print!("{txt}");
}
