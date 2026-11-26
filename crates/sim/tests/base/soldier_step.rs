//! What a soldier on foot walks up: the highest step and the steepest ramp, measured by running a soldier of the bench
//! into synthetic boxes and wedges on El_Alamein's desert, and the steepest terrain, on stretches of Wake's. They fix the
//! walkable-surface navigation mesh's step (`climb`) and slopes (`bytefield_sim::navmesh`): the mesh must not join what the
//! soldier cannot walk up. Skipped without the real data.
use bytefield_sim::obstacle::Obstacle;
use bytefield_sim::soldier::axis;

/// A flat stretch of El_Alamein's desert.
const AT: [f32; 2] = [1100.0, 1100.0];

/// Runs a soldier 6 s towards +z into the obstacle `make` places (given the ground height at the start) and returns the
/// highest it got above where it stood and how far along z it ended.
fn run(make: impl Fn(f32) -> Obstacle) -> (f32, f32) {
    let g = bytefield_testdata::base().unwrap();
    let mut w = bytefield_sim::World::load(&g, "bf1942", "El_Alamein", "conquest").unwrap();
    let ground = w.height_at(AT[0] as f64, AT[1] as f64).unwrap() as f32;
    w.push_obstacles(vec![make(ground)]);
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let sol = s.spawn_vehicle("GermanDesertSoldier", [AT[0], ground + 1.6, AT[1]], [0.0; 3]).expect("a soldier");
    for _ in 0..30 {
        s.step();
    }
    let y0 = s.vehicles[sol].craft.body.pos[1];
    let mut up = f32::MIN;
    for _ in 0..180 {
        s.vehicles[sol].soldier_mut().unwrap().set_input(&[(axis::THROTTLE, 1.0)]);
        s.step();
        up = up.max(s.vehicles[sol].craft.body.pos[1] - y0);
    }
    (up, s.vehicles[sol].craft.body.pos[2] - AT[1])
}

/// The highest step (5 cm apart) the soldier walks onto.
fn highest_step() -> f32 {
    let mut best = 0.0;
    for k in 2..=16 {
        let h = k as f32 * 0.05;
        // a block 2 m ahead, 6 m wide and 6 m deep, its top `h` above the ground at the start
        let (up, along) = run(|g| Obstacle::synthetic_box("step", [AT[0] - 3.0, g - 2.0, AT[1] + 2.0], [AT[0] + 3.0, g + h, AT[1] + 8.0]));
        let on = along > 4.0 && up > h - 0.25;
        eprintln!("step {h:.1} m: up {up:.2} m, along {along:.1} m, on top {on}");
        if on {
            best = h;
        }
    }
    best
}

/// The steepest ramp (2° apart) the soldier walks up to its top.
fn steepest_ramp() -> f32 {
    let mut best = 0.0;
    for deg in (30..=56).step_by(2) {
        let a = (deg as f32).to_radians();
        let rise = 3.0;
        let run_len = rise / a.tan();
        // the ramp starts 2 m ahead, a little sunk so there is no lip, and a flat top 4 m deep after it
        let (up, along) = run(|g| {
            let mut r = Obstacle::synthetic_ramp("ramp", [AT[0] - 3.0, g - 0.05, AT[1] + 2.0], [AT[0] + 3.0, g + rise, AT[1] + 2.0 + run_len]);
            let top = Obstacle::synthetic_box("top", [AT[0] - 3.0, g - 2.0, AT[1] + 2.0 + run_len], [AT[0] + 3.0, g + rise, AT[1] + 6.0 + run_len]);
            // one obstacle: the wedge and the block after it
            let base = r.lods[0].vertices.len() as u32;
            let mut verts = r.lods[0].vertices.clone();
            verts.extend_from_slice(&top.lods[0].vertices);
            let mut tris: Vec<[u32; 3]> = r.lods[0].faces.iter().map(|f| f.v).collect();
            tris.extend(top.lods[0].faces.iter().map(|f| f.v.map(|i| i + base)));
            r = Obstacle::synthetic("ramp", verts, &tris);
            r
        });
        let on = up > rise - 0.3;
        eprintln!("ramp {deg}°: up {up:.2} m, along {along:.1} m, on top {on}");
        if on {
            best = deg as f32;
        }
    }
    best
}

