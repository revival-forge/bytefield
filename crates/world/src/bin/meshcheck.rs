//! `meshcheck <game_dir> <mod> [--json out.json]`: parse every `.sm` the mod sees and check
//! that the geometry templates' meshes exist.
//!
//! Per mesh: indices inside their vertex arrays, finite numbers, unit normals, vertices inside
//! the header's bounding box, collision faces inside their vertices. Per `StandardMesh`
//! geometry template: `standardmesh/<file>.sm` exists (the path rule is a HYPOTHESIS: next to
//! the mesh sits `<file>.rs`, its materials).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use bytefield_con::session;
use bytefield_formats::{rs, sm, tm};
use serde::Serialize;

#[derive(Default, Serialize)]
struct Report {
    meshes: usize,
    parsed: usize,
    errors: Vec<String>,
    versions: BTreeMap<u32, usize>,
    lods: BTreeMap<usize, usize>,
    with_collision: usize,
    with_shadow: usize,
    with_portals: usize,
    portals: usize,
    unknown_sections: usize,
    triangles: usize,
    bad_indices: Vec<String>,
    non_unit_normals: usize,
    normals: usize,
    outside_bounds: Vec<String>,
    templates: usize,
    templates_missing_mesh: Vec<String>,
    /// LOD 0 materials and how many have a subshader of the same name in `<mesh>.rs`
    tree_meshes: usize,
    tree_parsed: usize,
    tree_errors: Vec<String>,
    materials: usize,
    materials_with_shader: usize,
    materials_with_texture: usize,
    /// the textures a material names that are not there as `.dds`
    textures_missing: std::collections::BTreeSet<String>,
    /// not at its path, but a file of that name exists in another folder (an `alternativePath`, if the level names it)
    textures_elsewhere: std::collections::BTreeSet<String>,
    unmatched_examples: Vec<String>,
}

