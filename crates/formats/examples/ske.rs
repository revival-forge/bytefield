//! Dumps a skeleton (`.ske`): each bone, its parent and its bind matrix (rows, position).
//!     cargo run --release -p bytefield-formats --example ske -- data/extracted/anim/animations/No4.ske

fn main() {
    let path = std::env::args().nth(1).expect("usage: ske <file.ske>");
    let s = bytefield_formats::anim::Skeleton::parse(&std::fs::read(&path).expect("read")).expect("parse");
    for (i, b) in s.bones.iter().enumerate() {
        let parent = b.parent.map_or("-".to_string(), |p| s.bones[p].name.clone());
        println!("{i:3} {:24} parent {:24} pos {:?} rows {:?}", b.name, parent, b.pos, b.rows);
    }
}
