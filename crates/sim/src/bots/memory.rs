//! `bots:memory`: what a veteran of a map knows of it before the round, learnt offline from the bots' own rounds
//!: per side, how often one dies per second spent in each cell of
//! the map, on foot and in a vehicle (and how much over its baseline: near a flag, the flag's surroundings; elsewhere, the map), and
//! how the arrivals at each flag went by the sector they came in from.
//!
//! It never says where the enemy is now, and nothing in it changes during the round. The bots read it in one place: a path on
//! foot ([`crate::bots::body::Maps::request_foot_from`]) pays a toll for crossing cells where the side dies more than round them (the
//! deaths are where the fighting is, round the flags: against the map's mean every way into a flag paid, and the attack slowed down,
//! Al_Nas −136 tickets a pair with a death at 1 000 m; against the flag's ring, the toll says which way in, not whether), as many metres as a death is worth ([`crate::bots::params::BotParams::memory_death_m`]) times the extra
//! deaths it expects on the way, so a way round a killing ground is taken when it is not much longer.
//!
//! TODO: parked, off by default. It acts (the side with it treads 20-60 % less danger on 7 of 8 maps), but against anchor 6 it
//! scores +0.02 ± 0.09 SD with the first memory and -0.35 ± 0.18 with a second one made with it on (128 and 64 pairs; DC_Al_Nas
//! -52 and -89 tickets): dying less on the way wins no tickets where the danger lies on the straight way between the flags, and
//! going round it arrives late. How to try again: the arrivals by sector the file already carries (the best sector's way in as a
//! waypoint), or the danger only in transit, away from the flags; finer cells in cities.
//!
//! The file is `<level>.json` in [`dir`] (`data/cache/memory`, or `BYTEFIELD_BOTS_MEMORY`); one made for other flags (another
//! level by the same name, or the flags moved) is left aside with a warning. Without a file, nothing changes.
use crate::navmesh::NavMesh;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The file's format; another is left aside.
pub const FORMAT: u32 = 2;
/// How fast a soldier crosses ground on its way, m/s: the seconds a polygon keeps it there.
pub const WALK_MPS: f32 = 4.0;
/// A flag of the file farther than this from the level's is another map, m.
pub const FLAG_SAME_M: f32 = 2.0;

/// One layer of a side (on foot or in a vehicle): the map's mean deaths per second of presence, and per cell (x, z, seconds, deaths,
/// danger: the deaths per second shrunk towards the mean where there are few seconds; excess: the danger over the baseline, the
/// ring of the flag within 100 m or else the map's mean).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Layer {
    pub mean: f32,
    pub cells: Vec<(i32, i32, f32, f32, f32, f32)>,
}

/// What a side learnt.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Side {
    pub foot: Layer,
    pub vehicle: Layer,
}

/// A level's memory.
#[derive(Clone, Debug, Deserialize)]
pub struct Memory {
    pub format: u32,
    pub level: String,
    /// the level's conquest points when it was made, in their order: the map's fingerprint
    pub flags: Vec<[f32; 3]>,
    pub cell_m: f32,
    /// by side ("1", "2")
    pub sides: BTreeMap<String, Side>,
}

/// Where the memories are: `BYTEFIELD_BOTS_MEMORY`, or `cache/memory` next to the game's tree.
pub fn dir(game: &Path) -> PathBuf {
    match std::env::var_os("BYTEFIELD_BOTS_MEMORY") {
        Some(d) => PathBuf::from(d),
        None => game.parent().unwrap_or(Path::new(".")).join("cache").join("memory"),
    }
}

