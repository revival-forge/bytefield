//! The screen when dead, as the original's spawn interface: over the 3D view, the kits' panel on
//! the left (the team's tab, its flag and name, each kit with its icon and how many of the team
//! carry it), the messages at the top, the bar of CLOSE / SCORE BOARD / DONE at the bottom, and
//! the level's map with the team's spawn points (white dots) on the right.
//!
//! Laid out on the original's 800×600 screen (its pictures, `menu/Texture/Ingame/respawn/…`, are
//! drawn at that size; at 1024×768 it is ×1.28, as the original's capture shows), scaled to the
//! window. Where the map goes we have not seen (the original showed none before the game began):
//! ours. ← and → (or a click on a dot) choose where, 1–5 (or a click) the kit
//! (`play::spawn_keys`); the player spawns there by itself when its time is up, as in the
//! original. The first time it waits on the screen until the player is done (DONE, CLOSE or
//! Enter; not with `--kit` or a script). CLOSE and DONE hide the screen (Enter brings it back);
//! SCORE BOARD shows the board.
//! The mouse is freed while dead, and taken at the spawn.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::assets::Cache;
use crate::lang::tr;
use crate::minimap::{area, on_map};
use crate::play::Game;
use bytefield_formats::lexicon::Lexicon;

/// The original's screen.
const W: f32 = 800.0;
const H: f32 = 600.0;
/// The kits' panel: its left, and where the first kit starts (under the tab and the team's box).
const PANEL_X: f32 = 30.0;
const KITS_Y: f32 = 127.0;
/// Each kit's box (`ingame_respawn_kits_middle`: 205×83 drawn).
const KIT_H: f32 = 83.0;
/// The map (ours): its corner and side.
const MAP: (f32, f32, f32) = (530.0, 90.0, 240.0);
/// Marks kept for the groups and for the flags.
const GROUPS: usize = 16;
const FLAGS: usize = 16;
/// A flag's icon on the map, in the screen's 800×600.
const FLAG: f32 = 22.0;
/// A spawn point's `map_dot` and the chosen one's `map_circle` around it (16 px on the capture of 1024, ×1,28; the circle, ours).
const DOT: f32 = 12.5;
const CIRCLE: f32 = 18.0;
/// Marks kept for the team's vehicles and soldiers, and their size against the minimap's (the same pictures in 1024: ÷1,28).
const MARKS: usize = 96;
const MARK_SCALE: f32 = 0.78;
/// Kits there may be (`game.setKit <team> 0..4`).
const KITS: usize = 5;
/// The kits' small icons (`Debriefing/classes`), by kind; the anti-tank has none.
const CLASS_ICONS: [Option<&str>; KITS] = [Some("Debriefing/classes/class_scout_16x16"), Some("Debriefing/classes/class_assault_16x16"), None, Some("Debriefing/classes/class_medic_16x16"), Some("Debriefing/classes/class_engineer_16x16")];
const TEXT: Color = Color::srgb(0.93, 0.93, 0.88);
const DARK_TEXT: Color = Color::srgb(0.12, 0.12, 0.08);

/// Shown or not (CLOSE and DONE hide it; Enter brings it back), and the score board asked for.
#[derive(Resource, Default)]
pub struct RespawnUi {
    pub hidden: bool,
    pub board: bool,
}

#[derive(Component)]
struct Screen;
/// A rectangle of the 800×600 screen, and the text's size there.
#[derive(Component, Clone, Copy)]
pub struct At(pub f32, pub f32, pub f32, pub f32);
#[derive(Component, Clone, Copy)]
pub struct FontAt(pub f32);
/// A spawn group's mark: the n-th of the team's (`Sim::spawn_groups`).
#[derive(Component)]
struct GroupMark(usize);
#[derive(Component)]
struct FlagMark(usize);
/// The team's vehicles and soldiers, then the player's ring.
#[derive(Component)]
struct TeamMark(usize);
/// The circle around the chosen spawn point.
#[derive(Component)]
struct ChosenMark;
/// A kit's box (clicked to choose it), its fill (the chosen one's), its icon, name, `ACTIVE …`
/// and count.
#[derive(Component)]
struct KitBox(usize);
#[derive(Component)]
struct KitFill(usize);
#[derive(Component)]
struct KitIcon(usize);
#[derive(Component)]
struct KitName(usize);
#[derive(Component)]
struct KitActive(usize);
#[derive(Component)]
struct KitCount(usize);
#[derive(Component)]
struct Tab;
#[derive(Component)]
struct TeamFlag;
#[derive(Component)]
struct TeamName;
/// The messages at the top, as the original's: the time to spawn (or, in the
/// pre-game, until the game starts) with its number on the right, the spawn point's, and the server's
/// news. A box without text is not drawn.
#[derive(Component)]
struct Message(usize);
/// A message's number, on the right of its box.
#[derive(Component)]
struct MessageValue(usize);
#[derive(Component)]
struct MessageBox(usize);
const MESSAGES: usize = 3;
/// «GAME IS RESTARTING»'s timer (6.0 when the
/// screen first finds enough people, down with the screen's frames; below 1 the box clears, so it
/// shows 5 s; in the original, at 5 and 8 s of the pre-game, not at 14)
const RESTARTING: f32 = 6.0;
const RESTARTING_CLEARS: f32 = 1.0;

