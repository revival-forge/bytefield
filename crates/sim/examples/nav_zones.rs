//! Which zone of bots3's path grids (infantry and tank, `bots3::body::paths::Grid`) each point falls in: two points in different
//! zones have no way between them, whatever the search does. For the census of the maps where bots3 does not play (why a squad
//! never gets to a flag):
//!   nav_zones <game> <mod> <level> x,z [x,z...]
//! One line per point: `x,z infantry <zone> (the zones within 15 and 30 m) tank …` (0: a blocked cell; `-`: outside the map), then
//! each grid's biggest zones. `NAV_WAY_TO=x,z,r`: the way on foot from each point to that circle ([`Grid::distances_to`]).
use bytefield_sim::bot_api::Nav;
use bytefield_sim::bots3::body::paths::Grid;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("nav_zones <game> <mod> <level> x,z [x,z...]");
        std::process::exit(2);
    }
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let grids: Vec<(&str, Option<Grid>)> = [("infantry", Nav::Infantry), ("tank", Nav::Tank)].into_iter().map(|(n, k)| (n, w.nav_map(k).map(|m| Grid::from_map(&m)))).collect();
    // (`NAV_WAY_TO=x,z,r`: also the way on foot from each point to that circle, m, and how long building its field took)
    let way = std::env::var("NAV_WAY_TO").ok().map(|w| w.split(',').map(|x| x.parse::<f32>().expect("x,z,r")).collect::<Vec<f32>>()).and_then(|w| {
        let g = grids[0].1.as_ref()?;
        let t = std::time::Instant::now();
        let f = g.distances_to(w[0], w[1], w[2]);
        println!("field to {w:?}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        Some(f)
    });
    for p in &a[4..] {
        let xz: Vec<f32> = p.split(',').map(|x| x.parse().expect("x,z")).collect();
        let zones: Vec<String> = grids
            .iter()
            .map(|(n, g)| {
                let Some(g) = g else { return format!("{n} -") };
                let at = g.zone_at(xz[0], xz[1]).map_or("-".into(), |z| z.to_string());
                format!("{n} {at} (within 15 m {:?}, 30 m {:?})", g.zones_near(xz[0], xz[1], 15.0), g.zones_near(xz[0], xz[1], 30.0))
            })
            .collect();
        let w = way.as_ref().and_then(|f| grids[0].1.as_ref()?.way_in(f, xz[0], xz[1])).map_or(String::new(), |m| format!(" way {m:.0} m"));
        println!("{p} {}{w}", zones.join(" "));
    }
    for (n, g) in &grids {
        let Some(g) = g else { continue };
        let mut size = std::collections::BTreeMap::<u32, usize>::new();
        for (z, b) in g.zone.iter().zip(&g.blocked) {
            if !b {
                *size.entry(*z).or_default() += 1;
            }
        }
        let mut big: Vec<_> = size.into_iter().collect();
        big.sort_by(|x, y| y.1.cmp(&x.1));
        println!("{n}: {} zones, the biggest (zone, cells): {:?}", big.len(), &big[..big.len().min(6)]);
    }
}
