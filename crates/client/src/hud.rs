//! A minimal HUD: the player's health, weapon and ammunition, the vehicle it is in, the
//! tickets and flags, the time to its spawn; a cross in the middle; who won, at a round's end.

use bevy::prelude::*;
use bevy::text::FontSize;

use crate::lang::{fill, heading, tr};
use crate::play::Game;
use bytefield_formats::lexicon::Lexicon;

#[derive(Component)]
struct Status;

#[derive(Component)]
struct Cross;

/// The two crosses of the original.
/// `Arms(r)`: a 2×2 dot and four arms 13 px long from `r` px off it, `r` growing with the weapon's deviation up to 5 (the
/// default `Game.setCrossHairRadius`): 0 (no gap: vehicles' guns, the M25), 3 (the UH-60's minigun), 5 (on foot, the
/// stationary MG42). `Small`: arms 7–8 px from 5 px off, no dot, half a pixel up and left (`CHTIcon`: knives, grenades,
/// kits, every plane's pilot, the SBD over its sight).
#[derive(Clone, Copy, PartialEq, Debug)]
enum CrossKind {
    Arms(i32),
    Small,
}

/// A group of the cross: the arms (false) or the small one (true).
#[derive(Component)]
struct CrossGroup(bool);

/// One of the four arms (left, right, up, down), moved with the radius.
#[derive(Component)]
struct CrossArm(u8);

/// The end of a round, in the middle.
#[derive(Component)]
struct Banner;

/// The kills of the last seconds, top right.
#[derive(Component)]
struct Kills;

/// The mode's state under the score, top right: each flag of CTF, the objectives.
#[derive(Component)]
struct Mode;

/// The radio: the menu open and what was said, left.
#[derive(Component)]
struct Radio;

/// The scoreboard (Tab, and at a round's end): a column for each team.
#[derive(Component)]
struct Board(u32);

#[derive(Component)]
struct BoardRoot;

/// The radio's lines: when (game time) and what.
#[derive(Default)]
struct RadioLines {
    seen: usize,
    lines: Vec<(f64, String)>,
    /// Our server's chat lines shown.
    net_chat: usize,
    /// how far the server's silence has been told: 0 not, 1 «connection problems», 2 «disconnected»
    net_silence: u8,
    /// The player's scout marks already told (their projectiles)
    marks: std::collections::HashSet<usize>,
    /// Whether the player sat in an artillery seat (`artPos`) the last frame
    art: bool,
}

/// `Game.setRadioFlashTime 7` (`Menu.con`): how long a message stays, and how many at once.
const RADIO_TIME: f64 = 7.0;
const RADIO_LINES: usize = 6;
/// Seconds of the server's silence before the HUD says «connection problems», and «disconnected»
const SILENT_WARN: f32 = 3.0;
const SILENT_GONE: f32 = 10.0;
/// Within this a message said out loud reaches a player (the original's server sends it to
/// those within 70 m; its `SoldierVoice` ramp is silent past 55 m)
const LOCAL_RANGE: f32 = 70.0;

/// The kills to show: when (game time) and what.
#[derive(Default)]
struct Feed {
    seen: usize,
    /// the control points' log seen
    seen_cp: usize,
    /// the flags' and the objectives' logs seen
    seen_ctf: usize,
    seen_obj: usize,
    lines: Vec<(f64, String)>,
}

/// How long a kill stays on the screen (s), and how many at once.
const FEED_TIME: f64 = 6.0;
const FEED_LINES: usize = 5;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, font).add_systems(Update, (setup.run_if(resource_added::<Game>), (update, update_mode).run_if(resource_exists::<Game>)).chain());
    }
}

/// The system's DejaVu Sans, where there is one, in place of Bevy's own font (which has no
/// accents or `¡`, and the lexicon's texts need them).
const SYSTEM_FONT: &[&str] = &["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf", "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf", "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", "C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\arial.ttf"];
/// The scoreboard's, for its columns (Bevy's own where there is none).
const MONO_FONT: &[&str] = &["/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", "/usr/share/fonts/TTF/DejaVuSansMono.ttf", "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf", "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf", "C:\\Windows\\Fonts\\consola.ttf", "C:\\Windows\\Fonts\\cour.ttf"];

#[derive(Resource, Default)]
struct Mono(Handle<Font>);

/// The first of these fonts there is.
fn read_font(paths: &[&str]) -> Option<Font> {
    // TODO: with none of them, accents come out as boxes: Bevy's own font is ASCII only (not tried on Windows)
    paths.iter().find_map(|p| std::fs::read(p).ok()).map(Font::from_bytes)
}

