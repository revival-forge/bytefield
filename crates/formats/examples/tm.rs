//! A `.tm`'s blocks and the spread of its vertex colours: `cargo run --release -p bytefield-formats --example tm -- <file.tm>`
fn main() {
    let path = std::env::args().nth(1).expect("usage: tm <file.tm>");
    let t = bytefield_formats::tm::parse(&std::fs::read(&path).expect("read")).expect("parse");
    println!("{} vertices; branches {:?}", t.positions.len(), t.branches.iter().map(|b| &b.texture).collect::<Vec<_>>());
    println!("trunks {:?}; sprites {:?}", t.trunks.iter().map(|b| &b.texture).collect::<Vec<_>>(), t.sprites.iter().map(|b| &b.texture).collect::<Vec<_>>());
    let n = t.colours.len().max(1) as f64;
    let mean: Vec<f64> = (0..4).map(|i| t.colours.iter().map(|c| c[i] as f64).sum::<f64>() / n).collect();
    let min: Vec<u8> = (0..4).map(|i| t.colours.iter().map(|c| c[i]).min().unwrap_or(0)).collect();
    let max: Vec<u8> = (0..4).map(|i| t.colours.iter().map(|c| c[i]).max().unwrap_or(0)).collect();
    println!("colour (B, G, R, A) mean {mean:.0?} min {min:?} max {max:?}");
}
