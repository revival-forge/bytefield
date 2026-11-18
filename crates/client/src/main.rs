//! `bytefield`: the client. Loads a level through the same core as the headless tools, runs the
//! simulation (`bytefield-sim`) at 30 Hz and puts the player in it as a soldier of its team.
//!
//!     bytefield <game_dir> <mod> <level> [--mode conquest] [--team 1|2] [--kit N] [--third-person]
//!            [--bots N] (default bots3) [--sens X] [--invert-y] [--script "0-60:w;70:e"] [--screenshot out.png] [--shot-tick N] [--chase-yaw D]
//!     bytefield <game_dir> <mod> <level> --fly [--at x,y,z] [--look x,y,z] [--screenshot out.png]
//!     bytefield <game_dir> <mod> <level> --host [--port 14567] [--bots N] [play options]
//!     bytefield <game_dir> --connect host[:port] [--team 1|2] [--kit N] [play options]
//!     bytefield <game_dir> [<mod>] [--menu create[,start]] [--lang Spanish|en|fr|...]
//!
//! Controls: see `play` (WASD, mouse, E to get in and out, C camera). `--fly`: the old viewer,
//! no simulation: right mouse button + mouse to look, WASD to move, Q/E down/up, Shift faster.
//! With --screenshot it saves the picture at a tick (`--shot-tick`, or after the script; the
//! viewer: frame 30) and exits, for checks without a person in front of the screen; with a
//! script the simulation runs one tick per frame (after a warm-up); `--chase-yaw` turns the
//! third-person camera about the player to look at it (90: from its right).
//!
//! Without a level it opens the menu (`--menu create`: on the create-game page, `,start`:
//! and starts, for checks). The texts are the lexicon's in `--lang` (or `BYTEFIELD_LANG`; Spanish).
//!
//! On a server: `--host` runs one in this process (`bytefield-server`'s round, with the
//! bots) and plays on it; `--connect` plays on another's (the level is the server's).

mod artview;
mod assets;
mod backdrop;
mod bmfont;
mod controls;
mod debug;
mod effects;
mod envmap;
mod first_screen;
mod flags;
mod gauges;
mod submarine;
mod graphics;
mod hud;
mod lang;
mod level;
mod lights;
mod menu;
mod minimap;
mod model;
mod movie;
mod play;
mod briefing;
mod respawn;
mod settings;
mod shadows;
mod skin;
mod sound;
mod replay;
mod tickets;
mod userlog;
mod weapon;

use std::path::PathBuf;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PresentMode;
use bytefield_sim::World;