/// Makes a system font with accents the default one (Bevy's has none).
pub fn default_font(fonts: &mut Assets<Font>) {
    if let Some(f) = read_font(SYSTEM_FONT) {
        let _ = fonts.insert(&Handle::<Font>::default(), f);
    }
}

fn font(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    default_font(&mut fonts);
    commands.insert_resource(Mono(read_font(MONO_FONT).map(|f| fonts.add(f)).unwrap_or_default()));
}

/// The HUD of a round (made with each: a teardown takes it away).
fn setup(mut commands: Commands, mono: Res<Mono>) {
    let font = TextFont { font_size: FontSize::Px(18.0), ..default() };
    commands.spawn((
        Text::new(""),
        font.clone(),
        TextColor(Color::WHITE),
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), bottom: Val::Px(10.0), max_width: Val::Px(470.0), ..default() },
        Status,
    ));
    // The original's cross (measured at 1024×768): a 2×2 dot in the middle
    // and four arms 2 px thick, 13 px long, from 5 px off the dot. Black: `game.setCrossHairColor 256 256 0` of
    // the default settings wraps to 0 in a byte. Level 2 (from the picture).
    // The arms move out with the weapon's deviation. TODO: the size at other resolutions.
    let black = BackgroundColor(Color::BLACK);
    let bar = |left: f32, top: f32, w: f32, h: f32| (Node { position_type: PositionType::Absolute, left: Val::Px(left), top: Val::Px(top), width: Val::Px(w), height: Val::Px(h), ..default() }, black);
    let group = |small: bool| (Node { position_type: PositionType::Absolute, ..default() }, CrossGroup(small), Visibility::Hidden);
    commands
        .spawn((Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), top: Val::Percent(50.0), ..default() }, Cross))
        .with_children(|c| {
            c.spawn(group(false)).with_children(|c| {
                c.spawn(bar(0.0, 0.0, 2.0, 2.0));
                c.spawn((bar(-17.0, 0.0, 13.0, 2.0), CrossArm(0)));
                c.spawn((bar(5.0, 0.0, 13.0, 2.0), CrossArm(1)));
                c.spawn((bar(0.0, -17.0, 2.0, 13.0), CrossArm(2)));
                c.spawn((bar(0.0, 5.0, 2.0, 13.0), CrossArm(3)));
            });
            // (a texture half a pixel off, the Mig29's: a solid row at 384 and a half-dark one at 383, the same with the columns)
            c.spawn(group(true)).with_children(|c| {
                let half = BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5));
                for (l, t, w, h) in [(-12.0, 0.0, 7.0, 1.0), (5.0, 0.0, 8.0, 1.0), (0.0, -12.0, 1.0, 7.0), (0.0, 5.0, 1.0, 8.0)] {
                    c.spawn(bar(l, t, w, h));
                    let (hl, ht) = if w > h { (l, t - 1.0) } else { (l - 1.0, t) };
                    c.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(hl), top: Val::Px(ht), width: Val::Px(w), height: Val::Px(h), ..default() }, half));
                }
            });
        });
    commands.spawn((
        Text::new(""),
        TextFont { font_size: FontSize::Px(16.0), ..default() },
        TextColor(Color::srgb(1.0, 0.92, 0.75)),
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Px(130.0), ..default() },
        Kills,
    ));
    commands.spawn((
        Text::new(""),
        TextFont { font_size: FontSize::Px(15.0), ..default() },
        TextColor(Color::WHITE),
        TextLayout::justify(Justify::Right),
        // (under the minimap and its bar)
        Node { position_type: PositionType::Absolute, right: Val::Px(12.0), top: Val::Px(285.0), ..default() },
        Mode,
    ));
    commands.spawn((
        Text::new(""),
        TextFont { font_size: FontSize::Px(34.0), ..default() },
        TextColor(Color::srgb(1.0, 0.92, 0.75)),
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, left: Val::Percent(25.0), width: Val::Percent(50.0), top: Val::Percent(30.0), padding: UiRect::all(Val::Px(16.0)), ..default() },
        BackgroundColor(Color::NONE),
        Banner,
    ));
    commands.spawn((
        Text::new(""),
        TextFont { font_size: FontSize::Px(16.0), ..default() },
        TextColor(Color::WHITE),
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Percent(35.0), ..default() },
        Radio,
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Px(60.0),
                margin: UiRect::left(Val::Px(-480.0)),
                width: Val::Px(960.0),
                column_gap: Val::Px(20.0),
                padding: UiRect::all(Val::Px(14.0)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            Visibility::Hidden,
            BoardRoot,
        ))
        .with_children(|b| {
            for team in [1u32, 2] {
                let color = if team == 1 { Color::srgb(1.0, 0.55, 0.5) } else { Color::srgb(0.6, 0.75, 1.0) };
                b.spawn((Text::new(""), TextFont { font: bevy::text::FontSource::Handle(mono.0.clone()), font_size: FontSize::Px(15.0), ..default() }, TextColor(color), Node { width: Val::Px(450.0), ..default() }, Board(team)));
            }
        });
}