#[test]
fn a_soldier_walks_up_steps_and_ramps() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let step = highest_step();
    let ramp = steepest_ramp();
    eprintln!("highest step {step:.2} m, steepest ramp {ramp}°");
    // the navigation mesh is built with these: it must not join what the soldier cannot walk up
    assert!((step - bytefield_sim::navmesh::SOLDIER_STEP).abs() < 1e-3, "the highest step: {step}");
    assert_eq!(ramp, bytefield_sim::navmesh::SOLDIER_RAMP_DEG, "the steepest object ramp");
}

/// The terrain a soldier walks up: on stretches of Wake's terrain 8 m long that rise along +z at one slope (within 6°) and with
/// no obstacle within 3 m, whether a soldier run 3 s up from the foot reaches the top; by slope, (stretches, reached).
fn terrain_slopes() -> std::collections::BTreeMap<i32, (u32, u32)> {
    let g = bytefield_testdata::base().unwrap();
    let w = bytefield_sim::World::load(&g, "bf1942", "Wake", "conquest").unwrap();
    let up = |x: f32, z: f32| w.level.height_and_normal(x as f64, z as f64, bytefield_formats::image::Orientation::IDENTITY);
    let mut out: std::collections::BTreeMap<i32, (u32, u32)> = Default::default();
    let mut starts = Vec::new();
    // a stretch: the slope along z between metres, the same all along, and flat enough across x
    'scan: for zi in (200..1850).step_by(3) {
        for xi in (200..1850).step_by(3) {
            let (x, z) = (xi as f32, zi as f32);
            let hs: Vec<f32> = (0..=8).map(|k| up(x, z + k as f32).map_or(f32::NAN, |h| h.0 as f32)).collect();
            if hs.iter().any(|h| h.is_nan()) || hs[0] < w.level.terrain.water_level as f32 + 1.0 {
                continue;
            }
            let deg: Vec<f32> = hs.windows(2).map(|p| (p[1] - p[0]).atan().to_degrees()).collect();
            let (lo, hi) = deg.iter().fold((f32::MAX, f32::MIN), |a, &d| (a.0.min(d), a.1.max(d)));
            let across = match (up(x + 1.0, z + 4.0), up(x - 1.0, z + 4.0)) {
                (Some(a), Some(b)) => (a.0 - b.0).abs() as f32 / 2.0,
                _ => continue,
            };
            if hi - lo > 6.0 || lo < 24.0 || across > 0.25 {
                continue;
            }
            if w.obstacles().iter().any(|o| (o.center[0] - x).hypot(o.center[2] - (z + 4.0)) < o.radius + 7.0) {
                continue;
            }
            let bin = (((lo + hi) / 2.0) / 4.0).floor() as i32 * 4;
            if out.get(&bin).is_some_and(|c| c.0 >= 6) {
                continue;
            }
            out.entry(bin).or_default().0 += 1;
            starts.push((bin, x, z, hs[0], hs[8]));
            if starts.len() > 60 {
                break 'scan;
            }
        }
    }
    for (bin, x, z, h0, h8) in starts {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        let sol = s.spawn_vehicle("USSoldier", [x, h0 + 1.6, z], [0.0; 3]).expect("a soldier");
        for _ in 0..15 {
            s.step();
        }
        let mut best = f32::MIN;
        for _ in 0..90 {
            s.vehicles[sol].soldier_mut().unwrap().set_input(&[(axis::THROTTLE, 1.0)]);
            s.step();
            best = best.max(s.vehicles[sol].craft.body.pos[2] - z);
        }
        let ok = best >= 7.5;
        eprintln!("terrain {bin}–{}° at {x},{z} ({:.1} m up 8 m): got {best:.1} m along", bin + 4, h8 - h0);
        if ok {
            out.get_mut(&bin).unwrap().1 += 1;
        }
    }
    out
}

