//! The walkable-surface navigation mesh (`bytefield_sim::navmesh`) on the real data: the same mesh on any number of threads,
//! a synthetic bridge gives two floors joined by its ramps, and the cache gives back what was built. Skipped without the
//! real data.
use bytefield_sim::navmesh::{Cached, NavMesh, Params};
use bytefield_sim::obstacle::Obstacle;

#[test]
fn the_mesh_does_not_depend_on_the_threads() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let w = bytefield_sim::World::load(&bytefield_testdata::base().unwrap(), "bf1942", "Wake", "conquest").unwrap();
    let p = Params::infantry(&w);
    // the airfield and the bunkers around it
    let area = Some([1000.0, 600.0, 1250.0, 850.0]);
    let mut one = NavMesh::build_in(&w, &p, 1, area);
    let four = NavMesh::build_in(&w, &p, 4, area);
    assert!(one.polys.len() > 100, "{}", one.polys.len());
    assert_eq!(one.fingerprint(), four.fingerprint());
    // all but the time each tile took
    one.stats = four.stats.clone();
    assert!(one == four, "the meshes differ");
}

/// A ramp rising towards −z (the mirror of [`Obstacle::synthetic_ramp`]).
fn ramp_down(lo: [f32; 3], hi: [f32; 3]) -> Obstacle {
    let r = Obstacle::synthetic_ramp("ramp", lo, hi);
    let m = &r.lods[0];
    let verts = m.vertices.iter().map(|v| [v[0], v[1], lo[2] + hi[2] - v[2]]).collect();
    let tris: Vec<[u32; 3]> = m.faces.iter().map(|f| [f.v[0], f.v[2], f.v[1]]).collect();
    Obstacle::synthetic("ramp", verts, &tris)
}

#[test]
fn a_bridge_is_two_floors_joined_by_its_ramps() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let (x, z) = (1100.0f32, 1100.0f32);
    let floors = |ramps: bool| {
        let mut w = bytefield_sim::World::load(&bytefield_testdata::base().unwrap(), "bf1942", "El_Alamein", "conquest").unwrap();
        let g = w.height_at(x as f64, z as f64).unwrap() as f32;
        // a deck 6 m up, 6 m wide, from z + 10 to z + 30; ramps of 31° at both ends
        let mut parts = vec![Obstacle::synthetic_box("deck", [x - 3.0, g + 5.6, z + 10.0], [x + 3.0, g + 6.0, z + 30.0])];
        if ramps {
            parts.push(Obstacle::synthetic_ramp("ramp", [x - 3.0, g - 0.5, z], [x + 3.0, g + 6.0, z + 10.0]));
            parts.push(ramp_down([x - 3.0, g - 0.5, z + 30.0], [x + 3.0, g + 6.0, z + 40.0]));
        }
        w.push_obstacles(parts);
        let m = NavMesh::build_in(&w, &Params::infantry(&w), 2, Some([x - 40.0, z - 40.0, x + 40.0, z + 80.0]));
        // from the ground before the bridge
        let reached = m.reach(&[[x, g, z - 10.0]]);
        // the floors over the middle of the bridge: (height over the ground, reached)
        let mut over: Vec<(f32, bool)> = (0..m.polys.len() as u32)
            .filter_map(|p| {
                let q = m.closest_on(p, x, z + 20.0);
                ((q[0] - x).abs() < 1e-3 && (q[2] - z - 20.0).abs() < 1e-3).then_some((q[1] - g, reached[p as usize]))
            })
            .collect();
        over.sort_by(|a, b| a.0.total_cmp(&b.0));
        over.dedup_by(|b, a| (b.0 - a.0).abs() < 0.5);
        over
    };
    let with = floors(true);
    assert_eq!(with.len(), 2, "the ground and the deck: {with:?}");
    assert!(with[0].0.abs() < 0.5 && with[0].1, "the ground under it, reached: {with:?}");
    assert!((with[1].0 - 6.0).abs() < 0.5 && with[1].1, "the deck, reached up the ramps: {with:?}");
    let without = floors(false);
    assert!(without.len() == 2 && !without[1].1, "without ramps the deck is an island: {without:?}");
}