/// A team's column of the scoreboard: its tickets, then its players by score (as the original
/// sorts them), with kills and deaths; the player's own row marked.
pub fn board(g: &Game, lex: &Lexicon, team: u32) -> String {
    let sim = &g.session.sim;
    let t = |k: &str| tr(lex, k);
    let name = heading(lex, if team == 1 { "COL_HEADING_AXIS_TEAM" } else { "COL_HEADING_ALLIED_TEAM" });
    let tickets = sim.modes.conquest.as_ref().map_or(0, |c| c.tickets[team as usize - 1].max(0));
    let mut rows: Vec<usize> = (0..sim.players.len()).filter(|&k| sim.players[k].team == team).collect();
    rows.sort_by_key(|&k| (-sim.players[k].score, sim.players[k].deaths, k));
    let mut out = format!("{name:<10} {} {tickets:>4}\n\n{:<20}{:>9}{:>9}{:>9}\n", t("TICKETS"), "", heading(lex, "COL_HEADING_SCORE"), heading(lex, "COL_HEADING_KILLS"), heading(lex, "COL_HEADING_DEATHS"));
    for k in rows {
        let p = &sim.players[k];
        let who = if k == g.player { format!("> {}", t("YOU")) } else { format!("  {}", who(g, lex, k)) };
        // (a long name, as the server's bots' «Hans-Jörg Böcker», cut to its column: the row would wrap)
        let who: String = who.chars().take(19).collect();
        let dead = if p.alive { ' ' } else { '+' };
        out += &format!("{who:<19}{dead}{:>9}{:>9}{:>9}\n", p.score, p.kills, p.deaths);
    }
    out
}