impl Memory {
    /// The memory of `level` in `dir`, if there is one made for these flags (the level's conquest points, in order).
    pub fn load(dir: &Path, level: &str, flags: &[[f32; 3]]) -> Option<Memory> {
        let path = dir.join(format!("{level}.json"));
        let text = std::fs::read_to_string(&path).ok()?;
        let m: Memory = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("bots memory: {}: {e}; left aside", path.display());
                return None;
            }
        };
        let flat = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
        if m.format != FORMAT || !m.level.eq_ignore_ascii_case(level) || m.flags.len() != flags.len() || m.flags.iter().zip(flags).any(|(a, b)| flat(*a, *b) > FLAG_SAME_M) {
            eprintln!("bots memory: {}: made for another map or format ({} flags, format {}); left aside", path.display(), m.flags.len(), m.format);
            return None;
        }
        Some(m)
    }

    /// The tolls of side `team`'s paths on foot over `mesh`: per polygon whose centre is in a cell more dangerous than its baseline,
    /// `death_m` metres (in cm) times the extra deaths a soldier expects crossing it (the cell's excess deaths per second times the
    /// seconds the polygon's size takes at [`WALK_MPS`]), at most one death's worth. In polygon order.
    // TODO: the vehicle layer is made but not read: the vehicles' ways are on the tank grid (`paths::Searcher`), which has no
    // tolls yet; the engineer's mines could also take it.
    pub fn foot_tolls(&self, mesh: &NavMesh, team: u32, death_m: f32) -> Vec<(u32, u32)> {
        let Some(side) = self.sides.get(&team.to_string()) else { return Vec::new() };
        let excess: BTreeMap<(i32, i32), f32> = side.foot.cells.iter().map(|c| ((c.0, c.1), c.5)).filter(|c| c.1 > 0.0).collect();
        if excess.is_empty() || death_m <= 0.0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for p in 0..mesh.polys.len() as u32 {
            let c = mesh.centre(p);
            let key = ((c[0] / self.cell_m).floor() as i32, (c[2] / self.cell_m).floor() as i32);
            let Some(&x) = excess.get(&key) else { continue };
            let cm = (death_m * 100.0 * (x * area_xz(mesh, p).sqrt() / WALK_MPS).min(1.0)).round() as u32;
            if cm > 0 {
                out.push((p, cm));
            }
        }
        out
    }
}

/// The area of polygon `p` across, m².
fn area_xz(mesh: &NavMesh, p: u32) -> f32 {
    let v: Vec<[f32; 3]> = mesh.poly_verts(p).collect();
    let n = v.len();
    (0..n).map(|k| v[k][0] * v[(k + 1) % n][2] - v[(k + 1) % n][0] * v[k][2]).sum::<f32>().abs() * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Memory {
        let text = r#"{"format": 2, "level": "Synthetic", "flags": [[100.0, 0.0, 100.0]], "cell_m": 32.0, "made_by": {"rounds": 4},
            "sides": {"1": {"foot": {"mean": 0.01, "cells": [[0, 0, 600.0, 30.0, 0.05, 0.02], [1, 0, 600.0, 3.0, 0.005, -0.02]]},
                            "vehicle": {"mean": 0.0, "cells": []}, "arrivals": []}}}"#;
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn it_is_read_only_for_its_map() {
        let d = std::env::temp_dir().join(format!("bytefield-memory-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let text = r#"{"format": 2, "level": "Synthetic", "flags": [[100.0, 0.0, 100.0]], "cell_m": 32.0, "sides": {}}"#;
        std::fs::write(d.join("Synthetic.json"), text).unwrap();
        assert!(Memory::load(&d, "Synthetic", &[[100.5, 7.0, 100.0]]).is_some());
        // the flags moved, another count, another level: left aside
        assert!(Memory::load(&d, "Synthetic", &[[110.0, 0.0, 100.0]]).is_none());
        assert!(Memory::load(&d, "Synthetic", &[[100.0, 0.0, 100.0], [0.0; 3]]).is_none());
        assert!(Memory::load(&d, "Other", &[[100.0, 0.0, 100.0]]).is_none());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn only_the_cells_over_the_mean_toll() {
        let m = memory();
        let s = &m.sides["1"].foot;
        let over: Vec<_> = s.cells.iter().filter(|c| c.5 > 0.0).map(|c| (c.0, c.1)).collect();
        assert_eq!(over, vec![(0, 0)]);
        // a side with nothing learnt has no tolls
        assert!(m.sides.get("2").is_none());
    }
}