#[test]
fn a_soldier_walks_up_terrain() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let by = terrain_slopes();
    eprintln!("terrain by slope (stretches, reached): {by:?}");
    // every stretch up to the mesh's terrain slope is walked up, and there are some of each
    let limit = bytefield_sim::navmesh::SOLDIER_TERRAIN_DEG as i32;
    for lo in (24..limit - 2).step_by(4) {
        assert!(by.get(&lo).is_some_and(|&(n, ok)| n >= 3 && ok == n), "{lo}–{}°: {:?}", lo + 4, by.get(&lo));
    }
    // and the steeper ones are not
    assert!(by.range(limit + 2..).all(|(_, &(_, ok))| ok == 0), "{by:?}");
}

/// Runs a soldier 6 s towards +z off the edge of a block whose top is `h` above the ground (the edge 2 m ahead) and returns
/// its hit points before and after, and whether it ended on the ground below.
fn drop_from(h: f32) -> (f32, f32, bool) {
    let g = bytefield_testdata::base().unwrap();
    let mut w = bytefield_sim::World::load(&g, "bf1942", "El_Alamein", "conquest").unwrap();
    let ground = w.height_at(AT[0] as f64, AT[1] as f64).unwrap() as f32;
    w.push_obstacles(vec![Obstacle::synthetic_box("ledge", [AT[0] - 3.0, ground - 2.0, AT[1] - 4.0], [AT[0] + 3.0, ground + h, AT[1] + 2.0])]);
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let sol = s.spawn_vehicle("GermanDesertSoldier", [AT[0], ground + h + 1.6, AT[1]], [0.0; 3]).expect("a soldier");
    for _ in 0..30 {
        s.step();
    }
    let hp = |s: &bytefield_sim::Sim| s.vehicles[sol].armor.as_ref().map_or(f32::NAN, |a| a.hp);
    let before = hp(&s);
    let y0 = s.vehicles[sol].craft.body.pos[1];
    for _ in 0..180 {
        if s.vehicles[sol].removed || s.vehicles[sol].soldier().is_none() {
            break;
        }
        s.vehicles[sol].soldier_mut().unwrap().set_input(&[(axis::THROTTLE, 1.0)]);
        s.step();
    }
    let p = s.vehicles[sol].craft.body.pos;
    (before, hp(&s), p[2] > AT[1] + 2.5 && p[1] < y0 - h + 0.3)
}

/// The highest drop (half a metre apart) the soldier takes without losing any health, and losing under a third of it: the
/// navigation mesh's links down a ledge go no deeper than the second, and cost more beyond the first.
#[test]
fn a_soldier_drops_off_ledges() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let (mut unhurt, mut bearable) = (0.5, 0.5);
    for k in 2..=20 {
        let h = k as f32 * 0.5;
        let (before, after, down) = drop_from(h);
        eprintln!("drop {h:.1} m: hit points {before:.1} -> {after:.1}, down {down}");
        if down && after >= before - 1e-3 && unhurt == h - 0.5 {
            unhurt = h;
        }
        if down && after > before * 2.0 / 3.0 && bearable == h - 0.5 {
            bearable = h;
        }
    }
    eprintln!("highest drop without harm {unhurt:.1} m, losing under a third of the health {bearable:.1} m");
    assert_eq!(unhurt, bytefield_sim::navmesh::SOLDIER_DROP, "the highest harmless drop");
    assert_eq!(bearable, bytefield_sim::navmesh::SOLDIER_DROP_MAX, "the highest drop losing under a third of the health");
}