/// The radio's texts: the open menu (its heading, then each key and entry) and the messages heard
/// (the team's, and those said out loud near the player).
fn radio(g: &Game, lex: &Lexicon, r: &mut RadioLines) -> String {
    use bytefield_sim::radio::{Entry, entry};
    let sim = &g.session.sim;
    let text = |k: &str| tr(lex, k);
    let point = |i: usize| sim.modes.conquest.as_ref().and_then(|c| c.points.get(i)).map(|p| p.template.name.clone());
    // (cursors count from the round's start: the sim cuts its logs, `trim_logs`; past the end, another round)
    if sim.journal.log_base.radio + sim.journal.radio_log.len() < r.seen {
        r.marks.clear();
    }
    let me = sim.players.get(g.player);
    let ear = me.and_then(|p| p.soldier).and_then(|s| sim.vehicles.get(s)).map(|v| v.craft.body.pos);
    // one's own new mark: «You called for artillery!» before the radio's line
    let mine = me.and_then(|p| p.soldier);
    for (k, _) in sim.scout_cameras().filter(|(_, p)| p.scout.is_some_and(|m| Some(m.player) == mine)) {
        if r.marks.insert(k) {
            r.lines.push((sim.clock.time, text("PLAYER_CALLED_FOR_ARTILLERY")));
        }
    }
    // sitting down at an artillery gun: each of the team's marks, «<scout> called for artillery (timeleft: <s>)»
    // (the original's on boarding the Priest's gun; level 3, when else it says it is not known)
    let art = mine.is_some_and(|s| sim.art_seat(s));
    if art && !r.art
        && let Some(s) = mine
    {
        for k in sim.team_scout_cameras(sim.vehicles[s].team) {
            let p = &sim.projectiles[k];
            let Some(m) = p.scout.as_ref() else { continue };
            let Some(scout) = sim.players.iter().position(|q| q.soldier == Some(m.player)) else { continue };
            let left = (p.ttl - p.age as f32).max(0.0).ceil() as i32;
            r.lines.push((sim.clock.time, format!("{} {}: {left})", who(g, lex, scout), text("CALLED_FOR_ARTILLERY"))));
        }
    }
    r.art = art;
    for e in bytefield_sim::log_since(&sim.journal.radio_log, sim.journal.log_base.radio, &mut r.seen) {
        let heard = if e.msg.local {
            ear.is_some_and(|a| (0..3).map(|j| (a[j] - e.at[j]).powi(2)).sum::<f32>() < LOCAL_RANGE * LOCAL_RANGE)
        } else {
            me.is_some_and(|p| p.team == e.team)
        };
        if !heard {
            continue;
        }
        let mut line = format!("{}: {}", who(g, lex, e.player), text(&format!("RADIO_{}", e.msg.key)));
        if let Some(n) = e.point.and_then(point) {
            line += &format!(" {n}");
        }
        r.lines.push((sim.clock.time, line));
    }
    if let Some(n) = g.net.as_ref() {
        // (counted from the round's start: the mirror keeps the newest, `chat_base` before them)
        for l in n.mirror.chat.get(r.net_chat.saturating_sub(n.mirror.chat_base)..).into_iter().flatten() {
            info!("chat: {l}");
            r.lines.push((sim.clock.time, l.clone()));
        }
        r.net_chat = n.mirror.chat_base + n.mirror.chat.len();
        // the server silent: said once at each step, and again if it comes back and goes (ours, level 3; 10 s is when our
        // server drops a silent client, `udp::TIMEOUT`)
        // TODO: when the original's client says these two (its lexicon has them; the times are unread). How: cut the link to a server in the original and time the HUD text.
        let s = n.silent().as_secs_f32();
        let now = if s >= SILENT_GONE { 2 } else if s >= SILENT_WARN { 1 } else { 0 };
        if now > r.net_silence {
            let key = if now == 2 { "GAME_MESSAGE_DISCONNECTED" } else { "GAME_MESSAGE_CONNECTION_PROBLEM" };
            let line = tr(lex, key);
            info!("net: the server silent for {s:.1} s: {line}");
            r.lines.push((sim.clock.time, line));
        }
        r.net_silence = now;
    }
    r.lines.retain(|l| sim.clock.time - l.0 < RADIO_TIME);
    let n = r.lines.len();
    if n > RADIO_LINES {
        r.lines.drain(..n - RADIO_LINES);
    }
    let mut out: Vec<String> = r.lines.iter().map(|l| l.1.clone()).collect();
    // (the chat box: the original's `SayAll>`, `SayTeam>`)
    if let Some((to_all, t)) = g.chat_box.as_ref() {
        let say = if g.console { ">".to_string() } else if g.note_to.is_some() { text("TICKET_NOTE") } else { text(if *to_all { "INGAME_CHAT_SAY_ALL" } else { "INGAME_CHAT_SAY_TEAM" }) };
        out.push(format!("{say} {t}_"));
    }
    const HEADINGS: [&str; 7] = ["RADIO_CONFIRM", "RADIO_REQUEST", "RADIO_ENEMY_SPOTTED", "RADIO_GAME", "LOCAL_CONFIRM", "LOCAL_ALARMS", "LOCAL_TACTICS"];
    if let Some(f) = g.radio_menu {
        out.push(String::new());
        out.push(format!("F{f} {}", text(&format!("RADIO_HEADING_{}", HEADINGS[(f - 1) as usize]))));
        let riding = sim.riding(g.player);
        for k in 1..=7 {
            let name = match entry(f, k, riding) {
                Some(Entry::Say(m)) => text(&format!("RADIO_HEADING_{}", m.key)),
                Some(Entry::Point(None)) => text("RADIO_CLOSEST_CONTROLPOINT"),
                Some(Entry::Point(Some(i))) => match point(i) {
                    Some(n) => n,
                    None => continue,
                },
                None => continue,
            };
            out.push(format!("  {k}  {name}"));
        }
        out.push(format!("  8  {}", text("RADIO_HEADING_CANCEL")));
    }
    out.join("\n")
}

/// The end of a round: who won (team 1 the Axis, 2 the Allies, as in bf1942) and when the next
/// starts; empty while playing.
pub fn banner(g: &Game, lex: &Lexicon) -> String {
    if g.paused {
        return tr(lex, "PAUSED");
    }
    let sim = &g.session.sim;
    // TODO: where and when the original shows it: unseen (level 3); the rule is the server's.
    if sim.round.round_end.is_none() && sim.player_outside(g.player) {
        return tr(lex, "DESSERTION_MESSAGE");
    }
    let (Some(c), Some(left)) = (sim.modes.conquest.as_ref(), sim.round.round_end) else { return String::new() };
    let t = |k: &str| tr(lex, k);
    let won = t(match c.winner {
        Some(1) => "AXIS_WINS",
        Some(2) => "ALLIES_WIN",
        _ => "DRAW",
    });
    let mine = sim.players[g.player].team as u8;
    let you = match c.winner {
        Some(w) if w == mine => format!("  ({})", t("YOUR_SIDE")),
        Some(1 | 2) => format!("  ({})", t("OTHER_SIDE")),
        _ => String::new(),
    };
    let next = fill(&t("NEXT_ROUND"), &[&format!("{:.0}", left.ceil())]);
    format!("{}\n{won}{you}\n{} {} / {}\n{next}", t("ROUND_OVER"), t("TICKETS"), c.tickets[0].max(0), c.tickets[1].max(0))
}