/// The spawn screen's message of the round's start: once a round, when the people are
/// enough, «GAME IS RESTARTING» for its time; until they are, «ADDITIONAL PLAYERS NEEDED» each frame.
#[derive(Default)]
struct Restarting {
    shown: bool,
    timer: f32,
    rounds: u32,
    /// the simulation's step last seen: the timer goes down with its steps (as the frames in play;
    /// with a script, a step a frame)
    steps: u64,
}
#[derive(Component, Clone, Copy, PartialEq)]
enum Press {
    Close,
    Board,
    Done,
}

pub struct RespawnPlugin;

impl Plugin for RespawnPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RespawnUi>().add_systems(Update, (setup, update, message_boxes, fit, clicks, crate::briefing::press, cursor).chain().distributive_run_if(resource_exists::<Game>));
    }
}

fn pic(cache: &mut Cache, g: &Game, images: &mut Assets<Image>, name: &str) -> Option<Handle<Image>> {
    let stem = name.trim().trim_matches('"');
    let stem = stem.rsplit_once('.').map_or(stem, |s| s.0);
    cache.texture(&g.session.sim.world.vfs, &format!("menu/Texture/{stem}"), images)
}

fn image(h: Option<Handle<Image>>) -> ImageNode {
    h.map(|image| ImageNode { image, image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() }).unwrap_or_default()
}

fn node() -> Node {
    Node { position_type: PositionType::Absolute, ..default() }
}

fn label(text: &str, px: f32, color: Color) -> impl Bundle {
    (Text::new(text), TextFont { font_size: FontSize::Px(px), ..default() }, FontAt(px), TextColor(color))
}

