//! What one entry of `Sim::vehicles` costs on the heap: a counting
//! allocator around `Sim::instantiate` of each template, `n` times, and the bytes per entry; then the
//! same entries retired (`Sim::retire`) and what stays.
//! `cargo run --release -p bytefield-sim --example entry_heap -- data/game bf1942 Berlin [n] [template...]`
//! (no templates: the two soldiers and the level's first vehicles)
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        LIVE.fetch_add(n as isize - l.size() as isize, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let n: usize = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(50);
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.start_conquest();
    let mut templates: Vec<String> = a.iter().skip(5).cloned().collect();
    // a soldier as `spawn_player` makes it: the team's skin with its first kit (`soldier:<team>`)
    if templates.is_empty() {
        templates.push("soldier:1".into());
        templates.push("soldier:2".into());
        let mut seen = std::collections::BTreeSet::new();
        for v in &s.vehicles {
            if seen.insert(v.template.clone()) && seen.len() <= 6 {
                templates.push(v.template.clone());
            }
        }
    }
    // the vector's own growth out of the measure
    s.vehicles.reserve(templates.len() * n + 16);
    println!("{} entries at the start, {:.1} MB live", s.vehicles.len(), live() as f64 / 1e6);
    for t in &templates {
        let before = live();
        let first = s.vehicles.len();
        for k in 0..n {
            let at = [100.0 + k as f32, 200.0, 100.0];
            if let Some(team) = t.strip_prefix("soldier:").and_then(|x| x.parse::<usize>().ok()) {
                let c = &s.modes.conquest.as_ref().unwrap().teams;
                let skin = c.skins[team - 1].clone().unwrap_or_else(|| bytefield_sim::replica::soldier_template(team as u32).into());
                let kit = c.kits[team - 1].first().cloned().flatten();
                let Some(i) = s.spawn_vehicle(&skin, at, [0.0; 3]) else { break };
                if let Some(kit) = kit {
                    let l0 = live();
                    let ws = s.world.kit_weapons(&kit);
                    s.vehicles[i].give_kit(ws, None);
                    let l1 = live();
                    let parts = s.world.kit_parts(&kit);
                    if k == 0 {
                        println!("  {skin} kit {kit}: weapons {:.1} KB, parts {:.1} KB", (l1 - l0) as f64 / 1e3, (live() - l1) as f64 / 1e3);
                    }
                    if let Some(m) = s.vehicles[i].soldier_mut() {
                        m.kit_parts = parts;
                    }
                }
            } else {
                s.instantiate(t, at, bytefield_sim::craft::ypr([0.0; 3]));
            }
        }
        let made = s.vehicles.len() - first;
        if made == 0 {
            println!("{t}: not made");
            continue;
        }
        let per = (live() - before) as f64 / made as f64;
        let mid = live();
        for i in first..s.vehicles.len() {
            s.retire(i);
        }
        let kept = (live() - before) as f64 / made as f64;
        println!("{t}: {made} made, {:.1} KB each; retired, {:.1} KB each stay (freed {:.1} MB)", per / 1e3, kept / 1e3, (mid - live()) as f64 / 1e6);
    }
}