/// A player as the feed names it: the person, a bot by its name (or by number and side).
fn who(g: &Game, lex: &Lexicon, k: usize) -> String {
    if k == g.player {
        return tr(lex, "YOU");
    }
    let p = &g.session.sim.players[k];
    if !p.name.is_empty() {
        return p.name.clone();
    }
    format!("{} {}", tr(lex, if p.ai { "BOT" } else { "PLAYER" }), k)
}

/// The player whose soldier is (or drives) vehicle `v`.
fn player_of(g: &Game, v: usize) -> Option<usize> {
    let sim = &g.session.sim;
    sim.players.iter().position(|p| p.soldier.is_some_and(|s| s == v || sim.vehicles.get(s).is_some_and(|x| x.entry.inside == Some(v))))
}

/// New deaths into the feed: who killed whom and with what (`Armor.killed_by`, the attacker's
/// vehicle; its soldier's weapon, or the vehicle).
fn feed(g: &Game, lex: &Lexicon, f: &mut Feed) {
    let sim = &g.session.sim;
    for (_, k, e) in bytefield_sim::log_since(&sim.journal.player_log, sim.journal.log_base.player, &mut f.seen) {
        let bytefield_sim::players::PlayerEvent::Died(sol) = e else { continue };
        let by = sim.vehicles.get(*sol).and_then(|v| v.armor.as_ref()).and_then(|a| a.killed_by);
        let line = match by.and_then(|b| Some((b, player_of(g, b)?))) {
            Some((_, a)) if a == *k && a == g.player => tr(lex, "YOU_DIED"),
            Some((_, a)) if a == *k => fill(&tr(lex, "SUICIDE"), &[&who(g, lex, *k)]),
            Some((b, a)) => {
                let v = &sim.vehicles[b];
                let with = v.soldier().and_then(|m| m.weapon.as_ref()).map_or(v.template.clone(), |w| w.template.name.clone());
                format!("{} {} {} ({with})", who(g, lex, a), tr(lex, "DEFAULT_KILL_TEXT"), who(g, lex, *k))
            }
            None if *k == g.player => tr(lex, "YOU_DIED"),
            None => fill(&tr(lex, "DIED"), &[&who(g, lex, *k)]),
        };
        info!("kill: {line}");
        f.lines.push((sim.clock.time, line));
    }
    // the flags taken and lost
    if let Some(c) = sim.modes.conquest.as_ref() {
        if c.log.len() < f.seen_cp {
            f.seen_cp = 0;
        }
        for &(_, k, e) in &c.log[f.seen_cp..] {
            let flag = c.points[k].template.name.clone();
            let side = |t: u32| tr(lex, if t == 1 { "THE_AXIS" } else { "THE_ALLIES" });
            let line = match e {
                bytefield_sim::conquest::CpEvent::Taken(t) => format!("{}: {flag}", tr(lex, if t == 1 { "AXIS_CAPTURED" } else { "ALLIES_CAPTURED" })),
                bytefield_sim::conquest::CpEvent::Lost(t) => fill(&tr(lex, "LOST_FLAG"), &[&side(t), &flag]),
            };
            info!("flag: {line}");
            f.lines.push((sim.clock.time, line));
        }
        f.seen_cp = c.log.len();
    }
    // the flags of CTF: who took, dropped, returned or captured one (the lexicon's texts)
    if let Some(c) = sim.modes.ctf.as_ref() {
        if c.log.len() < f.seen_ctf {
            f.seen_ctf = 0;
        }
        for &(_, _, e) in &c.log[f.seen_ctf..] {
            use bytefield_sim::ctf::FlagEvent::*;
            let line = match e {
                Taken(k) => format!("{} {}", who(g, lex, k), tr(lex, "STOLE_THE_FLAG")),
                Returned(k) => format!("{} {}", who(g, lex, k), tr(lex, "RETURNED_THE_FLAG")),
                Captured(k) => format!("{} {}", who(g, lex, k), tr(lex, "CAPTURED_THE_FLAG")),
                Dropped => tr(lex, "DROPPED_THE_FLAG"),
                Home => continue,
            };
            info!("flag: {line}");
            f.lines.push((sim.clock.time, line));
        }
        f.seen_ctf = c.log.len();
    }
    // the objectives: a target destroyed, a side's objectives done or timed out
    if let Some(o) = sim.modes.objectives.as_ref() {
        if o.log.len() < f.seen_obj {
            f.seen_obj = 0;
        }
        for &(_, i, e) in &o.log[f.seen_obj..] {
            use bytefield_sim::objective::{Goal, ObjectiveEvent};
            let ob = &o.list[i];
            let side = if ob.team == 1 { "AXIS" } else { "ALLIES" };
            let line = match (e, &ob.goal) {
                (ObjectiveEvent::Met(_), Goal::Destroy { target }) => format!("{target} {}", tr(lex, &format!("{side}_DESTROYED_OBJECTIVE1"))),
                (ObjectiveEvent::Done, Goal::Timer { .. }) => tr(lex, &format!("{}_OBJECTIVE_TIMER_TIMED_OUT1", side)),
                (ObjectiveEvent::Done, Goal::All(_)) => tr(lex, &format!("{}_OBJECTIVE_ACCOMPLISHED1", side)),
                _ => continue,
            };
            info!("objective: {line}");
            f.lines.push((sim.clock.time, line));
        }
        f.seen_obj = o.log.len();
    }
    f.lines.retain(|l| sim.clock.time - l.0 < FEED_TIME);
    let n = f.lines.len();
    if n > FEED_LINES {
        f.lines.drain(..n - FEED_LINES);
    }
}