struct Args {
    game: PathBuf,
    mod_name: String,
    level: String,
    mode: String,
    fly: bool,
    at: Option<Vec3>,
    look: Option<Vec3>,
    team: u32,
    kit: usize,
    /// the first spawn waits for the player's choice: `--choose` yes, `--kit` no (None: unless
    /// a script or a screenshot)
    choose: Option<bool>,
    third_person: bool,
    bots: usize,
    /// `--friendly-fire <%>` (CREATE GAME's slider), `--kickback <%>` and `--no-nose-cam`: the rules of a round of one's own or hosted
    rules: bytefield_sim::damage::ServerRules,
    /// `--welcome <line>`, again for each line: `game.setServerWelcomeMessage`, the briefing's COMMENTS of a round of one's own or hosted
    welcome: Vec<String>,
    /// (None: the saved options')
    sens: Option<f32>,
    lang: Option<String>,
    invert_y: bool,
    chase_yaw: f32,
    script: Option<Vec<play::Segment>>,
    /// the command line (the game's folder found on its own included), without a check's arguments: for a ticket's replay
    argv: Vec<String>,
    /// `--replay <ticket>`: the tick and hash to check (`replay.rs`)
    replay: Option<replay::Check>,
    screenshot: Option<PathBuf>,
    shot_tick: Option<u64>,
    host: bool,
    connect: Option<String>,
    port: u16,
    size: (u32, u32),
    /// in a window for this run (`--windowed`, or `--size` given), whatever the options say
    windowed: bool,
    steps: Option<menu::Steps>,
    /// `--hd` / `--no-hd`: the graphics packs on or off, whatever the options say
    hd: Option<bool>,
    /// `--graphics original|enhanced`: the graphics profile, whatever the options say
    graphics: Option<bytefield_content::Profile>,
    /// no level: the menu, on this page, pressing start by itself if asked
    menu: Option<(menu::Page, bool)>,
    /// `--no-intro`: the menu without the start's movies
    no_intro: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    // `--replay <ticket>`: its command line, its recording as the script and its check (`replay.rs`)
    let mut replay = None;
    if let Some(i) = a.iter().position(|x| x == "--replay") {
        let dir = a.get(i + 1).ok_or("--replay: <ticket folder>")?;
        let (args, check) = replay::expand(std::path::Path::new(dir))?;
        a = args;
        replay = Some(check);
    }
    if a.iter().any(|x| x == "--help" || x == "-h") {
        return Err("usage: bytefield [<game_dir>] [<mod>] [--menu create[,start]] [--no-intro] [--lang L] | [<game_dir>] <mod> <level> [--mode M] [--bots N] … (docs/commands.md)".into());
    }
    // with no <game_dir>: `content.cfg`'s, or the only one there is; with none, the first screen asks (`first_screen`).
    // A first argument that is no folder is the mod
    if a.is_empty() || a[0].starts_with("--") || !std::path::Path::new(&a[0]).is_dir() {
        let found = bytefield_content::first_run::candidates();
        let cfg = bytefield_content::Config::load();
        match bytefield_content::first_run::resolve(cfg.as_ref(), &found) {
            Some(p) => {
                // found on its own: saved, so that content.cfg and `bytefield-content` say what is played (not from checks)
                let check = std::env::var_os("BYTEFIELD_CONTENT").is_some() || a.iter().any(|x| x == "--screenshot" || x == "--script");
                if !check && cfg.as_ref().and_then(|c| c.install.as_ref()).is_none() {
                    if let Err(e) = bytefield_content::first_run::remember(&p) {
                        eprintln!("content.cfg: {e}");
                    }
                }
                a.insert(0, p.display().to_string())
            }
            None => {
                settings::set_language(a.iter().position(|x| x == "--lang").and_then(|i| a.get(i + 1)).and_then(|l| settings::language_name(l)).unwrap_or_else(lang::system_language));
                first_screen::run(a, std::env::var_os("BYTEFIELD_FIRST_SCREEN_SHOT").map(Into::into));
                std::process::exit(0);
            }
        }
    }
    let connect = a.iter().position(|x| x == "--connect").and_then(|i| a.get(i + 1)).cloned();
    let menu = connect.is_none() && (a.len() < 3 || a[1..3].iter().any(|x| x.starts_with("--")));
    let opt = |n: &str| a.iter().position(|x| x == n).and_then(|i| a.get(i + 1)).cloned();
    let flag = |n: &str| a.iter().any(|x| x == n);
    let v3 = |s: Option<String>| {
        s.and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect();
            (v.len() == 3).then(|| Vec3::new(v[0], v[1], -v[2]))
        })
    };
    let num = |n: &str| opt(n).map(|v| v.parse::<u64>().map_err(|_| format!("{n}: not a number: {v}"))).transpose();
    // bots3 plays the bots unless `--bot-ai bots2` asks for the other
    bytefield_sim::bots3::use_by_default(opt("--bot-ai").is_none_or(|v| v.starts_with("bots3")));
    Ok(Args {
        argv: replay::strip_check_args(&a),
        replay,
        game: PathBuf::from(&a[0]),
        // (with --connect, the server's; in the menu, its mod)
        mod_name: match (&connect, menu) {
            (Some(_), _) => String::new(),
            (None, true) => a.get(1).filter(|m| !m.starts_with("--")).cloned().unwrap_or_else(|| "bf1942".into()),
            (None, false) => a[1].clone(),
        },
        level: if connect.is_some() || menu { String::new() } else { a[2].clone() },
        no_intro: flag("--no-intro"),
        menu: menu.then(|| {
            let m = opt("--menu").unwrap_or_default();
            (if m.starts_with("create") { menu::Page::CreateGame } else { menu::Page::Main }, m.ends_with(",start"))
        }),
        mode: opt("--mode").unwrap_or_default(),
        fly: flag("--fly"),
        at: v3(opt("--at")),
        look: v3(opt("--look")),
        team: num("--team")?.unwrap_or(1).clamp(0, 2) as u32,
        kit: num("--kit")?.unwrap_or(0) as usize,
        choose: if flag("--choose") { Some(true) } else { num("--kit")?.map(|_| false) },
        third_person: flag("--third-person"),
        bots: num("--bots")?.unwrap_or(0) as usize,
        rules: {
            // the mod's `Settings/ServerSettings.con`, else the game's (the mods ship none); then the options
            let con = bytefield_sim::damage::server_settings_file(std::path::Path::new(&a[0]), a.get(1).filter(|m| !m.starts_with("--")).map_or("bf1942", |m| m.as_str()));
            let r = con.map_or_else(Default::default, |c| bytefield_sim::damage::ServerRules::default().with_con(&c));
            let r = if flag("--no-nose-cam") { bytefield_sim::damage::ServerRules { nose_cam: false, ..r } } else { r };
            let r = num("--kickback")?.map_or(r, |k| r.with_kickback(k as f32));
            num("--friendly-fire")?.map_or(r, |f| r.with_friendly_fire(f as f32))
        },
        welcome: a.windows(2).filter(|w| w[0] == "--welcome").map(|w| w[1].clone()).collect(),
        sens: opt("--sens").map(|v| v.parse::<f32>().map_err(|_| format!("--sens: not a number: {v}"))).transpose()?,
        lang: opt("--lang").or_else(|| std::env::var("BYTEFIELD_LANG").ok()),
        invert_y: flag("--invert-y"),
        chase_yaw: opt("--chase-yaw").map(|v| v.parse::<f32>().map_err(|_| format!("--chase-yaw: not a number: {v}"))).transpose()?.unwrap_or(0.0),
        script: opt("--script").map(|s| play::parse_script(&s)).transpose()?,
        screenshot: opt("--screenshot").map(PathBuf::from),
        shot_tick: num("--shot-tick")?,
        host: flag("--host"),
        port: num("--port")?.unwrap_or(bytefield_server::udp::PORT as u64) as u16,
        size: opt("--size").map(|v| v.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))).ok_or(format!("--size: not WxH: {v}"))).transpose()?.unwrap_or((1280, 720)),
        windowed: flag("--windowed") || opt("--size").is_some(),
        hd: if flag("--hd") { Some(true) } else if flag("--no-hd") { Some(false) } else { None },
        graphics: opt("--graphics").map(|v| bytefield_content::Profile::parse(&v).ok_or(format!("--graphics: original or enhanced, not {v}"))).transpose()?,
        steps: opt("--steps").map(|s| menu::Steps::parse(&s)).transpose()?,
        connect,
    })
}

