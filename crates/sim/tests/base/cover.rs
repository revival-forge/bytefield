//! The cover points (`bytefield_sim::cover`) on synthetic walls in El_Alamein's open desert: behind a wall taller than a man the
//! points hide standing towards it, behind a low one only crouched (a parapet), the open field gives none, and the points do not
//! depend on the threads. Skipped without the real data.
use bytefield_sim::cover::{CoverPoints, sector};
use bytefield_sim::navmesh::{NavMesh, Params};
use bytefield_sim::obstacle::Obstacle;

#[test]
fn walls_give_cover_points_and_the_field_none() {
    let game = bytefield_testdata::require!(Base);
    let (x, z) = (1100.0f32, 1100.0f32);
    let mut w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
    let g = w.height_at(x as f64, z as f64).unwrap() as f32;
    // a wall 2.5 m high, 8 m long across x, at z + 5; a low one, 1.1 m, at x + 20
    w.push_obstacles(vec![
        Obstacle::synthetic_box("wall", [x - 4.0, g - 0.5, z + 5.0], [x + 4.0, g + 2.5, z + 5.4]),
        Obstacle::synthetic_box("parapet", [x + 16.0, g - 0.5, z + 5.0], [x + 24.0, g + 1.1, z + 5.4]),
    ]);
    let m = NavMesh::build_in(&w, &Params::infantry(&w), 2, Some([x - 30.0, z - 30.0, x + 50.0, z + 40.0]));
    let c = CoverPoints::build(&w, &m, 2);
    let two = CoverPoints::build(&w, &m, 5);
    assert_eq!(c.points, two.points, "the points depend on the threads");
    // in front of the walls (south, z < z + 5), the +z sector; behind them, the -z one
    let (north, south) = (sector(0.0, 1.0), sector(0.0, -1.0));
    let by = |x0: f32, x1: f32, front: bool| c.points.iter().filter(move |p| p.pos[0] > x0 && p.pos[0] < x1 && if front { p.pos[2] < z + 5.0 } else { p.pos[2] > z + 5.4 }).collect::<Vec<_>>();
    let wall_front = by(x - 3.0, x + 3.0, true);
    let wall_back = by(x - 3.0, x + 3.0, false);
    assert!(wall_front.len() >= 2 && wall_back.len() >= 2, "too few points by the wall: {} in front, {} behind", wall_front.len(), wall_back.len());
    assert!(wall_front.iter().all(|p| p.crouch & p.stand & (1 << north) != 0), "the wall does not hide standing from the north: {wall_front:?}");
    assert!(wall_back.iter().all(|p| p.crouch & p.stand & (1 << south) != 0), "the wall does not hide standing from the south: {wall_back:?}");
    let low = by(x + 17.0, x + 23.0, true);
    assert!(low.len() >= 2, "too few points by the parapet: {low:?}");
    assert!(low.iter().all(|p| p.crouch & (1 << north) != 0 && p.stand & (1 << north) == 0), "the parapet is not one: {low:?}");
    assert!(low.iter().all(|p| p.hides_crouched_from([x + 20.0, g, z + 60.0]) && !p.hides_standing_from([x + 20.0, g, z + 60.0])));
    // nothing more than 3 m from either wall (the edges of the built area are open field)
    let far: Vec<_> = c.points.iter().filter(|p| (p.pos[2] - z - 5.2).abs() > 3.0 || !((x - 7.0..x + 7.0).contains(&p.pos[0]) || (x + 13.0..x + 27.0).contains(&p.pos[0]))).collect();
    assert!(far.is_empty(), "cover points in the open: {far:?}");
}