fn setup(mut commands: Commands, g: Res<Game>, mut cache: ResMut<Cache>, mut images: ResMut<Assets<Image>>, mut ui: ResMut<RespawnUi>, mut made: Local<bool>) {
    if *made && !g.is_added() {
        return;
    }
    *made = true;
    *ui = RespawnUi::default();
    let lex = crate::lang::load(&g.session.sim.world.vfs);
    let dir = g.session.sim.world.level.terrain.heightmap.rsplit_once('/').map_or("", |d| d.0).to_string();
    let map = g.session.sim.world.vfs.read(&format!("{dir}/textures/ingamemap.dds")).ok().and_then(|b| bytefield_formats::dds::decode(&b).ok()).map(|i| images.add(crate::assets::rgba_image(&i)));
    let mut p = |n: &str| pic(&mut cache, &g, &mut images, n);
    let (top, middle, bottom, long) = (p("Ingame/respawn/ingame_respawn_kits_top_256x128"), p("Ingame/respawn/ingame_respawn_kits_middle_256x128"), p("Ingame/respawn/ingame_respawn_kits_bottom_256x128"), p("Ingame/respawn/ingame_respawn_long_512x64"));
    let (close, green, message) = (p("Menu/knappext_n"), p("Menu/knapp3_n"), p("Ingame/text-mess/textmessbg_1line_256x32"));
    let (dot, circle) = (p("Minimap/map_dot"), p("Minimap/map_circle"));
    let classes: Vec<Option<Handle<Image>>> = CLASS_ICONS.iter().map(|c| c.and_then(&mut p)).collect();
    let t = |k: &str| tr(&lex, k);
    commands
        .spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, Visibility::Hidden, GlobalZIndex(10), Screen))
        .with_children(|c| {
            // the kits' panel: the tab, the team's box, the kits
            c.spawn((node(), At(PANEL_X, 30.0, 256.0, 32.0), ImageNode::default(), Tab));
            c.spawn((node(), At(PANEL_X + 10.0, 36.0, 90.0, 14.0))).with_child(label(&t("RESPAWN_AXIS"), 9.0, TEXT));
            c.spawn((node(), At(PANEL_X + 113.0, 36.0, 90.0, 14.0))).with_child(label(&t("RESPAWN_ALLIED"), 9.0, TEXT));
            c.spawn((node(), At(PANEL_X, 57.0, 256.0, 128.0), image(top)));
            c.spawn((node(), At(PANEL_X + 18.0, 58.0, 64.0, 64.0), ImageNode::default(), TeamFlag));
            c.spawn((node(), At(PANEL_X + 92.0, 80.0, 110.0, 24.0))).with_child((label("", 17.0, Color::WHITE), TeamName));
            for k in 0..KITS {
                let y = KITS_Y + KIT_H * k as f32;
                c.spawn((node(), At(PANEL_X, y, 256.0, 128.0), image(if k + 1 < KITS { middle.clone() } else { bottom.clone() }), KitBox(k)));
                c.spawn((node(), At(PANEL_X + 4.0, y + 17.0, 197.0, 62.0), BackgroundColor(Color::NONE), KitFill(k)));
                c.spawn((node(), At(PANEL_X + 4.0, y + 1.0, 197.0, 15.0), BackgroundColor(Color::srgba(0.62, 0.6, 0.38, 0.85))));
                c.spawn((node(), At(PANEL_X + 8.0, y + 3.0, 90.0, 12.0))).with_child((label("", 8.0, DARK_TEXT), KitName(k)));
                c.spawn((Node { justify_content: JustifyContent::FlexEnd, ..node() }, At(PANEL_X + 98.0, y + 3.0, 100.0, 12.0))).with_child((label("", 8.0, DARK_TEXT), KitActive(k)));
                if let Some(h) = classes[k].clone() {
                    c.spawn((node(), At(PANEL_X + 6.0, y + 19.0, 16.0, 16.0), image(Some(h))));
                }
                c.spawn((node(), At(PANEL_X + 22.0, y + 18.0, 64.0, 64.0), ImageNode::default(), KitIcon(k)));
                c.spawn((Node { justify_content: JustifyContent::Center, ..node() }, At(PANEL_X + 140.0, y + 36.0, 40.0, 24.0))).with_child((label("0", 16.0, Color::WHITE), KitCount(k)));
            }
            // the messages
            // (measured at 1024×768 in the original: y 8–37, 42–81 and 84–122, x 390–718)
            for (k, (y, h)) in [(6.0, 23.0), (33.0, 30.5), (65.6, 30.5)].into_iter().enumerate() {
                c.spawn((node(), At(305.0, y, 256.0, h), image(message.clone()), MessageBox(k))).with_children(|b| {
                    b.spawn((Node { margin: UiRect::axes(Val::Px(7.0), Val::Px(5.0)), ..default() }, label("", 8.5, TEXT), Message(k)));
                    b.spawn((Node { position_type: PositionType::Absolute, right: Val::Px(10.0), top: Val::Px(4.0), ..default() }, label("", 10.0, TEXT), MessageValue(k)));
                });
            }
            // the bar at the bottom
            c.spawn((node(), At(281.0, 551.0, 512.0, 64.0), image(long)));
            for (x, pic, key, press) in [(330.0, close, "RESPAWN_CLOSE", Press::Close), (481.0, green.clone(), "RESPAWN_SCOREBOARD", Press::Board), (631.0, green, "RESPAWN_DONE", Press::Done)] {
                c.spawn((node(), At(x - 3.0, 556.0, 128.0, 128.0), image(pic)));
                c.spawn((Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..node() }, At(x, 557.0, 107.0, 25.0), press)).with_child(label(&t(key), 8.5, TEXT));
            }
            // the map, with the flags and the team's spawn points
            let (mx, my, side) = MAP;
            c.spawn((node(), At(mx - 3.0, my - 3.0, side + 6.0, side + 6.0), BackgroundColor(Color::srgba(0.25, 0.25, 0.22, 0.85))));
            c.spawn((node(), At(mx, my, side, side), image(map)));
            for k in 0..FLAGS {
                c.spawn((node(), At(0.0, 0.0, FLAG, FLAG), ImageNode::default(), Visibility::Hidden, FlagMark(k)));
            }
            for k in 0..MARKS {
                c.spawn((node(), At(0.0, 0.0, 12.0, 12.0), ImageNode::default(), UiTransform::default(), Visibility::Hidden, TeamMark(k)));
            }
            for k in 0..GROUPS {
                c.spawn((node(), At(0.0, 0.0, DOT, DOT), image(dot.clone()), Visibility::Hidden, GroupMark(k)));
            }
            c.spawn((node(), At(0.0, 0.0, CIRCLE, CIRCLE), image(circle), Visibility::Hidden, ChosenMark));
        });
}