/// A platform 5 m up reached by one ramp, as steep as asked: 45° joins it (the stairs of DC_LostVillage's base house, whose
/// upper floor holds a spawn point, are a smooth ramp of 45°), 50° does not (a soldier walks up 46° at most).
#[test]
fn a_ramp_of_45_degrees_joins_its_floor_and_one_of_50_does_not() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let (x, z) = (1100.0f32, 1100.0f32);
    let joined = |deg: f32| {
        let mut w = bytefield_sim::World::load(&bytefield_testdata::base().unwrap(), "bf1942", "El_Alamein", "conquest").unwrap();
        let g = w.height_at(x as f64, z as f64).unwrap() as f32;
        let run = 5.0 / deg.to_radians().tan();
        w.push_obstacles(vec![
            Obstacle::synthetic_box("deck", [x - 2.0, g + 4.6, z + run], [x + 2.0, g + 5.0, z + run + 8.0]),
            Obstacle::synthetic_ramp("ramp", [x - 2.0, g, z], [x + 2.0, g + 5.0, z + run]),
        ]);
        let m = NavMesh::build_in(&w, &Params::infantry(&w), 2, Some([x - 40.0, z - 40.0, x + 40.0, z + 60.0]));
        let reached = m.reach(&[[x, g, z - 10.0]]);
        (0..m.polys.len() as u32).any(|p| {
            let q = m.closest_on(p, x, z + run + 4.0);
            (q[0] - x).abs() < 1e-3 && (q[2] - z - run - 4.0).abs() < 1e-3 && (q[1] - g - 5.0).abs() < 0.5 && reached[p as usize]
        })
    };
    assert!(joined(45.0), "the deck up a ramp of 45° is an island");
    assert!(!joined(50.0), "the deck up a ramp of 50° is reached");
}

