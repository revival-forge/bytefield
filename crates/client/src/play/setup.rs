//! Starting and clearing a round: `Setup`, `start`, and the first-person arms' placement helpers.

use super::*;

pub struct Setup {
    pub world: &'static World,
    pub team: u32,
    pub kit: usize,
    pub script: Option<Vec<Segment>>,
    pub third_person: bool,
    pub chase_yaw: f32,
    /// a fixed camera (`--at`, `--look`)
    pub fixed: Option<Transform>,
    pub sens: f32,
    pub invert_y: bool,
    /// bots to add (`bytefield_sim::bots`), half in each team, kits in turn
    pub bots: usize,
    /// friendly fire and the nose camera, in a round of one's own (on a server, its: the snapshot carries them)
    pub rules: bytefield_sim::damage::ServerRules,
    /// `--welcome`'s lines (on a server, its `Welcome`'s)
    pub welcome: Vec<String>,
    /// the first spawn waits on the screen when dead for the player to choose (as the original's;
    /// not with `--kit`, a script or a screenshot)
    pub choose: bool,
    /// `--kit` was given (with `BYTEFIELD_PLAYER_BOT`, the filmed bot keeps it, [`bytefield_sim::bots::film_kit`])
    pub kit_given: bool,
    /// playing on a server: the round is its, this `Sim` a copy of it
    pub net: Option<UdpClient>,
    /// `--replay`: the ticket's tick and hash (`replay.rs`)
    pub replay: Option<crate::replay::Check>,
}