fn run(game: &Path, mod_name: &str, json: Option<&Path>) -> Result<(), String> {
    let (_, vfs) = session::mount(game, mod_name)?;
    let mut rep = Report::default();
    let paths: Vec<String> = vfs.list().into_iter().filter(|p| p.to_lowercase().ends_with(".sm")).map(str::to_string).collect();
    let file_names: std::collections::HashSet<String> = vfs
        .list()
        .into_iter()
        .filter_map(|p| {
            let l = p.to_ascii_lowercase();
            let stem = l.strip_suffix(".dds").or_else(|| l.strip_suffix(".tga"))?;
            Some(stem.rsplit('/').next().unwrap_or(stem).to_string())
        })
        .collect();
    for p in &paths {
        rep.meshes += 1;
        let bytes = match vfs.read(p) {
            Ok(b) => b,
            Err(e) => {
                rep.errors.push(format!("{p}: {e}"));
                continue;
            }
        };
        let m = match sm::parse(&bytes) {
            Ok(m) => m,
            Err(e) => {
                rep.errors.push(format!("{p}: {e}"));
                continue;
            }
        };
        rep.parsed += 1;
        let rs_path = format!("{}.rs", &p[..p.len() - 3]);
        let shaders = vfs.read(&rs_path).map(|b| rs::parse(&String::from_utf8_lossy(&b))).unwrap_or_default();
        for mat in m.lods.first().map(|l| l.materials.as_slice()).unwrap_or(&[]) {
            rep.materials += 1;
            match shaders.iter().find(|s| s.name.eq_ignore_ascii_case(&mat.name)) {
                Some(s) => {
                    rep.materials_with_shader += 1;
                    if let Some(t) = &s.texture {
                        // the client's lookup (`assets.rs`, `texture`): with or without `texture/`, `.dds` or `.tga`, the extension
                        // sometimes in the name. Its `alternativePath`s depend on the level: any folder with that file name counts apart
                        let n = t.replace('\\', "/");
                        let lower = n.to_ascii_lowercase();
                        let n = if lower.ends_with(".dds") || lower.ends_with(".tga") { &n[..n.len() - 4] } else { &n[..] };
                        if [format!("{n}.dds"), format!("texture/{n}.dds"), format!("{n}.tga"), format!("texture/{n}.tga")].iter().any(|p| vfs.exists(p)) {
                            rep.materials_with_texture += 1;
                        } else if file_names.contains(&n.rsplit('/').next().unwrap_or(n).to_ascii_lowercase()) {
                            rep.textures_elsewhere.insert(t.to_lowercase());
                        } else {
                            rep.textures_missing.insert(t.to_lowercase());
                        }
                    }
                }
                None => {
                    if rep.unmatched_examples.len() < 20 {
                        rep.unmatched_examples.push(format!("{p}: '{}' not in [{}]", mat.name, shaders.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")));
                    }
                }
            }
        }
        *rep.versions.entry(m.version).or_default() += 1;
        *rep.lods.entry(m.lods.len()).or_default() += 1;
        rep.with_collision += (!m.collision.is_empty()) as usize;
        rep.with_shadow += (!m.shadow_lods.is_empty()) as usize;
        rep.with_portals += (!m.portals.is_empty()) as usize;
        rep.portals += m.portals.len();
        rep.unknown_sections += m.unknown_sections;
        let (lo, hi) = (m.bounds[0], m.bounds[1]);
        let slack = |i: usize| 0.05 * (hi[i] - lo[i]).abs() + 0.01;
        let mut outside = 0;
        for (li, lod) in m.lods.iter().chain(&m.shadow_lods).enumerate() {
            for mat in &lod.materials {
                rep.triangles += mat.triangles.len();
                if mat.triangles.iter().flatten().any(|&i| i as usize >= mat.positions.len()) {
                    rep.bad_indices.push(format!("{p} lod {li} {}", mat.name));
                }
                for n in &mat.normals {
                    rep.normals += 1;
                    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    if !(0.9..=1.1).contains(&l) {
                        rep.non_unit_normals += 1;
                    }
                }
                if li == 0 {
                    outside += mat.positions.iter().filter(|v| (0..3).any(|i| !v[i].is_finite() || v[i] < lo[i] - slack(i) || v[i] > hi[i] + slack(i))).count();
                }
            }
        }
        if outside > 0 {
            rep.outside_bounds.push(format!("{p}: {outside} vertices"));
        }
        for c in &m.collision {
            if c.faces.iter().any(|(f, _)| f.iter().any(|&i| i as usize >= c.vertices.len())) {
                rep.bad_indices.push(format!("{p} collision"));
            }
        }
    }

    for p in vfs.list().into_iter().filter(|p| p.to_lowercase().ends_with(".tm")) {
        rep.tree_meshes += 1;
        match vfs.read(p).map_err(|e| e.to_string()).and_then(|b| tm::parse(&b).map_err(|e| e.to_string())) {
            Ok(t) => {
                rep.tree_parsed += 1;
                if t.indices.iter().any(|&i| i as usize >= t.positions.len()) {
                    rep.bad_indices.push(format!("{p} tree"));
                }
            }
            Err(e) => rep.tree_errors.push(format!("{p}: {e}")),
        }
    }

    let it = session::load(&vfs, None, "conquest");
    for t in it.host.db.templates.values() {
        if t.namespace == "geometrytemplate" && t.kind.eq_ignore_ascii_case("standardmesh") {
            rep.templates += 1;
            if let Some(f) = t.get("file").and_then(|a| a.first()) {
                let path = format!("standardmesh/{}.sm", f.replace('\\', "/"));
                if !vfs.exists(&path) {
                    rep.templates_missing_mesh.push(format!("{} -> {path} ({})", t.name, t.origin));
                }
            }
        }
    }
    rep.templates_missing_mesh.sort();

    println!(
        "{mod_name}: {}/{} meshes parsed, {} errors, {} triangles, {} with collision, {} with shadow LODs, {} with portals ({}), {} unknown sections",
        rep.parsed,
        rep.meshes,
        rep.errors.len(),
        rep.triangles,
        rep.with_collision,
        rep.with_shadow,
        rep.with_portals,
        rep.portals,
        rep.unknown_sections
    );
    println!(
        "  bad indices: {}, normals not unit: {}/{}, meshes with vertices outside their box: {}",
        rep.bad_indices.len(),
        rep.non_unit_normals,
        rep.normals,
        rep.outside_bounds.len()
    );
    println!("  StandardMesh templates: {}, mesh not found: {}", rep.templates, rep.templates_missing_mesh.len());
    println!("  tree meshes: {}/{} parsed {:?}", rep.tree_parsed, rep.tree_meshes, rep.tree_errors.iter().take(3).collect::<Vec<_>>());
    println!("  LOD0 materials: {}, with a subshader: {}, with a texture found: {}", rep.materials, rep.materials_with_shader, rep.materials_with_texture);
    println!("  textures only in another folder ({}): {}", rep.textures_elsewhere.len(), rep.textures_elsewhere.iter().cloned().collect::<Vec<_>>().join(", "));
    println!("  textures not found ({}): {}", rep.textures_missing.len(), rep.textures_missing.iter().cloned().collect::<Vec<_>>().join(", "));
    for e in rep.unmatched_examples.iter().take(5) {
        println!("  unmatched: {e}");
    }
    for e in rep.errors.iter().take(10) {
        println!("  error: {e}");
    }
    if let Some(j) = json {
        std::fs::write(j, serde_json::to_string_pretty(&rep).unwrap()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().position(|a| a == "--json").and_then(|i| args.get(i + 1)).map(Path::new);
    if args.len() < 2 {
        eprintln!("usage: meshcheck <game_dir> <mod> [--json out.json]");
        return ExitCode::FAILURE;
    }
    match run(Path::new(&args[0]), &args[1], json) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("meshcheck: {e}");
            ExitCode::FAILURE
        }
    }
}
