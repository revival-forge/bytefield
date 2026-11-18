//! Which levels ship pathfinding maps for the bots:  map_census <game> <mod>...
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let game = std::path::Path::new(&a[0]);
    for m in &a[1..] {
        let Ok((_, vfs)) = bytefield_con::session::mount(game, m) else {
            println!("{m}: does not mount");
            continue;
        };
        let names: Vec<String> = vfs.list().iter().map(|p| bytefield_vfs::normalize(p)).collect();
        let (mut with, mut without) = (Vec::new(), Vec::new());
        for (level, _) in bytefield_con::session::levels(&vfs) {
            let pre = format!("bf1942/levels/{}/pathfinding/", level.to_ascii_lowercase());
            if names.iter().any(|n| n.starts_with(&pre) && n.ends_with("level0map.raw")) {
                with.push(level);
            } else {
                without.push(level);
            }
        }
        println!("{m}: {} with maps, {} without: {}", with.len(), without.len(), without.join(" "));
    }
}
