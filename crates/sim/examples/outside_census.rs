//! Which spawn points fall outside the play area (`Sim::outside_play`), per level: a person spawning there starts its
//! ten seconds. Also how much of the combat area is the material that hurts.
//!   outside_census <game> <mod> [mode]
//! Prints `<level> <spawn points> <outside> <% of the area on material 7> <first outside>` per level.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let game = std::path::PathBuf::from(&a[1]);
    let mode = a.get(3).map_or("conquest", |s| s);
    let (_, vfs) = bytefield_con::session::mount(&game, &a[2]).expect("the mod");
    let mut levels: Vec<String> = bytefield_con::session::levels(&vfs).into_iter().map(|(l, _)| l).collect();
    levels.sort();
    for level in levels {
        let Ok(w) = bytefield_sim::World::load(&game, &a[2], &level, mode) else {
            println!("{level} did not load");
            continue;
        };
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.start_conquest();
        let (mut n, mut out, mut first) = (0, 0, None);
        if let Some(c) = s.modes.conquest.as_ref() {
            for g in c.groups.values() {
                for sp in &g.spawns {
                    n += 1;
                    if s.outside_play([sp.pos[0], sp.pos[1], sp.pos[2]]) {
                        out += 1;
                        first.get_or_insert(sp.pos);
                    }
                }
            }
        }
        let ws = w.level.terrain.world_size;
        let [cx, cz, aw, ah] = w.level.combat_area.unwrap_or([0.0, 0.0, ws, ws]);
        let (mut cells, mut hurt) = (0u64, 0u64);
        for i in 0..200 {
            for j in 0..200 {
                let (x, z) = (cx + aw * (i as f64 + 0.5) / 200.0, cz + ah * (j as f64 + 0.5) / 200.0);
                cells += 1;
                hurt += (w.level.material_at(x, z) == bytefield_sim::players::MATERIAL_TO_GIVE_DAMAGE) as u64;
            }
        }
        println!("{level} {n} {out} {:.1} {first:?}", 100.0 * hurt as f64 / cells as f64);
    }
}