pub fn start(s: Setup, commands: &mut Commands) {
    let mut sim = Sim::new(s.world, TICK);
    sim.round.playing = true;
    sim.rules.server = s.rules;
    sim.rules.welcome = s.net.as_ref().map_or_else(|| s.welcome.clone(), |n| n.welcome.comments());
    let n = sim.start_conquest().len();
    // for trying the end of a round: BYTEFIELD_TICKETS=<both teams' tickets at the start>
    if let Some(t) = std::env::var("BYTEFIELD_TICKETS").ok().and_then(|v| v.parse().ok())
        && let Some(c) = sim.modes.conquest.as_mut()
    {
        c.tickets = [t, t];
    }
    // `game.serverScoreLimit` (0, the original's default: none): BYTEFIELD_SCORE_LIMIT=<captures>
    sim.rules.score_limit = std::env::var("BYTEFIELD_SCORE_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    // `game.serverAxisTeamRatio`/`serverAlliedTeamRatio` (1 and 1): BYTEFIELD_TEAM_RATIO=<team 1>:<team 2>
    if let Some((a, b)) = std::env::var("BYTEFIELD_TEAM_RATIO").ok().and_then(|v| v.split_once(':').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))) {
        sim.rules.team_ratio = [a, b];
    }
    // `game.serverAutoBalanceTeams` (0): BYTEFIELD_AUTO_BALANCE=1
    sim.rules.auto_balance = std::env::var("BYTEFIELD_AUTO_BALANCE").is_ok_and(|v| v == "1");
    // `game.serverGameTime` (minutes; 0, the default: none): BYTEFIELD_TIME_LIMIT=<minutes>
    sim.rules.time_limit = std::env::var("BYTEFIELD_TIME_LIMIT").ok().and_then(|v| v.parse::<u32>().ok()).map_or(0, |m| m * 60);
    let (player, team) = match &s.net {
        // (the players come with the snapshot, ours among them)
        Some(net) => {
            bytefield_server::follow(&mut sim, &net.mirror);
            let p = net.welcome.player as usize;
            (p, sim.players.get(p).map_or(s.team, |p| p.team))
        }
        None => {
            // (`--team 0`: the shorter, `getTeamToAddPlayer`)
            // `BYTEFIELD_PLAYER_BOT=1`: the bots' AI plays the player too, the camera on it (to film a round)
            let p = sim.join(s.team, std::env::var_os("BYTEFIELD_PLAYER_BOT").is_some());
            if std::env::var_os("BYTEFIELD_PLAYER_BOT").is_some() {
                bytefield_sim::bots::film(Some(p));
                // (`--kit` given: the filmed bot keeps it at every spawn, not the one its side's head would pick)
                bytefield_sim::bots::film_kit(s.kit_given.then_some(s.kit));
            }
            sim.players[p].kit = s.kit;
            sim.hold_spawn(p, s.choose);
            // to try a spawn group from a script (a level's paratroopers: Market_Garden, 10): BYTEFIELD_SPAWN_GROUP=<group>
            if let Some(g) = std::env::var("BYTEFIELD_SPAWN_GROUP").ok().and_then(|v| v.parse().ok()) {
                sim.choose_spawn(p, Some(g), s.kit);
            }
            (p, sim.players[p].team)
        }
    };
    let on_orig = false;
    // `game.serverGameStartDelay` (20 in the game's `ServerSettings.con`): the pre-game, in a round of
    // one's own played by a person (not with a script or `--kit`); BYTEFIELD_START_DELAY=<s> sets it, 0 none
    // (on a server of ours, the server's: the snapshot carries it, `replica::Snapshot::pregame`)
    let start_delay = if s.net.is_none() && !on_orig && s.choose { 20.0 } else { 0.0 };
    sim.start_pregame(std::env::var("BYTEFIELD_START_DELAY").ok().and_then(|v| v.parse().ok()).filter(|_| s.net.is_none() && !on_orig).unwrap_or(start_delay));
    // a copy of a server's round, ours too: its hits give no damage, the deaths are the server's
    if s.net.is_some() {
        sim.role.remote = true;
    }
    if on_orig {
        sim.role.remote = true;
        for v in sim.vehicles.iter_mut() {
            v.removed = true;
        }
    }
    let template = bytefield_sim::replica::soldier_template(team);
    if s.bots > 0 && s.net.is_none() && !on_orig {
        sim.enable_bots();
        // to film a mind against another (as `tournament3 --a --b`): BYTEFIELD_MINDS="1=bots:cover-fight;2=bots"
        if let Ok(v) = std::env::var("BYTEFIELD_MINDS") {
            let level = s.world.level.name.clone();
            for (team, name) in v.split(';').filter_map(|x| x.split_once('=')) {
                let team: u32 = team.trim().parse().unwrap_or_else(|_| panic!("BYTEFIELD_MINDS: {team} is not a team"));
                assert!(bytefield_sim::bots::is_name(name.trim()), "BYTEFIELD_MINDS: no mind called {name}");
                sim.set_minds(team, bytefield_sim::bots::mind(name.trim(), &level, 0));
                info!("team {team}: mind {name}");
            }
        }
        // (as `gameStatusPlaying` fills the server: each to `getTeamToAddPlayer`'s team)
        for i in 0..s.bots {
            let p = sim.join(0, true);
            sim.players[p].kit = (i / 2) % 5;
        }
    }
    info!("{n} vehicles; the player in team {team}{}", if s.net.is_some() { " (on a server)" } else { "" });
    let cam = crate::level::before_spawn_camera(s.world, team);
    let kit = sim.players[player].kit;
    commands.insert_resource(Game {
        session: LocalSession::new(sim),
        net: s.net.map(|mut n| {
            n.mirror.hold = s.choose;
            n
        }),
        prediction: Default::default(),
        player,
        tick: 0,
        alpha: 0.0,
        acc: 0.0,
        third_person: s.third_person,
        third_on_foot: s.third_person,
        paused: false,
        outside: Outside::Chase,
        nose: false,
        orbit: Vec2::ZERO,
        chase: None,
        scout_view: None,
        chase_yaw: s.chase_yaw,
        fixed: s.fixed,
        script: s.script,
        realtime: std::env::var_os("BYTEFIELD_REALTIME").is_some(),
        spent_ms: [0.0; 2],
        mouse_factor: s.sens,
        mouse_flip: s.invert_y,
        mouse: Vec2::ZERO,
        seat_head: 0.0,
        tapped: HashSet::new(),
        menu_open: false,
        radio_menu: None,
        radio_icons: true,
        radio_taken: HashSet::new(),
        spawn_choice: (None, kit),
        spawn_taken: HashSet::new(),
        chat_box: None,
        console: false,
        note_to: None,
        before: Held::default(),
        recording: Default::default(),
        replay: s.replay,
        camera: (cam, cam),
        center_1p: center_1p(s.world, template),
        arms_eased: None,
    });
    commands.insert_resource(crate::briefing::Briefing { up: s.choose });
    commands.insert_resource(Shown::default());
    commands.insert_resource(Shade::default());
    commands.insert_resource(Shells::default());
    commands.insert_resource(Skins::default());
    commands.insert_resource(Weapons::default());
    commands.insert_resource(InHand::default());
    let fov_1p = s.world.db.object_template(template).and_then(|t| t.get("set1pfov")).and_then(|a| a.first()).and_then(|a| a.trim().parse().ok()).unwrap_or(0.45);
    let atm = crate::level::Atmosphere::of(s.world);
    let eye = commands.spawn((Camera3d::default(), Projection::Perspective(PerspectiveProjection { fov: crate::settings::world_fov(), far: atm.far(), ..default() }), cam, PlayCamera, crate::level::WorldCamera, crate::level::world_layers(), atm.camera())).id();
    // the same exposure and no tone mapping as the world's (with Bevy's defaults the arms came out black)
    // the last camera on the window: the post-process goes on it
    let mut arms = commands.spawn((
        Camera3d::default(),
        atm.camera(),
        ArmsCamera { fov: fov_1p },
        Camera { order: 1, clear_color: ClearColorConfig::None, ..default() },
        Projection::Perspective(PerspectiveProjection { fov: fov_1p, near: 0.01, far: 10.0, ..default() }),
        RenderLayers::layer(FIRST_PERSON_LAYER),
        Transform::IDENTITY,
        ChildOf(eye),
    ));
    arms.insert(crate::graphics::PostCamera);
}