/// Runs a soldier 8 s towards +z into a ladder 6 m high on the south face of a tower (the ladder 2 m ahead), looking up by
/// `pitch` (radians) or down, and returns the highest its feet got and whether it ever climbed.
fn into_ladder(look: f32) -> (f32, bool) {
    let g = bytefield_testdata::base().unwrap();
    let mut w = bytefield_sim::World::load(&g, "bf1942", "El_Alamein", "conquest").unwrap();
    let ground = w.height_at(AT[0] as f64, AT[1] as f64).unwrap() as f32;
    w.push_obstacles(vec![
        Obstacle::synthetic_box("tower", [AT[0] - 4.0, ground - 1.0, AT[1] + 2.3], [AT[0] + 4.0, ground + 6.0, AT[1] + 10.0]),
        Obstacle::synthetic_ladder("ladder", [AT[0] - 0.5, ground - 0.5, AT[1] + 2.0], [AT[0] + 0.5, ground + 6.0, AT[1] + 2.3]),
    ]);
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let sol = s.spawn_vehicle("GermanDesertSoldier", [AT[0], ground + 1.6, AT[1]], [0.0; 3]).expect("a soldier");
    for _ in 0..30 {
        s.step();
    }
    let y0 = s.vehicles[sol].craft.body.pos[1];
    let (mut up, mut climbed) = (f32::MIN, false);
    for _ in 0..240 {
        let m = s.vehicles[sol].soldier_mut().unwrap();
        m.pitch = look;
        m.set_input(&[(axis::THROTTLE, 1.0)]);
        s.step();
        climbed |= s.vehicles[sol].soldier().unwrap().climbing();
        up = up.max(s.vehicles[sol].craft.body.pos[1] - y0);
    }
    (up, climbed)
}

/// A soldier pushing into a ladder climbs it, looking up or level.
#[test]
fn a_soldier_climbs_a_ladder() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    for look in [0.0f32, 0.4] {
        let (up, climbed) = into_ladder(look);
        eprintln!("into a ladder looking {look}: up {up:.2} m, climbed {climbed}");
        assert!(climbed && up > 5.5, "looking {look}: up {up:.2} m, climbed {climbed}");
    }
}

/// Runs a soldier 6 s towards +z, jumping all the way, into `obstacles` (given the ground height at the start; the soldier at
/// `AT`) and returns the highest its body got above where it stood and how far along z it ended.
fn run_jumping(obstacles: impl Fn(f32) -> Vec<Obstacle>) -> (f32, f32) {
    run_among(obstacles, true)
}

/// [`run_jumping`], jumping or not.
fn run_among(obstacles: impl Fn(f32) -> Vec<Obstacle>, jump: bool) -> (f32, f32) {
    let g = bytefield_testdata::base().unwrap();
    let mut w = bytefield_sim::World::load(&g, "bf1942", "El_Alamein", "conquest").unwrap();
    let ground = w.height_at(AT[0] as f64, AT[1] as f64).unwrap() as f32;
    w.push_obstacles(obstacles(ground));
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let sol = s.spawn_vehicle("GermanDesertSoldier", [AT[0], ground + 1.6, AT[1]], [0.0; 3]).expect("a soldier");
    for _ in 0..30 {
        s.step();
    }
    let y0 = s.vehicles[sol].craft.body.pos[1];
    let mut up = f32::MIN;
    for _ in 0..180 {
        s.vehicles[sol].soldier_mut().unwrap().set_input(&[(axis::THROTTLE, 1.0), (axis::ACTION, if jump { 1.0 } else { 0.0 })]);
        s.step();
        up = up.max(s.vehicles[sol].craft.body.pos[1] - y0);
    }
    (up, s.vehicles[sol].craft.body.pos[2] - AT[1])
}

/// The highest ledge (5 cm apart) the soldier gets onto running and jumping.
fn highest_jump() -> f32 {
    let mut best = 0.0;
    for k in 8..=40 {
        let h = k as f32 * 0.05;
        let (up, along) = run_jumping(|g| vec![Obstacle::synthetic_box("ledge", [AT[0] - 3.0, g - 2.0, AT[1] + 2.0], [AT[0] + 3.0, g + h, AT[1] + 8.0])]);
        let on = along > 4.0 && up > h - 0.25;
        eprintln!("jump onto {h:.2} m: up {up:.2} m, along {along:.1} m, on top {on}");
        if on && (k == 8 || (best - (h - 0.05)).abs() < 1e-3) {
            best = h;
        }
    }
    best
}