#[test]
fn the_cache_gives_back_the_mesh() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let w = bytefield_sim::World::load(&bytefield_testdata::base().unwrap(), "bf1942", "Wake", "conquest").unwrap();
    let p = Params { tile: 256, ..Params::infantry(&w) };
    let dir = std::env::temp_dir().join(format!("bytefield-navmesh-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (built, how) = NavMesh::load_or_build(&w, &p, 4, &dir);
    assert_eq!(how, Cached::Built);
    let (read, how) = NavMesh::load_or_build(&w, &p, 4, &dir);
    assert_eq!(how, Cached::Read);
    assert_eq!(built, read);
    // other parameters, another file
    let (_, how) = NavMesh::load_or_build(&w, &Params { climb: 0.3, ..p }, 4, &dir);
    assert_eq!(how, Cached::Built);
    let _ = std::fs::remove_dir_all(&dir);
}

/// El_Alamein with synthetic obstacles around (x, z) and its mesh near there, with the queries over it; and the ground's
/// height at (x, z).
fn with(x: f32, z: f32, parts: impl Fn(f32) -> Vec<Obstacle>) -> (bytefield_sim::navmesh::query::Query, f32) {
    let mut w = bytefield_sim::World::load(&bytefield_testdata::base().unwrap(), "bf1942", "El_Alamein", "conquest").unwrap();
    let g = w.height_at(x as f64, z as f64).unwrap() as f32;
    w.push_obstacles(parts(g));
    let m = NavMesh::build_in(&w, &Params::infantry(&w), 2, Some([x - 40.0, z - 40.0, x + 40.0, z + 80.0]));
    (bytefield_sim::navmesh::query::Query::new(std::sync::Arc::new(m)), g)
}

/// The bridge of [`a_bridge_is_two_floors_joined_by_its_ramps`]: a deck 6 m up from z + 10 to z + 30, ramps at both ends.
fn bridge(x: f32, z: f32, g: f32) -> Vec<Obstacle> {
    vec![
        Obstacle::synthetic_box("deck", [x - 3.0, g + 5.6, z + 10.0], [x + 3.0, g + 6.0, z + 30.0]),
        Obstacle::synthetic_ramp("ramp", [x - 3.0, g - 0.5, z], [x + 3.0, g + 6.0, z + 10.0]),
        ramp_down([x - 3.0, g - 0.5, z + 30.0], [x + 3.0, g + 6.0, z + 40.0]),
    ]
}

#[test]
fn over_the_bridge_and_under_it() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let (x, z) = (1100.0f32, 1100.0f32);
    let (q, g) = with(x, z, |g| bridge(x, z, g));
    let m = &q.mesh;
    // the polygon at the feet: on the deck from the deck, on the ground from under it
    let deck = q.locate([x, g + 6.0, z + 20.0], 1.0, 1.0).expect("the deck");
    let under = q.locate([x, g, z + 20.0], 1.0, 1.0).expect("the ground under it");
    assert!((m.height(deck, x, z + 20.0) - g - 6.0).abs() < 0.3, "{}", m.height(deck, x, z + 20.0) - g);
    assert!((m.height(under, x, z + 20.0) - g).abs() < 0.1 && m.polys[under as usize].ground);
    // across the bridge from the ground before it to the ground after it: up one ramp, over the deck, down the other (the
    // ramps are solid: straight along the bridge, the ground is cut)
    let (_, steps, _) = q.corridor([x, g, z - 10.0], [x, g, z + 50.0]).expect("a way across");
    let heights: Vec<f32> = steps.iter().map(|s| s.mid()[1] - g).collect();
    let top = heights.iter().position(|&h| (h - 6.0).abs() < 0.3).expect("over the deck");
    assert!(heights[..top].iter().any(|&h| h > 1.0 && h < 5.0) && heights[top..].iter().any(|&h| h > 1.0 && h < 5.0), "up a ramp and down the other: {heights:?}");
    let up = q.path([x, g, z - 10.0], [x, g + 6.0, z + 20.0]).expect("a way onto the deck");
    assert!(up.last().is_some_and(|p| (p[1] - g - 6.0).abs() < 0.3), "{up:?}");
    let down = q.path([x, g + 6.0, z + 20.0], [x, g, z + 50.0]).expect("a way off the deck");
    assert!(down.last().is_some_and(|p| (p[1] - g).abs() < 0.5), "{down:?}");
    // under it, from one side to the other: on the ground all the way
    let below = q.path([x - 15.0, g, z + 20.0], [x + 15.0, g, z + 20.0]).expect("a way under the bridge");
    assert!(below.iter().all(|p| p[1] < g + 1.0), "{below:?}");
    assert!(below.len() <= 2, "straight under it: {below:?}");
}

#[test]
fn up_a_ladder_and_down_it() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let (x, z) = (1100.0f32, 1100.0f32);
    // a tower 6 m high with a flat top of 8 × 8 m, a ladder on its south face
    let (q, g) = with(x, z, |g| vec![Obstacle::synthetic_box("tower", [x - 4.0, g - 1.0, z + 10.0], [x + 4.0, g + 6.0, z + 18.0]), Obstacle::synthetic_ladder("ladder", [x - 0.5, g - 0.5, z + 9.7], [x + 0.5, g + 6.0, z + 10.0])]);
    assert!(q.mesh.links.iter().any(|l| l.kind == bytefield_sim::navmesh::LinkKind::Ladder), "the ladder's link");
    let up = q.path([x, g, z], [x, g + 6.0, z + 14.0]).expect("a way up the ladder");
    assert!(up.iter().any(|p| (p[0] - x).abs() < 1.0 && (p[2] - z - 9.85).abs() < 1.0 && p[1] < g + 1.0), "to the ladder's foot: {up:?}");
    assert!(up.last().is_some_and(|p| (p[1] - g - 6.0).abs() < 0.3), "{up:?}");
    let down = q.path([x, g + 6.0, z + 14.0], [x, g, z]).expect("and down it");
    assert!(down.last().is_some_and(|p| (p[1] - g).abs() < 0.5), "{down:?}");
}

