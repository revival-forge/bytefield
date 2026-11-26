//! A mod's files as the engine sees them (archives and loose files, later mounts on top):
//! `vfs_get <game> <mod> <substring>` lists the matching paths with their origin;
//! `vfs_get <game> <mod> <path> <out>` writes that file to <out> (e.g. a `.sm` for `smdump`).
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    assert!(a.len() >= 4, "vfs_get <game> <mod> <substring> | vfs_get <game> <mod> <path> <out>");
    let mut vfs = bytefield_vfs::Vfs::new();
    vfs.mount_mod(&Path::new(&a[1]).join("Mods").join(&a[2])).expect("mod");
    if let Some(out) = a.get(4) {
        std::fs::write(out, vfs.read(&a[3]).expect("read")).expect("write");
        return;
    }
    let needle = a[3].to_lowercase();
    for p in vfs.list().into_iter().filter(|p| p.contains(&needle)) {
        println!("{p}\t{}", vfs.origin(p).unwrap_or_default());
    }
}