/// Whether the soldier, running and jumping, gets through a window `gap` high whose sill is `sill` above the ground, in a
/// wall 0.3 m thick (the window 1.2 m wide), to land 2 m past it.
fn through_window(sill: f32, gap: f32) -> bool {
    let (x, z) = (AT[0], AT[1] + 2.0);
    let (_, along) = run_jumping(|g| {
        vec![
            Obstacle::synthetic_box("left", [x - 4.0, g - 1.0, z], [x - 0.6, g + 4.0, z + 0.3]),
            Obstacle::synthetic_box("right", [x + 0.6, g - 1.0, z], [x + 4.0, g + 4.0, z + 0.3]),
            Obstacle::synthetic_box("sill", [x - 0.6, g - 1.0, z], [x + 0.6, g + sill, z + 0.3]),
            Obstacle::synthetic_box("lintel", [x - 0.6, g + sill + gap, z], [x + 0.6, g + 4.0, z + 0.3]),
        ]
    });
    along > 4.3
}

/// What a soldier jumps: the highest ledge it gets onto running and jumping, and the windows it gets through (Berlin's sealed
/// house at 1781,1962 is left by a window with a 0.8 m sill and a 1.6 m gap). They fix the navigation mesh's jump links.
#[test]
fn a_soldier_jumps_onto_ledges_and_through_windows() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let h = highest_jump();
    eprintln!("highest ledge jumped onto {h:.2} m");
    assert!((h - bytefield_sim::navmesh::SOLDIER_JUMP).abs() < 1e-3, "the highest ledge jumped onto: {h:.2} m");
    // the lowest gap it gets through, for each sill
    for sill in [0.5f32, 0.8, 1.0, 1.2, 1.4] {
        let gaps: Vec<f32> = (0..6).map(|k| 1.1 + 0.1 * k as f32).filter(|&g| through_window(sill, g)).collect();
        eprintln!("window sill {sill} m: through gaps {gaps:?}");
        let least = gaps.first().copied().unwrap_or(f32::MAX);
        assert!(least <= bytefield_sim::navmesh::SOLDIER_JUMP_GAP + 1e-3 && least > 1.3, "sill {sill} m: the least gap {least}");
    }
}

/// Whether the soldier, walking (no jump), gets through a door `width` wide and `height` high in a wall 0.3 m thick, to 2 m past it.
fn through_door(width: f32, height: f32) -> bool {
    let (x, z) = (AT[0], AT[1] + 2.0);
    let (_, along) = run_among(
        |g| {
            vec![
                Obstacle::synthetic_box("left", [x - 4.0, g - 1.0, z], [x - width / 2.0, g + 4.0, z + 0.3]),
                Obstacle::synthetic_box("right", [x + width / 2.0, g - 1.0, z], [x + 4.0, g + 4.0, z + 0.3]),
                Obstacle::synthetic_box("lintel", [x - width / 2.0, g + height, z], [x + width / 2.0, g + 4.0, z + 0.3]),
            ]
        },
        false,
    );
    along > 4.3
}

/// The narrowest door a soldier walks through: the navigation mesh joins what is that wide or wider, though its erosion
/// (the walker's radius) shuts the narrower ones of them.
#[test]
fn a_soldier_walks_through_doors() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let widths: Vec<f32> = (0..9).map(|k| 0.4 + 0.1 * k as f32).filter(|&w| through_door(w, 2.2)).collect();
    eprintln!("doors 2.2 m high walked through: {widths:?}");
    let least = widths.first().copied().unwrap_or(f32::MAX);
    assert!((least - bytefield_sim::navmesh::SOLDIER_DOOR).abs() < 0.05, "the narrowest door: {least}");
    let heights: Vec<f32> = (0..12).map(|k| 0.8 + 0.1 * k as f32).filter(|&h| through_door(1.2, h)).collect();
    eprintln!("doors 1.2 m wide walked through: {heights:?}");
    let least = heights.first().copied().unwrap_or(f32::MAX);
    assert!((least - bytefield_sim::navmesh::SOLDIER_DOOR_HIGH).abs() < 0.05, "the lowest door: {least}");
}