/// A template's vector property, if it has one.
pub(super) fn template_vec(world: &World, template: &str, prop: &str) -> Option<[f32; 3]> {
    let v = bytefield_world::parse_vec(world.db.object_template(template)?.get(prop)?.first()?)?;
    (v.len() == 3).then(|| [v[0] as f32, v[1] as f32, v[2] as f32])
}

/// `center1pHands` (the template's, else that of the soldiers).
pub(super) fn center_1p(world: &World, template: &str) -> [f32; 3] {
    template_vec(world, template, "center1phands").unwrap_or([-0.12, -1.56, 0.1])
}

/// Where the first-person skeleton is from the eyes, in Bevy's frame: its root at
/// `center1pHands` plus the weapon's `soldierCameraPosition`, turned with the camera (the
/// server's `updateAnimations` in the first person); our skeleton's matrices put the root
/// `setCharacterHeight` under the soldier's centre, so the poses move up by that much.
/// The client's formula; against the original's rendering zoomed, the weapon within millimetres.
pub(super) fn arms_offset(g: &Game, i: usize) -> Vec3 {
    let m = g.session.sim.vehicles[i].soldier();
    let height = m.and_then(|m| m.skeleton.as_ref()).map_or(-1.0, |s| s.height);
    let w = g.arms_eased.map_or_else(|| arms_target(g, i), |e| e.0);
    let c = g.center_1p;
    assets::to_bevy([c[0] + w[0], c[1] + w[1] - height, c[2] + w[2]])
}

/// The position the weapon asks of the arms: `soldierZoomPosition` zoomed, else `soldierCameraPosition`.
pub(super) fn arms_target(g: &Game, i: usize) -> [f32; 3] {
    let prop = if zoom(g).is_some() { "soldierzoomposition" } else { "soldiercameraposition" };
    let m = g.session.sim.vehicles[i].soldier();
    m.and_then(|m| m.weapon.as_ref()).and_then(|w| template_vec(g.session.sim.world, &w.template.name, prop)).unwrap_or([0.0; 3])
}

/// One frame of the arms going to the weapon's place (the client's soldier update; only the client has it): the
/// position a quarter of the way, the first person's zoom factor `0.3 new + 0.7 old`, and that within 0.001 lands.
/// TODO: taken as once per drawn frame, as the function's place suggests; its caller is unread.
pub(super) fn arms_ease(cur: ([f32; 3], f32), target: ([f32; 3], f32)) -> ([f32; 3], f32) {
    let p = std::array::from_fn(|k| cur.0[k] + (target.0[k] - cur.0[k]) * 0.25);
    let f = if (cur.1 - target.1).abs() > 0.001 { target.1 * 0.3 + cur.1 * 0.7 } else { target.1 };
    (p, f)
}

/// Drops the round's resources (`main`'s teardown; the entities go there).
pub fn clear(commands: &mut Commands) {
    commands.remove_resource::<Game>();
    commands.remove_resource::<Shown>();
    commands.remove_resource::<Shells>();
    commands.remove_resource::<Skins>();
    commands.remove_resource::<Weapons>();
    commands.remove_resource::<InHand>();
    commands.insert_resource(crate::effects::Effects::default());
    commands.insert_resource(crate::lights::Flashes::default());
    commands.insert_resource(Burning::default());
}