#[derive(Resource)]
struct Shot {
    path: Option<PathBuf>,
    /// the tick to take it at (None: the viewer's frame `min_frames`)
    tick: Option<u64>,
    /// in the viewer (`--fly`), the frame to take it at: `--shot-tick`, 30 without (the placed effects' smoke needs
    /// some seconds to build up, Husky's `SmokeIdleXpack`)
    min_frames: u32,
    frames: u32,
    taken: Option<u32>,
}

#[derive(Component)]
struct Fly {
    yaw: f32,
    pitch: f32,
}

/// What loading a level takes.
#[derive(SystemParam)]
struct Spawn<'w, 's> {
    commands: Commands<'w, 's>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    terrain_materials: ResMut<'w, Assets<level::TerrainMaterial>>,
    water_materials: ResMut<'w, Assets<level::WaterMaterial>>,
    tree_materials: ResMut<'w, Assets<level::TreeMaterial>>,
    billboard_materials: ResMut<'w, Assets<level::BillboardMaterial>>,
    baked_materials: ResMut<'w, Assets<level::BakedMaterial>>,
    images: ResMut<'w, Assets<Image>>,
}

fn setup(mut sp: Spawn) {
    let args = parse_args().expect("arguments");
    if let Some((page, start)) = args.menu {
        sp.commands.insert_resource(Shot { path: args.screenshot.clone(), tick: None, min_frames: 30, frames: 0, taken: None });
        let settings = settings::Settings::load();
        match menu::Menu::new(args.game.clone(), &args.mod_name, page, start.then_some(10), &settings) {
            Ok(m) => {
                // the start's movies, unless asked not to or the menu works by itself
                if std::env::var_os("BYTEFIELD_INTRO").is_some() || !(args.no_intro || start || args.screenshot.is_some() || std::env::var_os("BYTEFIELD_NO_INTRO").is_some()) {
                    let mut movies = movie::Movies::start(m.vfs(), &args.game, &args.mod_name);
                    // (`BYTEFIELD_INTRO=n`: from the n-th on)
                    movies.skip(std::env::var("BYTEFIELD_INTRO").ok().and_then(|n| n.parse().ok()).unwrap_or(0));
                    sp.commands.insert_resource(movies);
                }
                sp.commands.insert_resource(movie::MenuMusic::for_mod(&args.game, &args.mod_name));
                sp.commands.insert_resource(m);
            }
            Err(e) => panic!("menu: {e}"),
        }
        return;
    }
    launch(args, &mut sp).expect("the round");
}