#[test]
fn down_a_drop_and_not_up_it() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let (x, z) = (1100.0f32, 1100.0f32);
    // a block 2.5 m high, 8 × 8 m, without steps; and one 6 m high, too high to drop from unharmed
    let (q, g) = with(x, z, |g| vec![Obstacle::synthetic_box("low", [x - 4.0, g - 1.0, z + 10.0], [x + 4.0, g + 2.5, z + 18.0]), Obstacle::synthetic_box("high", [x - 4.0, g - 1.0, z + 30.0], [x + 4.0, g + 6.0, z + 38.0])]);
    let down = q.path([x, g + 2.5, z + 14.0], [x, g, z]).expect("a way down the drop");
    assert!(down.last().is_some_and(|p| (p[1] - g).abs() < 0.5), "{down:?}");
    assert_eq!(q.path([x, g, z], [x, g + 2.5, z + 14.0]), None, "a drop is not climbed");
    assert_eq!(q.path([x, g + 6.0, z + 34.0], [x, g, z]), None, "6 m is no way down");
}

/// In the polygons on the terrain, the height is the terrain's; their vertices' alone was metres off on El_Alamein's dunes.
#[test]
fn the_height_on_the_terrain_is_the_terrain() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let w = bytefield_sim::World::load(&bytefield_testdata::base().unwrap(), "bf1942", "El_Alamein", "conquest").unwrap();
    let m = NavMesh::build_in(&w, &Params::infantry(&w), 4, Some([900.0, 900.0, 1300.0, 1300.0]));
    let (mut n, mut worst, mut fan_worst) = (0, 0.0f32, 0.0f32);
    for p in 0..m.polys.len() as u32 {
        if !m.polys[p as usize].ground || m.area(p) < 50.0 {
            continue;
        }
        // a point inside: halfway from the centre to each vertex
        let c = m.centre(p);
        for v in m.poly_verts(p) {
            let (px, pz) = (0.5 * (c[0] + v[0]), 0.5 * (c[2] + v[2]));
            let t = w.height_at(px as f64, pz as f64).unwrap() as f32;
            worst = worst.max((m.height(p, px, pz) - t).abs());
            n += 1;
        }
        fan_worst = fan_worst.max((m.centre(p)[1] - w.height_at(c[0] as f64, c[2] as f64).unwrap() as f32).abs());
    }
    eprintln!("{n} points in big polygons on the terrain: off by {worst:.3} m at most (the mean of the vertices: {fan_worst:.2} m)");
    assert!(n > 100, "{n}");
    assert!(worst <= 0.1, "{worst}");
}

#[test]
fn off_a_deck_the_start_is_not_the_ground_far_below() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    use bytefield_sim::bots::body::nav::{MeshNav, MeshSearcher};
    use bytefield_sim::bots::body::paths::Answer;
    let (x, z) = (1100.0f32, 1100.0f32);
    let (q, g) = with(x, z, |g| bridge(x, z, g));
    let nav = MeshNav::new(q.mesh.clone());
    // a soldier off the mesh just past the deck's side, 3 m over it (in the air, on a rail): the ground is right under its feet,
    // 9 m down, deeper than a soldier drops (as the river under DC_Operation_Bragg's bridge); the deck, a metre and a half across.
    // Its way to the ground before the bridge starts on the deck, down the ramp
    let feet = [x + 4.5, g + 9.0, z + 20.0];
    let mut s = MeshSearcher::default();
    let answer = s.request(&nav, 1, 0, [feet[0], feet[2]], Some(feet[1]), [x, z - 10.0], &|_| true, false, &[], false);
    let Answer::Path(points) = answer else { panic!("no path: {answer:?}") };
    assert!((points[0][1] - g - 6.0).abs() < 0.5, "it starts on the deck, not on the ground under it: {points:?}");
}