/// What a spawn group is called: the flag that gives it (`setSpawnGroup`), or its team's base.
fn group_name(g: &Game, lex: &Lexicon, id: i64) -> String {
    let c = g.session.sim.modes.conquest.as_ref();
    c.and_then(|c| c.points.iter().find(|p| p.template.spawn_group == id || p.template.second_spawn_group == id)).map_or_else(|| tr(lex, "BASE"), |p| p.template.name.clone())
}

/// Where a spawn group is: the middle of its points.
fn group_pos(g: &Game, id: i64) -> Option<[f32; 3]> {
    let s = &g.session.sim.modes.conquest.as_ref()?.groups.get(&id)?.spawns;
    let n = s.len().max(1) as f32;
    Some(s.iter().fold([0.0; 3], |a, p| [a[0] + p.pos[0] / n, a[1] + p.pos[1] / n, a[2] + p.pos[2] / n]))
}

/// A property's last value (`setKitIcon 0 "kits/…"`: the path), unquoted.
pub fn last_arg(g: &Game, template: &str, key: &str) -> Option<String> {
    let t = g.session.sim.world.db.object_template(template)?;
    t.get(key).and_then(|a| a.last()).map(|s| s.trim_matches('"').to_string())
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
/// A message's box only with a text in it. The time to spawn stays up with the screen closed (CLOSE,
/// DONE), so that the wait has a number: ours.
/// TODO: what the original shows while dead with its screen closed (under Wine).
fn message_boxes(g: Res<Game>, ui: Res<RespawnUi>, briefing: Res<crate::briefing::Briefing>, mut boxes: Query<(&MessageBox, &mut Visibility)>, texts: Query<(&Message, &Text)>) {
    let closed = g.dead() && ui.hidden && !ui.board && !briefing.up;
    for (m, t) in texts.iter() {
        let want = if t.0.is_empty() {
            Visibility::Hidden
        } else if m.0 == 0 && closed {
            Visibility::Visible
        } else {
            Visibility::Inherited
        };
        for (b, mut v) in boxes.iter_mut() {
            if b.0 == m.0 && *v != want {
                *v = want;
            }
        }
    }
}

fn update(
    g: Res<Game>,
    ui: Res<RespawnUi>,
    briefing: Res<crate::briefing::Briefing>,
    mut cache: ResMut<Cache>,
    mut images: ResMut<Assets<Image>>,
    mut screen: Query<&mut Visibility, (With<Screen>, Without<GroupMark>, Without<FlagMark>, Without<TeamMark>, Without<ChosenMark>)>,
    mut groups: Query<(&GroupMark, &mut At, &mut Visibility), (Without<Screen>, Without<FlagMark>, Without<TeamMark>, Without<ChosenMark>)>,
    mut flags: Query<(&FlagMark, &mut At, &mut ImageNode, &mut Visibility), (Without<Screen>, Without<GroupMark>, Without<TeamMark>, Without<ChosenMark>)>,
    mut team_marks: Query<(&TeamMark, &mut At, &mut ImageNode, &mut UiTransform, &mut Visibility), (Without<Screen>, Without<GroupMark>, Without<FlagMark>, Without<ChosenMark>)>,
    mut chosen_mark: Query<(&mut At, &mut Visibility), (With<ChosenMark>, Without<Screen>, Without<GroupMark>, Without<FlagMark>, Without<TeamMark>)>,
    mut fills: Query<(&KitFill, &mut BackgroundColor), Without<FlagMark>>,
    mut pics: Query<(&mut ImageNode, Option<&KitIcon>, Has<Tab>, Has<TeamFlag>), (Without<FlagMark>, Without<TeamMark>)>,
    mut texts: Query<(&mut Text, Option<&KitName>, Option<&KitActive>, Option<&KitCount>, Option<&Message>, Option<&MessageValue>, Has<TeamName>)>,
    mut lex: Local<Option<Lexicon>>,
    mut seen: Local<(u32, Restarting)>,
) {
    if g.is_added() {
        (*lex, *seen) = (None, Default::default());
    }
    let dead = g.dead();
    for mut v in screen.iter_mut() {
        *v = if dead && !ui.hidden && !briefing.up { Visibility::Inherited } else { Visibility::Hidden };
    }
    if !dead {
        return;
    }
    let sim = &g.session.sim;
    let lex = lex.get_or_insert_with(|| crate::lang::load(&sim.world.vfs));
    let me = &sim.players[g.player];
    let team = me.team;
    let kits: Vec<Option<String>> = sim.modes.conquest.as_ref().and_then(|c| c.teams.kits.get(team.wrapping_sub(1) as usize)).cloned().unwrap_or_default();
    // the pictures that go with the team: the tab (the team's side lighter), its flag, the kits' icons
    let (team_seen, restart) = &mut *seen;
    if *team_seen != team {
        *team_seen = team;
        let skin = sim.modes.conquest.as_ref().and_then(|c| c.teams.skins.get(team.wrapping_sub(1) as usize).cloned().flatten());
        let flag = skin.and_then(|s| last_arg(&g, &s, "setTeamFlagIcon")).and_then(|f| pic(&mut cache, &g, &mut images, &f));
        let tab = pic(&mut cache, &g, &mut images, if team == 1 { "Ingame/respawn/ingame_respawn_kits_tab2_256x32" } else { "Ingame/respawn/ingame_respawn_kits_tab_256x32" });
        let icons: Vec<Option<Handle<Image>>> = (0..KITS).map(|k| kits.get(k).cloned().flatten().and_then(|n| last_arg(&g, &n, "setKitIcon")).and_then(|f| pic(&mut cache, &g, &mut images, &f))).collect();
        for (mut im, icon, is_tab, is_flag) in pics.iter_mut() {
            let h = match (icon, is_tab, is_flag) {
                (Some(k), _, _) => icons[k.0].clone(),
                (_, true, _) => tab.clone(),
                (_, _, true) => flag.clone(),
                _ => continue,
            };
            *im = image(h);
        }
    }
    let ids = sim.spawn_groups(g.player);
    let (choice, kit) = g.spawn_choice;
    let picked = choice.filter(|c| ids.contains(c));
    // (the one it will spawn in: its choice if still its team's, else the first)
    let chosen = picked.or(ids.first().copied());
    for (f, mut bg) in fills.iter_mut() {
        bg.0 = if f.0 == kit { Color::srgba(0.45, 0.55, 0.25, 0.75) } else { Color::NONE };
    }
    let name_key = |k: usize| kits.get(k).cloned().flatten().and_then(|n| last_arg(&g, &n, "setKitName"));
    // (in the pre-game, its time left; the time to spawn shows once the waves run)
    let (time_key, time) = if sim.round.pregame > 0.0 { ("TIME_TO_START", sim.round.pregame) } else { ("TIME_TO_SPAWN", me.spawn_time) };
    // (the original clears it only while the game is in its end,
    // so once a round; a pre-game that starts again in the same round, with too few people, does not show it again)
    if sim.round.rounds != restart.rounds {
        (restart.shown, restart.rounds) = (false, sim.round.rounds);
    }
    let dt = sim.clock.steps.saturating_sub(restart.steps).min(30) as f32 * sim.clock.dt as f32;
    restart.steps = sim.clock.steps;
    let info = if sim.round.players_needed > 0 {
        format!("{} {}", tr(lex, "INFO_MESSAGE_GAME_NOT_STARTED"), sim.round.players_needed)
    } else {
        if !restart.shown {
            (restart.shown, restart.timer) = (true, RESTARTING);
        }
        if restart.timer > 0.0 {
            restart.timer -= dt;
        }
        if restart.timer >= RESTARTING_CLEARS { tr(lex, "INFO_MESSAGE_GAME_IS_RESTARTING") } else { String::new() }
    };
    let messages: [(String, String); MESSAGES] = [
        if sim.round.pregame > 0.0 || me.spawn_time > 0.0 { (tr(lex, time_key), format!("{:.0}", time.ceil())) } else { Default::default() },
        (
            match picked {
                Some(id) => format!("{}: {}", tr(lex, "RESPAWN_SPAWN_POINT_SELECTED"), group_name(&g, lex, id)),
                None => tr(lex, "RESPAWN_SPAWN_POINT_NOT_SELECTED"),
            },
            String::new(),
        ),
        (info, String::new()),
    ];
    for (mut t, name, active, count, msg, value, is_team) in texts.iter_mut() {
        let want = if let Some(k) = name {
            name_key(k.0).map(|n| tr(lex, &n)).unwrap_or_default()
        } else if let Some(k) = active {
            // (`setKitActiveName`; every mod here sets it as the name's key with ACTIVE_, which stays for a kit without it)
            let template = kits.get(k.0).cloned().flatten();
            let own = template.as_ref().and_then(|n| last_arg(&g, n, "setKitActiveName"));
            own.or_else(|| name_key(k.0).map(|n| n.replacen("RESPAWN_", "RESPAWN_ACTIVE_", 1))).map(|n| tr(lex, &n)).unwrap_or_default()
        } else if let Some(k) = count {
            if kits.get(k.0).is_some_and(Option::is_some) { sim.players.iter().filter(|p| p.team == team && p.alive && p.kit == k.0).count().to_string() } else { String::new() }
        } else if let Some(m) = msg {
            messages[m.0].0.clone()
        } else if let Some(m) = value {
            messages[m.0].1.clone()
        } else if is_team {
            tr(lex, if team == 1 { "RESPAWN_AXIS" } else { "RESPAWN_ALLIED" })
        } else {
            continue;
        };
        if t.0 != want {
            t.0 = want;
        }
    }
    let a = area(&g);
    let (mx, my, side) = MAP;
    for (m, mut at, mut vis) in groups.iter_mut() {
        match ids.get(m.0).and_then(|&id| group_pos(&g, id)).and_then(|p| on_map(a, p[0], p[2])) {
            Some((u, v)) => {
                *at = At(mx + u * side - DOT / 2.0, my + v * side - DOT / 2.0, DOT, DOT);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (mut at, mut vis) in chosen_mark.iter_mut() {
        match chosen.and_then(|id| group_pos(&g, id)).and_then(|p| on_map(a, p[0], p[2])) {
            Some((u, v)) => {
                *at = At(mx + u * side - CIRCLE / 2.0, my + v * side - CIRCLE / 2.0, CIRCLE, CIRCLE);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    // the team's vehicles and soldiers, and the ring where the camera is (`hudcmp/w2/f_after.png`), as on the minimap
    let mut get = |n: &str| pic(&mut cache, &g, &mut images, n);
    let mut list = crate::minimap::team_marks(&g, team, &mut get);
    list.truncate(MARKS - 1);
    let (eye, ahead) = (g.camera.1.translation, g.camera.1.forward());
    list.push(crate::minimap::Mark { image: get("Minimap/minimap_icon_ring_32x32"), x: eye.x, z: -eye.z, side: 32.0, tint: Color::WHITE, turn: crate::minimap::heading([ahead.x, ahead.y, -ahead.z]) });
    for (k, mut at, mut im, mut t, mut vis) in team_marks.iter_mut() {
        let Some((m, (u, v))) = list.get(k.0).and_then(|m| Some((m, on_map(a, m.x, m.z)?))) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let s = m.side * MARK_SCALE;
        *at = At(mx + u * side - s / 2.0, my + v * side - s / 2.0, s, s);
        if let Some(h) = &m.image
            && im.image != *h
        {
            im.image = h.clone();
        }
        im.color = m.tint;
        t.rotation = Rot2::radians(m.turn);
        *vis = if m.image.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    }
    let points = sim.modes.conquest.as_ref().map_or(&[][..], |c| &c.points[..]);
    for (m, mut at, mut im, mut vis) in flags.iter_mut() {
        match points.get(m.0).and_then(|p| on_map(a, p.pos[0], p.pos[2]).map(|uv| (uv, p))) {
            Some(((u, v), p)) => {
                *at = At(mx + u * side - FLAG / 2.0, my + v * side - FLAG / 2.0, FLAG, FLAG);
                // (the minimap's: its owner's icon, a base's crossed out)
                let name = crate::minimap::flag(&g, p.team, p.template.unable_to_change_team);
                if let Some(h) = pic(&mut cache, &g, &mut images, &name)
                    && im.image != h
                {
                    im.image = h;
                }
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

/// The 800×600 screen as large as the window lets it, in its middle.
fn fit(windows: Query<&Window>, mut nodes: Query<(Ref<At>, &mut Node)>, mut fonts: Query<(Ref<FontAt>, &mut TextFont)>, mut last: Local<(f32, f32)>) {
    let Ok(w) = windows.single() else { return };
    let s = (w.width() / W).min(w.height() / H).max(0.1);
    let (ox, oy) = ((w.width() - W * s) / 2.0, (w.height() - H * s) / 2.0);
    let changed = *last != (w.width(), w.height());
    *last = (w.width(), w.height());
    for (at, mut n) in nodes.iter_mut() {
        if changed || at.is_changed() {
            let At(x, y, aw, ah) = *at;
            n.left = Val::Px(ox + x * s);
            n.top = Val::Px(oy + y * s);
            n.width = Val::Px(aw * s);
            n.height = Val::Px(ah * s);
        }
    }
    for (f, mut font) in fonts.iter_mut() {
        // (and a text made after the window last changed: a second round's, the briefing's)
        if changed || f.is_changed() {
            font.font_size = FontSize::Px(f.0 * s);
        }
    }
}

/// A click (the mouse free) on a spawn point, a kit or a button; Enter shows the screen again.
#[allow(clippy::type_complexity)]
fn clicks(
    mut g: ResMut<Game>,
    mut ui: ResMut<RespawnUi>,
    briefing: Res<crate::briefing::Briefing>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    groups: Query<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility, &GroupMark)>,
    kits: Query<(&ComputedNode, &UiGlobalTransform, &KitBox)>,
    presses: Query<(&ComputedNode, &UiGlobalTransform, &Press)>,
) {
    if !g.dead() {
        if ui.hidden || ui.board {
            *ui = RespawnUi::default();
        }
        return;
    }
    if briefing.up {
        return;
    }
    // (a script's `close`: as CERRAR)
    if g.before.keys.contains("close") {
        ui.hidden = true;
    }
    // (Enter: the screen back, or, on it, done choosing)
    if crate::controls::current().just(&keys, &buttons, "enter") {
        if ui.hidden {
            ui.hidden = false;
        } else if g.holding() {
            g.hold_spawn(false);
            g.record_event("release".into());
        }
    }
    if ui.hidden || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(at) = windows.single().ok().and_then(|w| w.physical_cursor_position()) else { return };
    let (mut group, mut kit) = g.spawn_choice;
    let ids = g.session.sim.spawn_groups(g.player);
    for (n, t, vis, m) in &groups {
        if vis.get()
            && n.contains_point(*t, at)
            && let Some(&id) = ids.get(m.0)
        {
            group = Some(id);
        }
    }
    for (n, t, k) in &kits {
        // (the box's picture is 128 high, the kit 83: the part under the next is the next's)
        let top = t.translation.y - n.size().y / 2.0;
        if n.contains_point(*t, at) && at.y < top + n.size().y * KIT_H / 128.0 {
            kit = k.0;
        }
    }
    for (n, t, p) in &presses {
        if n.contains_point(*t, at) {
            match p {
                Press::Close | Press::Done => {
                    ui.hidden = true;
                    if g.holding() {
                        g.hold_spawn(false);
                        g.record_event("release".into());
                    }
                }
                Press::Board => ui.board = !ui.board,
            }
        }
    }
    if (group, kit) != g.spawn_choice {
        g.choose_spawn(group, kit);
        g.record_event(format!("spawn={}/{kit}", group.map_or("-".to_string(), |g| g.to_string())));
    }
}

/// The mouse freed at a death (to click on the screen), taken at each spawn (the first one too:
/// without it, the first click only took the mouse). Not with a script or the menu up.
fn cursor(g: Res<Game>, mut cursor: Query<&mut CursorOptions>, mut was_dead: Local<bool>, menu: Option<Res<crate::menu::Menu>>) {
    if g.is_added() {
        *was_dead = true;
    }
    let dead = g.dead();
    let Ok(mut c) = cursor.single_mut() else { return };
    if g.script.is_some() || menu.is_some() {
        return;
    }
    if dead && !*was_dead && c.grab_mode != CursorGrabMode::None {
        c.grab_mode = CursorGrabMode::None;
        c.visible = true;
    } else if !dead && *was_dead {
        c.grab_mode = CursorGrabMode::Locked;
        c.visible = false;
    }
    *was_dead = dead;
}
