//! The dedicated server: a round of the bench at 30 Hz on the real clock, its
//! players over UDP.
//!
//!     bytefield-server <game_dir> <mod> <level> [--mode conquest] [--port 14567] [--bots N] [--seconds S]
//!                   [--start-delay 20] [--min-players N] [--friendly-fire 100] [--kickback 0] [--no-nose-cam]
//!                   [--welcome LINE]... [--snapshots-per-second 30|15|10] [--packet-bytes 1200]
//!
//! Each round starts with a pre-game (`--start-delay`, the game's `serverGameStartDelay`); in
//! conquest without bots it waits for 2 people (`--min-players`): until then it plays, and then
//! the map starts again, as the original's. `--friendly-fire` is CREATE GAME's slider (the soldiers'
//! and the vehicles' share, in %); `--kickback` is `serverKickBack` and `…OnSplash` (the share an attacker takes
//! back, in %; CREATE GAME has no control for it); `--no-nose-cam` is `serverAllowNoseCam 0`; each `--welcome`
//! is a `game.setServerWelcomeMessage` line, the briefing's COMMENTS.

use bytefield_server::udp::serve;
use bytefield_server::{Server, round};
use bytefield_server::udp::{PORT, UdpServer};
use bytefield_sim::World;

/// Whether this process is alone in its console: Windows made the console for it, as on a double click in Explorer.
#[cfg(windows)]
fn own_console() -> bool {
    let mut ids = [0u32; 2];
    // SAFETY: the buffer and its length match; the call only writes into it.
    unsafe { windows_sys::Win32::System::Console::GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) == 1 }
}

#[cfg(not(windows))]
fn own_console() -> bool {
    false
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        // Opened with a double click: say what this is before the window closes.
        if own_console() {
            println!("bytefield-server is a dedicated server, run from a terminal with the game folder, a mod and a map:");
            println!("  .\\bytefield-server \"C:\\Games\\Battlefield 1942\" bf1942 El_Alamein --bots 16");
            println!("To play, or to host a game for friends, open bytefield.exe (CREATE GAME). All the options:\n");
        }
        eprintln!("usage: bytefield-server <game_dir> <mod> <level> [--mode conquest] [--port {PORT}] [--bots N] (default bots) [--seconds S] [--max-players N] [--start-delay S] [--min-players N] [--friendly-fire PERCENT] [--kickback PERCENT] [--no-nose-cam] [--welcome LINE]... [--snapshots-per-second 30|15|10] [--packet-bytes 1200]");
        if own_console() {
            println!("\nPress Enter to close.");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        std::process::exit(2);
    }
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let num = |k: &str| opt(k).map(|v| v.parse::<u64>().unwrap_or_else(|_| panic!("{k}: not a number: {v}")));
    let mode = opt("--mode").unwrap_or_else(|| "conquest".into());
    let port = num("--port").unwrap_or(PORT as u64) as u16;
    let bots = num("--bots").unwrap_or(0) as usize;
    let seconds = num("--seconds");
    let world: &'static World = Box::leak(Box::new(World::load(a[0].as_ref(), &a[1], &a[2], &mode).expect("level")));
    let better = false;
    // the bots play unless `--bot-ai` asks for another AI
    bytefield_sim::bots::use_by_default(opt("--bot-ai").is_none_or(|v| bytefield_sim::bots::is_name(&v)));
    let mut sim = round(world, bots, better);
    // the mod's `ServerSettings.con` (else the game's): its rules and pre-game; then the options
    let con = bytefield_sim::damage::server_settings_file(a[0].as_ref(), &a[1]).unwrap_or_default();
    sim.rules.server = sim.rules.server.with_con(&con);
    let con_delay = bytefield_sim::damage::server_settings(&con).into_iter().find(|(n, _)| n == "servergamestartdelay").and_then(|(_, v)| v.parse::<f32>().ok());
    let start_delay = opt("--start-delay").map_or(con_delay.unwrap_or(bytefield_server::START_DELAY), |v| v.parse().unwrap_or_else(|_| panic!("--start-delay: not a number: {v}")));
    bytefield_server::hosted(&mut sim, &mode, bots, start_delay, num("--min-players").map(|n| n as u32));
    if let Some(ff) = num("--friendly-fire") {
        sim.rules.server = sim.rules.server.with_friendly_fire(ff as f32);
    }
    if let Some(k) = num("--kickback") {
        sim.rules.server = sim.rules.server.with_kickback(k as f32);
    }
    if a.iter().any(|x| x == "--no-nose-cam") {
        sim.rules.server.nose_cam = false;
    }
    sim.rules.welcome = a.windows(2).filter(|w| w[0] == "--welcome").map(|w| w[1].clone()).collect();
    let comments = sim.rules.welcome.clone();
    let mut server = Server::new(sim);
    let mut udp = UdpServer::bind(("0.0.0.0", port), &a[1], &a[2], &mode).expect("bind");
    udp.comments = comments;
    if let Some(n) = num("--max-players") {
        udp.most_clients = n as usize;
    }
    if let Some(r) = num("--snapshots-per-second") {
        assert!(matches!(r, 30 | 15 | 10), "--snapshots-per-second: 30, 15 or 10 (a step of 30 Hz is the unit)");
        server.every = 30 / r;
    }
    if let Some(b) = num("--packet-bytes") {
        assert!((300..=1400).contains(&b), "--packet-bytes: from 300 to 1400 (more would be split on the way)");
        server.budget = Some(b as usize);
    }
    eprintln!("{} {} ({mode}): {} players, on {}", a[1], a[2], server.sim.players.len(), udp.local_addr());
    serve(&mut server, &mut udp, seconds.map(|s| s * 30));
}