/// The menu's choice: the round it asked for, with the command line's other options.
fn from_menu(chosen: Res<menu::Chosen>, settings: Res<settings::Settings>, mut sp: Spawn) {
    let mut args = parse_args().expect("arguments");
    (args.mod_name, args.level, args.mode, args.team, args.bots) = (chosen.mod_name.clone(), chosen.level.clone(), chosen.mode.clone(), chosen.team, chosen.bots);
    args.connect = chosen.connect.clone();
    args.rules = args.rules.with_friendly_fire(chosen.friendly_fire as f32);
    // the ticket's command line: the round alone, as the menu chose it
    args.argv = replay::menu_argv(&args.argv, &args.mod_name, &args.level, &args.mode, args.team, args.bots, chosen.friendly_fire);
    sp.commands.remove_resource::<menu::Chosen>();
    let (game, mod_name) = (args.game.clone(), args.mod_name.clone());
    let page = if args.connect.is_some() { menu::Page::Join } else { menu::Page::CreateGame };
    let result = launch(args, &mut sp);
    if result.is_ok() {
        sp.commands.insert_resource(menu::JustLoaded::new(page));
    }
    if let Err(e) = result {
        // back to the menu, saying why
        warn!("{e}");
        match menu::Menu::new(game, &mod_name, menu::Page::Join, None, &settings) {
            Ok(mut m) => {
                m.failed(e);
                sp.commands.insert_resource(m);
            }
            Err(e) => panic!("menu: {e}"),
        }
    }
}