/// The mode's panel: in CTF each flag (at home, carried by whom, on the ground and for how long);
/// in objective mode the targets left and a timer's time.
pub fn mode(g: &Game, lex: &Lexicon) -> String {
    let sim = &g.session.sim;
    let mut lines = Vec::new();
    if let Some(c) = sim.modes.ctf.as_ref() {
        for b in &c.bases {
            let name = tr(lex, if b.team == 1 { "THE_AXIS" } else { "THE_ALLIES" });
            let f = &b.flag;
            lines.push(match f.carrier {
                _ if f.home => fill(&tr(lex, "FLAG_HOME"), &[&name]),
                Some(k) => fill(&tr(lex, "FLAG_CARRIED"), &[&name, &who(g, lex, k)]),
                None => fill(&tr(lex, "FLAG_DOWN"), &[&name, &format!("{:.0}", f.timer.max(0.0).ceil())]),
            });
        }
    }
    if let Some(o) = sim.modes.objectives.as_ref() {
        use bytefield_sim::objective::Goal;
        for ob in &o.list {
            match &ob.goal {
                Goal::Destroy { target } => lines.push(format!("{} {target}", if ob.done || ob.timer > 0.0 { "✗" } else { "□" })),
                Goal::Timer { limit, clock } => {
                    let left = (limit * bytefield_sim::objective::ATTACKER_TICKETS_MOD as f32 / 100.0 - clock).max(0.0) as u32;
                    lines.push(fill(&tr(lex, "TIME_LEFT"), &[&format!("{}:{:02}", left / 60, left % 60)]));
                }
                Goal::All(_) => {}
            }
        }
    }
    lines.join("\n")
}

