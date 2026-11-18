//! Why a `.skn` does not fit its mesh: skin_fit <game> <mod> <level> <mesh> <skn> <ske>
use bytefield_formats::{
    anim::{Skeleton, Skin},
    sm,
};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let mesh = sm::parse(&w.vfs.read(&format!("standardmesh/{}.sm", a[4])).expect("sm")).expect("sm");
    let skin = Skin::parse(&w.vfs.read(&a[5]).expect("skn")).expect("skn");
    let ske = Skeleton::parse(&w.vfs.read(&a[6]).expect("ske")).expect("ske");
    println!("mesh {:?} / {:?}", w.vfs.origin(&format!("standardmesh/{}.sm", a[4])), w.vfs.origin(&a[5]));
    println!(
        "skn: {} vertices, {} bones; with no bone in the skeleton: {:?}",
        skin.vertices.len(),
        skin.bones.len(),
        skin.bones.iter().filter(|b| ske.index(b).is_none()).collect::<Vec<_>>()
    );
    println!("skeleton bones: {:?}", ske.bones.iter().take(4).map(|b| b.name.as_str()).collect::<Vec<_>>());
    println!("skn bones: {:?}", skin.bones);
    for (b, name) in skin.bones.iter().enumerate().filter(|(_, n)| ske.index(n).is_none()) {
        let uses: Vec<(f32, [f32; 3])> = skin.vertices.iter().flat_map(|(_, inf)| inf.iter().filter(|i| i.bone as usize == b).map(|i| (i.weight, i.offset))).collect();
        let only_it = skin.vertices.iter().filter(|(_, inf)| inf.len() == 1 && inf[0].bone as usize == b).count();
        let (mi, ma) = uses.iter().fold((f32::MAX, f32::MIN), |m, u| (m.0.min(u.0), m.1.max(u.0)));
        println!(
            "{name}: {} influences, weight {mi:.3}..{ma:.3}, {only_it} vertices on it alone; first: {:?}",
            uses.len(),
            &uses[..uses.len().min(3)]
        );
    }
    for (k, l) in mesh.lods.iter().enumerate() {
        let (mut n, mut off, mut worst) = (0, 0, 0f32);
        for m in &l.materials {
            for p in &m.positions {
                let d = skin.vertices.iter().map(|(q, _)| (0..3).map(|c| (q[c] - p[c]).abs()).fold(0.0f32, f32::max)).fold(f32::MAX, f32::min);
                n += 1;
                if d >= 1e-4 {
                    off += 1;
                }
                worst = worst.max(d);
            }
        }
        println!("LOD {k}: {} materials, {n} vertices, {off} with no skn vertex, the worst at {worst:.4}", l.materials.len());
    }
}
