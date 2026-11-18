//! Whether a pathfinding map parses, and its size: `cargo run --release -p bytefield-formats --example pathmap -- <map.raw>...`
fn main() {
    for path in std::env::args().skip(1) {
        let b = std::fs::read(&path).expect("read");
        match bytefield_formats::pathmap::PathMap::parse(&b) {
            Ok(m) => println!("{path}: {}x{} at {} bpp", m.width, m.height, m.bpp),
            Err(e) => println!("{path}: {e:?}"),
        }
    }
}