/// The status line(s) for the player.
pub fn status(g: &Game) -> String {
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let mut lines = Vec::new();
    if let Some(c) = sim.modes.conquest.as_ref() {
        let flags: String = c.points.iter().map(|cp| match cp.team {
            1 => '1',
            2 => '2',
            _ => '-',
        }).collect();
        lines.push(format!("tickets {} / {}   flags [{}]   {:.0} s", c.tickets[0], c.tickets[1], flags, sim.clock.time));
    }
    match p.soldier.filter(|_| p.alive).map(|i| &sim.vehicles[i]) {
        None => lines.push(format!("dead: spawns in {:.0} s", p.spawn_time.ceil())),
        Some(s) => {
            let hp = s.armor.as_ref().map_or(0.0, |a| a.hp / a.max_hp * 100.0);
            let b = &s.craft.body.pos;
            let mut l = format!("health {hp:.0}   ({:.0}, {:.0}, {:.0})", b[0], b[1], b[2]);
            match s.entry.inside {
                Some(k) => {
                    let v = &sim.vehicles[k];
                    let vhp = v.armor.as_ref().map_or(0.0, |a| a.hp / a.max_hp * 100.0);
                    l += &format!("   {} (seat {}) {vhp:.0} %", v.template, s.entry.position + 1);
                }
                None => {
                    let d = |v: &bytefield_sim::mover::Vehicle| (0..3).map(|j| (v.craft.body.pos[j] - b[j]).powi(2)).sum::<f32>().sqrt();
                    if let Some((v, dist)) = sim.vehicles.iter().filter(|v| !v.removed && !v.has_soldier_contact() && !v.entry.seats.is_empty()).map(|v| (v, d(v))).min_by(|a, b| a.1.total_cmp(&b.1))
                        && dist < 40.0
                    {
                        // where it is from where the soldier faces (degrees, + to its right)
                        let (f, to) = (s.craft.body.rows[2], [v.craft.body.pos[0] - b[0], v.craft.body.pos[2] - b[2]]);
                        let side = (f[2] * to[0] - f[0] * to[1]).atan2(f[0] * to[0] + f[2] * to[1]).to_degrees();
                        l += &format!("   near: {} at {dist:.0} m, {side:+.0} degrees", v.template);
                    }
                    if let Some(w) = s.soldier().and_then(|m| m.weapon.as_ref()) {
                        let mag = w.mags.first().copied().unwrap_or(0);
                        let rest: i32 = w.mags.iter().skip(1).sum();
                        l += &format!("   {} {mag} / {rest}{}", w.template.name, if w.reload > 0.0 { " (recargando)" } else { "" });
                    }
                }
            }
            lines.push(l);
        }
    }
    lines.join("\n")
}

#[allow(clippy::type_complexity)]
fn update_mode(g: Res<Game>, mut q: Query<&mut Text, With<Mode>>, mut lex: Local<Option<Lexicon>>) {
    if g.is_added() {
        *lex = None;
    }
    let lex = lex.get_or_insert_with(|| crate::lang::load(&g.session.sim.world.vfs));
    let Ok(mut t) = q.single_mut() else { return };
    let m = mode(&g, lex);
    if t.0 != m {
        t.0 = m;
    }
}