/// Takes the round down: its entities (everything with a place in the world or on the screen),
/// its resources, its sounds and the server it hosts. The menu stays.
fn teardown(mut commands: Commands, round: Option<Res<menu::Round>>, roots: Query<Entity, (Or<(With<Transform>, With<Node>)>, Without<ChildOf>, Without<menu::Keep>)>, mut sounds: ResMut<sound::Sounds>) {
    if let Some((run, thread)) = round.as_ref().and_then(|r| r.hosting.as_ref()) {
        run.store(false, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = thread.lock().ok().and_then(|mut t| t.take()) {
            let _ = t.join();
        }
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    sounds.reset();
    play::clear(&mut commands);
    if let Some(r) = round.as_ref() {
        commands.insert_resource(Leaked(r.world));
    }
    commands.remove_resource::<menu::Round>();
    commands.remove_resource::<menu::Teardown>();
    commands.remove_resource::<menu::JustLoaded>();
    commands.remove_resource::<level::Atmosphere>();
    commands.remove_resource::<assets::Cache>();
    commands.insert_resource(ClearColor(Color::BLACK));
}

/// A torn-down round's `World`, to free once nothing borrows it.
#[derive(Resource)]
struct Leaked(usize);

/// Frees it, the frame after the teardown: by then the round's resources (the simulation, which
/// borrows it) are gone, and the hosted server's thread has ended.
fn free_world(mut commands: Commands, leaked: Res<Leaked>, game: Option<Res<play::Game>>, meshes: Res<Assets<Mesh>>, images: Res<Assets<Image>>, mats: Res<Assets<StandardMaterial>>, all: Query<Entity>) {
    if game.is_some() {
        return;
    }
    debug!("teardown: {} entities, {} meshes, {} images, {} materials", all.iter().count(), meshes.len(), images.len(), mats.len());
    // SAFETY: the pointer is the [`Box::leak`] of `launch`; what borrowed it (the `Sim` of `Game`, the
    // host's thread, joined) is dropped, and no system keeps a reference across rounds.
    unsafe { drop(Box::from_raw(leaked.0 as *mut World)) };
    commands.remove_resource::<Leaked>();
    give_back_memory();
}

/// A round's memory back to the system (glibc keeps it in its arenas: a client rotating levels went from 1 to 3,8 GB in 40 minutes).

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn give_back_memory() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }
    // SAFETY: a plain glibc call, with no pointers of ours.
    unsafe { malloc_trim(0) };
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn give_back_memory() {}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn memory_log() {}

/// Each minute, what the allocator has in use (`RUST_LOG=bytefield=debug`): the same level in two passes of a rotation tells a
/// leak from memory kept.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn memory_log(time: Res<Time<bevy::time::Real>>, mut last: Local<f32>) {
    if time.elapsed_secs() - *last < 60.0 {
        return;
    }
    *last = time.elapsed_secs();
    // (glibc's `struct mallinfo2`: ten size_t)
    #[repr(C)]
    struct Info {
        arena: usize,
        ordblks: usize,
        smblks: usize,
        hblks: usize,
        hblkhd: usize,
        usmblks: usize,
        fsmblks: usize,
        uordblks: usize,
        fordblks: usize,
        keepcost: usize,
    }
    // (looked up when running, not linked: `mallinfo2` is glibc 2.33, and the published binary is linked against 2.31)
    unsafe extern "C" {
        fn dlsym(handle: *mut std::ffi::c_void, name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    }
    // SAFETY: RTLD_DEFAULT (null) and a NUL-terminated name; the symbol, when there, is glibc's `mallinfo2`, with this signature.
    let f = unsafe { dlsym(std::ptr::null_mut(), c"mallinfo2".as_ptr()) };
    if f.is_null() {
        return;
    }
    // SAFETY: as above, a plain glibc call with no pointers of ours.
    let i = unsafe { std::mem::transmute::<*mut std::ffi::c_void, extern "C" fn() -> Info>(f)() };
    debug!("memory in use {} MB, kept free {} MB", (i.uordblks + i.hblkhd) >> 20, i.fordblks >> 20);
}

fn launch(mut args: Args, sp: &mut Spawn) -> Result<(), String> {
    let commands = &mut sp.commands;
    let net = match args.connect.clone() {
        Some(addr) => {
            let addr = if addr.contains(':') { addr } else { format!("{addr}:{}", args.port) };
            let net = join(&addr, &args)?;
            (args.mod_name, args.level, args.mode) = (net.welcome.mod_name.clone(), net.welcome.level.clone(), net.welcome.mode.clone());
            Some(net)
        }
        None => None,
    };
    movie::set_game(&args.game, &args.mod_name);
    // without `--mode`: conquest, or the first mode the level has when it has no conquest (GC_DeathStar
    // only brings `objectivemode`: in conquest its round ended at once)
    if args.mode.is_empty() {
        let modes = bytefield_con::session::mount(&args.game, &args.mod_name).map(|(_, vfs)| bytefield_con::session::levels(&vfs)).unwrap_or_default();
        let has = modes.iter().find(|(l, _)| l.eq_ignore_ascii_case(&args.level)).map(|(_, m)| m.clone()).unwrap_or_default();
        args.mode = if has.is_empty() || has.contains(&"conquest") { "conquest".into() } else { has[0].to_string() };
        info!("mode {} ({} brings {:?})", args.mode, args.level, has);
    }
    let world: &'static World = Box::leak(Box::new(World::load(&args.game, &args.mod_name, &args.level, &args.mode).map_err(|e| format!("{}: {e}", args.level))?));
    let mut hosting = None;
    let net = match net {
        Some(n) => Some(n),
        None if args.host => {
            let sim = bytefield_server::round(world, args.bots, false);
            let mut sim = sim;
            sim.rules.server = args.rules;
            sim.rules.welcome = args.welcome.clone();
            let start_delay = std::env::var("BYTEFIELD_START_DELAY").ok().and_then(|v| v.parse().ok()).unwrap_or(bytefield_server::START_DELAY);
            bytefield_server::hosted(&mut sim, &args.mode, args.bots, start_delay, None);
            let mut server = bytefield_server::Server::new(sim);
            let mut udp = bytefield_server::udp::UdpServer::bind(("0.0.0.0", args.port), &args.mod_name, &args.level, &args.mode).map_err(|e| format!("port {}: {e}", args.port))?;
            udp.comments = args.welcome.clone();
            info!("hosting on {}", udp.local_addr());
            let run = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
            let r = run.clone();
            let thread = std::thread::spawn(move || bytefield_server::udp::serve_while(&mut server, &mut udp, None, &r));
            hosting = Some((run, std::sync::Mutex::new(Some(thread))));
            Some(join(&format!("127.0.0.1:{}", args.port), &args)?)
        }
        None => None,
    };
    commands.insert_resource(menu::Round { game: args.game.clone(), mod_name: args.mod_name.clone(), world: world as *const World as usize, hosting });
    let mut cache = assets::Cache::default();
    cache.set_alternative_paths(world.db.globals.iter().filter(|c| c.command.eq_ignore_ascii_case("textureManager.alternativePath")).filter_map(|c| c.args.first()).map(String::as_str));
    cache.tree_light = Some(level::Atmosphere::of(world).tree_light());
    level::spawn(world, commands, &mut sp.meshes, &mut sp.materials, &mut sp.terrain_materials, &mut sp.water_materials, &mut sp.tree_materials, &mut sp.baked_materials, &mut sp.billboard_materials, &mut sp.images, &mut cache);
    commands.insert_resource(cache);
    commands.insert_resource(envmap::Cube::of(world, &mut sp.images));
    let atm = level::Atmosphere::of(world);
    let script_end = args.script.as_ref().map(|s| s.iter().map(|g| g.to).max().unwrap_or(0) + 1);
    commands.insert_resource(Shot { path: args.screenshot.clone(), tick: if args.fly { None } else { args.shot_tick.or(script_end).or(Some(30)) }, min_frames: if args.fly { args.shot_tick.map_or(30, |t| t as u32) } else { 30 }, frames: 0, taken: None });
    if args.fly {
        let over = level::overview(world);
        let look = args.look.unwrap_or(over.translation + *over.forward() * 100.0);
        let tr = level::free_camera(world, args.at.unwrap_or(over.translation), look);
        let (yaw, pitch, _) = tr.rotation.to_euler(EulerRot::YXZ);
        let mut cam = commands.spawn((Camera3d::default(), Projection::Perspective(PerspectiveProjection { fov: settings::world_fov(), far: atm.far(), ..default() }), tr, Fly { yaw, pitch }, level::WorldCamera, level::world_layers(), atm.camera()));
        cam.insert((graphics::PostCamera, effects::ViewerCamera));
        // the level's placed effects (smoke), as the original shows them before the spawn
        commands.insert_resource(effects::ViewerWorld(world));
        commands.insert_resource(effects::Effects::default());
        return Ok(());
    }
    let on_orig = false;
    let choose = !on_orig && args.choose.unwrap_or(args.script.is_none() && args.screenshot.is_none() && args.steps.is_none());
    replay::set_launch(args.argv.clone(), choose);
    play::start(play::Setup { world, team: args.team, kit: args.kit, script: args.script, third_person: args.third_person || args.chase_yaw != 0.0, chase_yaw: args.chase_yaw, fixed: args.at.map(|at| level::free_camera(world, at, args.look.unwrap_or(at + Vec3::Z * 100.0))), sens: args.sens.unwrap_or(1.0), invert_y: args.invert_y, bots: args.bots, rules: args.rules, welcome: args.welcome,
        choose, net, replay: args.replay.clone(),
    }, commands);
    Ok(())
}

/// On the server at `addr`, with its first snapshot.
fn join(addr: &str, args: &Args) -> Result<bytefield_server::udp::UdpClient, String> {
    use std::time::{Duration, Instant};
    let hello = bytefield_server::udp::Hello { team: args.team, kit: args.kit as u32, name: std::env::var("BYTEFIELD_NAME").unwrap_or_else(|_| settings::Settings::load().name) };
    let mut net = bytefield_server::udp::UdpClient::connect(addr, hello, Duration::from_secs(5)).map_err(|e| format!("{addr}: {e}"))?;
    info!("on {addr}: {} {} ({}), player {}", net.welcome.mod_name, net.welcome.level, net.welcome.mode, net.welcome.player);
    let start = Instant::now();
    while net.mirror.latest().is_none() {
        if start.elapsed() > Duration::from_secs(10) {
            return Err(format!("{addr}: no snapshot"));
        }
        net.send(0, Vec::new());
        std::thread::sleep(Duration::from_millis(30));
        net.poll();
    }
    Ok(net)
}

fn fly(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, buttons: Res<ButtonInput<MouseButton>>, motion: Res<AccumulatedMouseMotion>, mut q: Query<(&mut Transform, &mut Fly)>) {
    let Ok((mut t, mut f)) = q.single_mut() else { return };
    if buttons.pressed(MouseButton::Right) {
        f.yaw -= motion.delta.x * 0.003;
        f.pitch = (f.pitch - motion.delta.y * 0.003).clamp(-1.5, 1.5);
    }
    t.rotation = Quat::from_euler(EulerRot::YXZ, f.yaw, f.pitch, 0.0);
    let mut v = Vec3::ZERO;
    let (fwd, right) = (*t.forward(), *t.right());
    for (k, d) in [(KeyCode::KeyW, fwd), (KeyCode::KeyS, -fwd), (KeyCode::KeyD, right), (KeyCode::KeyA, -right), (KeyCode::KeyE, Vec3::Y), (KeyCode::KeyQ, -Vec3::Y)] {
        if keys.pressed(k) {
            v += d;
        }
    }
    let speed = if keys.pressed(KeyCode::ShiftLeft) { 300.0 } else { 60.0 };
    t.translation += v.normalize_or_zero() * speed * time.delta_secs();
}

fn screenshot(mut commands: Commands, time: Res<Time>, mut shot: ResMut<Shot>, game: Option<Res<play::Game>>, steps: Res<menu::Steps>, mut exit: MessageWriter<AppExit>) {
    let Some(path) = shot.path.clone() else { return };
    shot.frames += 1;
    let due = match (shot.tick, game) {
        // a check through the menus: at its `shot` step
        _ if !steps.list.is_empty() => steps.shot,
        (Some(t), Some(g)) => g.tick >= t,
        _ => shot.frames >= shot.min_frames && time.elapsed_secs() > play::WARM_UP,
    };
    if due && shot.taken.is_none() {
        // the file is written a few frames later, off the main thread
        let _ = std::fs::remove_file(&path);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
        shot.taken = Some(shot.frames);
    }
    let written = std::fs::metadata(&path).is_ok_and(|m| m.len() > 0);
    if shot.taken.is_some_and(|f| (written && shot.frames >= f + 5) || shot.frames >= f + 600) {
        exit.write(AppExit::Success);
    }
}

/// With a screenshot, the HUD's text goes to the log too (what a check reads).
fn log_status(shot: Res<Shot>, game: Option<Res<play::Game>>) {
    if let (Some(_), Some(g)) = (shot.path.as_ref(), game)
        && shot.taken == Some(shot.frames)
    {
        info!("tick {}: {}", g.tick, hud::status(&g).replace('\n', " | "));
    }
}

// mimalloc: 6–7 % more frames and 10 % less `step` with 64 bots, some 150 MB more of RSS
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// The window's mode: the whole screen, borderless (the monitor's size, no change of video
/// mode), or a window.
pub fn window_mode(fullscreen: bool) -> bevy::window::WindowMode {
    if fullscreen { bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current) } else { bevy::window::WindowMode::Windowed }
}

