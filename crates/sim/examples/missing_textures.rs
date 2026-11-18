//! Every texture a mesh's shader (`standardmesh/*.rs`) names that no file answers: not `<name>.dds`/`.tga`
//! (also under `texture/`) in the vfs, nor a Bink movie in the game's folder. A
//! texture the client cannot find is drawn white.
//! `cargo run --release -p bytefield-sim --example missing_textures -- data/game GCMOD GC_Hoth [--mode conquest] [--all]`
//! (by default only the meshes some geometry template of the level's names; `--all`: every shader in the vfs)
//! One line per texture: `missing <texture> in <n> shaders, e.g. <file.rs>`. Found counts the level's
//! `textureManager.alternativePath` by the file's name, as the client; `alternative <texture>`: one
//! not under `texture/` that only the alternative path answers (GC_Ryloth's `textures/custom/trench`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mode = a.iter().position(|x| x == "--mode").and_then(|i| a.get(i + 1)).map_or("conquest", |m| m.as_str());
    let game = std::path::Path::new(&a[1]);
    let w = bytefield_sim::World::load(game, &a[2], &a[3], mode).expect("world");
    let alternative = w.db.globals.iter().rev().find(|c| c.command.eq_ignore_ascii_case("textureManager.alternativePath")).and_then(|c| c.args.first());
    let alternative = alternative.map(|p| p.trim().trim_matches('"').replace('\\', "/").trim_matches('/').to_string()).filter(|p| !p.is_empty());
    let strip = |n: &str| {
        let n = n.replace('\\', "/");
        if n.to_ascii_lowercase().ends_with(".dds") || n.to_ascii_lowercase().ends_with(".tga") { n[..n.len() - 4].to_string() } else { n }
    };
    let own = |n: &str| {
        let n = strip(n);
        ["dds", "tga"].iter().any(|e| w.vfs.exists(&format!("{n}.{e}")) || w.vfs.exists(&format!("texture/{n}.{e}"))) || movie(game, &a[2], &n)
    };
    let alt = |n: &str| alternative.as_ref().is_some_and(|a| w.vfs.exists(&format!("{a}/{}.dds", strip(n).rsplit('/').next().unwrap_or(""))));
    // only the meshes a geometry template names (GC keeps vanilla's unused ones; `--all`: every one)
    let used: std::collections::HashSet<String> = w
        .db
        .templates
        .iter()
        .filter(|((ns, _), _)| ns.eq_ignore_ascii_case("geometrytemplate"))
        .filter_map(|(_, t)| t.get("file").and_then(|f| f.first()))
        .map(|f| format!("standardmesh/{}.rs", bytefield_vfs::normalize(f)))
        .collect();
    let all = a.iter().any(|x| x == "--all");
    let mut missing: std::collections::BTreeMap<String, (usize, String)> = Default::default();
    let mut by_alt: std::collections::BTreeSet<String> = Default::default();
    let mut shaders = 0;
    // (the level's own meshes are listed by their whole path, `bf1942/levels/<map>/standardMesh/…`)
    let dir = format!("bf1942/levels/{}/", a[3].to_lowercase());
    for p in w.vfs.list() {
        let p = bytefield_vfs::normalize(p);
        let p = p.strip_prefix(&dir).map_or(p.clone(), str::to_string);
        if !(p.starts_with("standardmesh/") && p.ends_with(".rs")) || !(all || used.contains(&p)) {
            continue;
        }
        shaders += 1;
        let Ok(b) = w.vfs.read(&p) else { continue };
        for s in bytefield_formats::rs::parse(&String::from_utf8_lossy(&b)) {
            if let Some(t) = s.texture.as_ref().filter(|t| !t.to_ascii_lowercase().starts_with("texture/") && !own(t) && alt(t)) {
                by_alt.insert(t.to_lowercase());
            }
            if let Some(t) = s.texture.filter(|t| !own(t) && !alt(t)) {
                let e = missing.entry(t.to_lowercase()).or_insert((0, p.clone()));
                e.0 += 1;
            }
        }
    }
    println!("{shaders} shaders, {} textures missing", missing.len());
    for (t, (n, f)) in &missing {
        println!("missing {t} in {n} shaders, e.g. {f}");
    }
    for t in &by_alt {
        println!("alternative {t}");
    }
}

/// As the client's `movie::resolve`: under the mod, bf1942, then the game's folder, any case.
fn movie(game: &std::path::Path, mod_name: &str, name: &str) -> bool {
    let rel = if name.to_ascii_lowercase().ends_with(".bik") { name.to_string() } else { format!("{name}.bik") };
    [game.join("Mods").join(mod_name), game.join("Mods/bf1942"), game.to_path_buf()].iter().any(|root| {
        let mut at = root.clone();
        for part in rel.split('/').filter(|p| !p.is_empty()) {
            match std::fs::read_dir(&at).ok().and_then(|d| d.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))) {
                Some(e) => at = e.path(),
                None => return false,
            }
        }
        at.is_file()
    })
}