fn update(
    g: Res<Game>,
    mut q: Query<&mut Text, (With<Status>, Without<Kills>, Without<Banner>, Without<Radio>, Without<Mode>)>,
    mut kills: Query<&mut Text, (With<Kills>, Without<Banner>, Without<Radio>)>,
    mut banner_q: Query<(&mut Text, &mut BackgroundColor), (With<Banner>, Without<Radio>)>,
    mut radio_q: Query<(&mut Text, &mut TextColor), (With<Radio>, Without<Status>, Without<Kills>)>,
    mut r: Local<RadioLines>,
    mut lex: Local<Option<Lexicon>>,
    respawn: Res<crate::respawn::RespawnUi>,
    mut cross: Query<&mut Visibility, With<Cross>>,
    mut cross_groups: Query<(&CrossGroup, &mut Visibility), Without<Cross>>,
    mut cross_arms: Query<(&CrossArm, &mut Node)>,
    mut f: Local<Feed>,
    mut board_root: Query<&mut Visibility, (With<BoardRoot>, Without<Cross>, Without<CrossGroup>)>,
    mut boards: Query<(&mut Text, &Board), (Without<Status>, Without<Kills>, Without<Banner>, Without<Radio>)>,
) {
    // (a new round: what this remembered was the last one's)
    if g.is_added() {
        (*r, *f, *lex) = Default::default();
    }
    let lex = lex.get_or_insert_with(|| crate::lang::load(&g.session.sim.world.vfs));
    // (ours, for checks: the position, health and weapon in words; the gauges show them)
    if let Ok(mut t) = q.single_mut() {
        let quiet = String::new();
        t.0 = if std::env::var_os("BYTEFIELD_HUD_DEBUG").is_some() { status(&g) } else { quiet };
    }
    if let Ok((mut t, mut bg)) = banner_q.single_mut() {
        let b = banner(&g, lex);
        if t.0 != b {
            bg.0 = if b.is_empty() { Color::NONE } else { Color::srgba(0.0, 0.0, 0.0, 0.6) };
            t.0 = b;
        }
    }
    if let Ok((mut t, mut c)) = radio_q.single_mut() {
        let text = radio(&g, lex, &mut r);
        if t.0 != text {
            t.0 = text;
        }
        // `Game.setAxisRadioColor` and `setAlliedRadioColor` (`Menu.con`): the player's team's
        let team = g.session.sim.players.get(g.player).map_or(0, |p| p.team);
        let want = if team == 1 { Color::srgb(1.0, 0.35, 0.35) } else { Color::srgb(0.4, 0.6, 1.0) };
        if c.0 != want {
            c.0 = want;
        }
    }
    feed(&g, lex, &mut f);
    if let Ok(mut t) = kills.single_mut() {
        let text = f.lines.iter().map(|l| l.1.as_str()).collect::<Vec<_>>().join("\n");
        if t.0 != text {
            t.0 = text;
        }
    }
    // the scoreboard while Tab is held, at the round's end, and from the spawn screen
    let open = g.before.keys.contains("tab") || g.session.sim.round.round_end.is_some() || (respawn.board && g.dead());
    if let Ok(mut v) = board_root.single_mut() {
        let want = if open { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if open {
        for (mut t, b) in boards.iter_mut() {
            let text = board(&g, lex, b.0);
            if t.0 != text {
                t.0 = text;
            }
        }
    }
    // behind a scope or dead, no cross; nor from outside a vehicle (F10–F12: the original draws none);
    // nor where the weapon's or the seat's `setCrossHairType` is not `CHTCrossHair`
    let p = &g.session.sim.players[g.player];
    let outside = g.third_person && p.soldier.is_some_and(|i| g.session.sim.vehicles[i].entry.inside.is_some());
    // the nose camera draws the cross, a plane's too
    let kind = cross_hair(&g).or(g.nose.then_some(CrossKind::Arms(5)));
    let shown = if g.dead() || outside || kind.is_none() || crate::play::zoom(&g).is_some_and(|z| z.scope) { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in cross.iter_mut() {
        if *v != shown {
            *v = shown;
        }
    }
    for (k, mut v) in cross_groups.iter_mut() {
        let want = if kind.is_some_and(|c| (c == CrossKind::Small) == k.0) { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if let Some(CrossKind::Arms(r)) = kind {
        let r = r as f32;
        for (a, mut n) in cross_arms.iter_mut() {
            let off = Val::Px(if a.0 % 2 == 0 { -12.0 - r } else { r });
            let slot = if a.0 < 2 { &mut n.left } else { &mut n.top };
            if *slot != off {
                *slot = off;
            }
        }
    }
}

/// Which cross the player's weapon in hand, or its seat's PlayerControlObject, draws: `setCrossHairType` (the client's
/// parser: `CHTNone` 0, `CHTIcon` 1, `CHTCrossHair` 2). `CHTIcon` draws the small cross (in
/// 17 of 17 planes, the AC-130's seats, the knife; the SBD's seat 1 over its sight, which was once taken for none).
// TODO: the type of a template without the command (taken as a cross).
fn cross_hair(g: &Game) -> Option<CrossKind> {
    // (looking from a scout's mark: its own cross, `artview`)
    if g.scout_view.is_some() {
        return None;
    }
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let Some(s) = p.soldier.map(|i| &sim.vehicles[i]) else { return Some(CrossKind::Arms(5)) };
    // the template that names the type, and the weapon whose deviation opens the cross
    let (name, weapon) = match s.entry.inside {
        Some(k) => {
            let v = &sim.vehicles[k];
            let seat = s.entry.position;
            let w = v.weapons.iter().find(|(part, w)| v.craft.parts[*part].seat == seat && w.template.input_fire != 23).map(|(_, w)| w);
            (v.entry.seats.get(seat).map(|seat| seat.pco.clone()), w)
        }
        None => {
            let w = s.soldier().and_then(|m| m.weapon.as_ref());
            (w.map(|w| w.template.name.clone()), w)
        }
    };
    let kind = name.and_then(|n| sim.world.db.object_template(&n)).and_then(|t| t.get("setcrosshairtype").and_then(|a| a.first()).map(|v| v.trim().to_ascii_lowercase()));
    match kind.as_deref() {
        None | Some("chtcrosshair" | "crosshair") => {
            // TODO: the original's crosshair node moves the arms out by its "Deviation"
            // value in pixels, as is. The HUD component binds it. Each frame it sets Radius = dev × CrossHairRadius and Deviation = dev × CrossHairSize, where dev is the
            // client's weapon deviation: minDev + the fire's, which decays by a
            // fixed amount per update + a one-shot.
            // Unknown: how the node's 800×600 units become pixels and what caps it at 5. How: read the node's update in the
            // original's client and compare against its screen.
            // Level 3, a line through the captures: deviation 0–0.2 → 0 (the M25, the Stryker's and AH64's guns), the
            // minigun's 0.4 → 3, on foot → 5 (the most).
            let dev = weapon.map_or(1.0, |w| w.dev);
            Some(CrossKind::Arms((15.0 * dev - 3.0).round().clamp(0.0, 5.0) as i32))
        }
        Some("chticon" | "icon") => Some(CrossKind::Small),
        _ => None,
    }
}