fn main() {
    tickets::watch_panics();
    eprintln!("Bytefield {}", tickets::COMMIT);
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let check = args.screenshot.is_some() || args.script.is_some() || args.steps.is_some();
    userlog::open(check);
    // a check does not wait for the screen (a hidden or sleeping one holds vsync back)
    let saved = settings::Settings::load();
    let present_mode = if args.screenshot.is_some() || !saved.vsync { PresentMode::AutoNoVsync } else { PresentMode::AutoVsync };
    // (a check keeps its window, of the size it asks for)
    let saved_fullscreen = saved.fullscreen && !args.windowed && !check;
    saved.apply();
    let packs = saved.apply_content(args.hd, args.graphics);
    controls::set_install(&args.game);
    // (`settings.cfg`'s one mouse, before each map had its own: once, into `controls.cfg`)
    if (saved.sens != 1.0 || saved.invert_y) && !controls::has_own_mouse() {
        let mut c = controls::current();
        c.adopt_old_mouse(saved.sens, saved.invert_y);
        c.save();
        controls::set(c);
    }
    if !packs.is_empty() {
        info!("graphics packs over the install: {packs}");
    }
    let g = assets::graphics();
    info!("graphics: {}", g.describe());
    if let Some(l) = &args.lang {
        settings::set_language(l);
    }
    settings::READ_ONLY.store(check && std::env::var_os("BYTEFIELD_CONFIG").is_none(), std::sync::atomic::Ordering::Relaxed);
    let pools = settings::task_pools(saved.compute_threads, std::env::var("BYTEFIELD_COMPUTE_THREADS").ok().and_then(|n| n.parse().ok()));
    // (a scripted run, captures: every pipeline ready the frame it is first needed. Compiled in the background, a mesh with a
    // new variant was missing for a second or two: DC's M67, `envmap` with lightmap UVs)
    // TODO: in a round too? The original draws all at once; ours shows nothing until the variant is ready
    let scripted = args.script.is_some() || args.steps.is_some();
    let mut app = App::new();
    app.insert_resource(saved);
    if let Some(s) = args.steps {
        app.insert_resource(s);
    }
    app.add_plugins(DefaultPlugins.set(WindowPlugin { primary_window: Some(Window { title: crate::lang::PROJECT.into(), resolution: args.size.into(), present_mode, mode: window_mode(saved_fullscreen), ..default() }), ..default() }).set(TaskPoolPlugin { task_pool_options: pools }).set(userlog::plugin()).set(bevy::render::RenderPlugin { synchronous_pipeline_compilation: scripted, ..default() }))
        .add_plugins((tickets::TicketsPlugin, bmfont::BmFontPlugin))
        .add_plugins((play::PlayPlugin, sound::SoundPlugin, hud::HudPlugin, minimap::MinimapPlugin, respawn::RespawnPlugin, briefing::BriefingPlugin, menu::MenuPlugin, gauges::GaugesPlugin, submarine::SubmarinePlugin, artview::ArtViewPlugin, debug::DebugPlugin, movie::MoviePlugin, flags::FlagsPlugin));
    // (the shaders before their materials: these load them as they are added)
    bevy::asset::embedded_asset!(app, "shaders/terrain.wesl");
    bevy::asset::embedded_asset!(app, "shaders/water.wesl");
    bevy::asset::embedded_asset!(app, "shaders/tree.wesl");
    bevy::asset::embedded_asset!(app, "shaders/billboard.wesl");
    bevy::asset::embedded_asset!(app, "shaders/baked.wesl");
    bevy::asset::embedded_asset!(app, "shaders/particle.wesl");
    bevy::asset::embedded_asset!(app, "shaders/envmap.wesl");
    app.add_plugins((shadows::ShadowsPlugin, skin::SkinPlugin, backdrop::BackdropPlugin));
    app.set_error_handler(log_system_error);
    app.add_plugins((MaterialPlugin::<level::TerrainMaterial>::default(), MaterialPlugin::<level::WaterMaterial>::default(), MaterialPlugin::<level::TreeMaterial>::default(), MaterialPlugin::<level::BillboardMaterial>::default(), MaterialPlugin::<level::BakedMaterial>::default(), MaterialPlugin::<effects::ParticleMaterial>::default(), MaterialPlugin::<envmap::EnvMapMaterial>::default()));
    app
        .add_systems(Startup, (assets::bc_support, setup).chain())
        .add_systems(Update, ((fly, screenshot, log_status).chain(), level::gamma_fog))
        .add_systems(Update, (effects::level_effects, effects::update.in_set(effects::EffectsUpdate)).chain().run_if(resource_exists::<effects::ViewerWorld>))
        // (before `Update`: the new round is there for all its systems in the same frame; in `Update`, a system's
        // `resource_added::<Game>` could be looked at before it and another's `is_added` after, a frame apart, and the
        // gauges' flags and kit pictures of a second round were set before their nodes were made)
        .add_systems(PreUpdate, from_menu.run_if(resource_exists::<menu::Chosen>).run_if(not(resource_exists::<menu::Teardown>)))
        // (after everything: the round's systems run as a set, under one check that it is there)
        .add_systems(Last, teardown.run_if(resource_exists::<menu::Teardown>))
        .add_systems(First, free_world.run_if(resource_exists::<Leaked>))
        .add_systems(Last, memory_log)
        .add_observer(graphics::no_indirect_drawing)
        .add_systems(PreUpdate, graphics::ui_in_gamma)
        .add_systems(PostUpdate, level::trim_cascades.after(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades).before(bevy::light::SimulationLightSystems::UpdateLightFrusta))
        .add_systems(PostUpdate, (level::follow_sky, level::fade_trees, level::sway_trees, level::water_time, level::shadow_casters, level::reflect_water, graphics::apply, graphics::render_scale).before(bevy::transform::TransformSystems::Propagate).before(bevy::camera::CameraUpdateSystems))
        .add_systems(PostUpdate, (graphics::taa_reset, graphics::dof, shadows::update, envmap::swap).after(bevy::transform::TransformSystems::Propagate))
        .add_systems(Last, quit_now)
        .run();
}

// Leaves without tearing the renderer down: the NVIDIA driver's exit handlers ran while the
// render thread still submitted, so
// `_exit`, which skips them. Nothing is saved on the way out (settings are saved by the menu,
// the log is the launcher's tee).
/// A system's error goes to the log once per system, with its name, and the round goes on: Bevy's default
/// panics, and a resource missing for one frame must not take the game down with it.
fn log_system_error(error: bevy::ecs::error::BevyError, ctx: bevy::ecs::error::ErrorContext) {
    static SEEN: std::sync::Mutex<Option<std::collections::HashSet<String>>> = std::sync::Mutex::new(None);
    let name = ctx.name().to_string();
    if SEEN.lock().is_ok_and(|mut s| s.get_or_insert_default().insert(name.clone())) {
        error!("system {name}: {error}");
    }
}

fn quit_now(mut exits: MessageReader<AppExit>) {
    if let Some(e) = exits.read().last() {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        let code = match e {
            AppExit::Success => 0,
            AppExit::Error(c) => c.get() as i32,
        };
        unsafe { libc::_exit(code) }
    }
}
