//! The screen after a level loads, before the spawn screen, as the original's multiplayer briefing (`LoadMenu`, `Briefing/MP_briefing_512x512`):
//! the level's name, the two flags, the game type and the map's, the server's rules, the objectives and the comments, with READY underneath
//! (or Enter). Positions from the menu's file; compared with the original.
//! Level 2: the sim is already running behind it, as on a server. Comes only where the first spawn waits for the player (`--choose`, not `--kit` or a script).
//! The level's commands it reads (`game.setMultiplayerBriefingObjectives`, `…MapType`) and `game.setLoadPicture` are in `Menu/init.con`, which no `Game` command of ours runs: [`level_menu_arg`].

use bevy::prelude::*;

use crate::assets::Cache;
use crate::bmfont::BmText;
use crate::lang::{heading, tr};
use crate::menu::Menu;
use crate::play::Game;
use crate::respawn::At;
use crate::sound::{MENU_HIGHLIGHT, MENU_SOUNDS, MENU_START_GAME, Sounds};

/// The panel's corner on the 800×600 screen, and READY's bar.
const PANEL: (f32, f32) = (144.0, 80.0);
const BAR: (f32, f32) = (144.0, 417.0);
/// READY, in the bar's frame (`Menu/knapp3_*`: 128 px, 109×25 of it the button)
const READY: (f32, f32, f32, f32) = (192.0, 8.0, 109.0, 25.0);
/// The headings' strip (`ColorEffect`, floats in the menu's file: 0.5195, 0.4902, 0.3).
const STRIP: Color = Color::srgb(0.5195, 0.4902, 0.3);
/// Between the objectives' lines (their node's five rows are 15 apart, for the comments; here measured on the capture).
const OBJECTIVES_PITCH: f32 = 12.0;
/// COMMENTS (`Sim::welcome`), as the original's with eight lines, one of them long: five slots of 15 from the panel's 250, the
/// rest not shown; a long line breaks inside its slot, its next pieces 8 lower, over the next slot's
const COMMENTS: (f32, f32, usize) = (250.0, 15.0, 5);
const COMMENT_WRAP_PITCH: f32 = 8.0;
/// Where OBJECTIVES and COMMENTS break their lines: from the text's left to the strip's right edge (x+10 to x+506). The original's lines
/// ask for 492 to 500: a line of 492 stays whole, one of 500 breaks.
const TEXT_WIDTH: f32 = 496.0;
const TEXT: Color = Color::WHITE;

/// Up until READY. `play::start` sets it for each round.
#[derive(Resource, Default)]
pub struct Briefing {
    pub up: bool,
}

#[derive(Component)]
pub struct Screen;
#[derive(Component)]
pub struct Ready;
/// A line in one of the original's fonts: its box's width on the 800×600 screen (the text is scaled with the window).
#[derive(Component, Clone, Copy)]
pub struct Line(f32);

pub struct BriefingPlugin;

impl Plugin for BriefingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Briefing>().add_systems(Update, (setup, rescale).chain().run_if(resource_exists::<Game>)).add_systems(PostUpdate, hide_the_rest.before(bevy::ui::UiSystems::Prepare));
    }
}

/// What a command in the level's `Menu/init.con` says (`game.setLoadPicture Load/Desert.tga`; GC's objectives are a whole sentence in quotes). The last one counts, as when they run: GC_Anchorhead has two pictures.
pub fn level_menu_arg(vfs: &bytefield_vfs::Vfs, level: &str, command: &str) -> Option<String> {
    let bytes = vfs.read(&format!("bf1942/levels/{level}/menu/init.con")).ok()?;
    menu_arg(&String::from_utf8_lossy(&bytes), command)
}

fn menu_arg(text: &str, command: &str) -> Option<String> {
    text.lines().rev().find_map(|l| {
        let (c, rest) = l.trim().split_once(char::is_whitespace)?;
        c.eq_ignore_ascii_case(command).then(|| rest.trim().trim_matches('"').to_string())
    })
}

/// A level's name as the original's screens write it: the lexicon's text under the level's folder name, whatever its case
/// (`Aberdeen`: OPERATION ABERDEEN), else the name as it is.
pub fn level_title(lex: &bytefield_formats::lexicon::Lexicon, level: &str) -> String {
    // (in capitals and with spaces, also without a key: DC AL NAS, GC ANCHORHEAD; the loading panel writes the name as it is)
    match lex.get(level, &crate::settings::language()).map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => t.replace('_', " ").to_uppercase(),
        None => crate::lang::level_fallback(level),
    }
}

/// What the briefing's third row says.
#[derive(Debug, PartialEq)]
enum Third {
    /// Conquest and coop: `TICKET RATIO`.
    Ratio,
    /// CTF and TDM: `SCORE LIMIT`.
    Score,
    /// Objective mode: no third row.
    None,
}

/// How a mode lays the briefing out: the map's type after the mode in the subtitle, the third row, and the lexicon's key of
/// the objectives when the mode has its own text (the same on every level; otherwise, the level's).
fn layout(mode: &str) -> (bool, Third, Option<&'static str>) {
    match mode.to_ascii_uppercase().as_str() {
        "GPM_CTF" => (false, Third::Score, Some("MULTIPLAYER_BRIEFING_CAPTURE_THE_FLAG")),
        "GPM_TDM" => (false, Third::Score, Some("MULTIPLAYER_BRIEFING_TEAM_DEATH_MATCH")),
        "GPM_OBJECTIVEMODE" => (false, Third::None, None),
        _ => (true, Third::Ratio, None),
    }
}

/// The lexicon's key of a game play mode (`session::game_play_mode`).
fn mode_key(mode: &str) -> &'static str {
    match mode.to_ascii_uppercase().as_str() {
        "GPM_CTF" => "CTF",
        "GPM_TDM" => "TDM",
        "GPM_COOP" => "COOP",
        "GPM_OBJECTIVEMODE" => "OBJECTIVEMODE",
        _ => "CONQUEST",
    }
}

fn node(x: f32, y: f32, w: f32, h: f32) -> (Node, At) {
    (Node { position_type: PositionType::Absolute, ..default() }, At(x, y, w, h))
}

/// A line at (x, y) on the 800×600 screen in one of the original's fonts.
// TODO: at 1024×768 some lines sit 1–2 px below the original's (the rules, the objectives, COMMENTS; titles and headings match). The menu
// file's boxes differ from these y (rules at 162, not 160) and the original seems to put the base line at the `.dif`'s line height, not the
// tallest glyph, but no one rounding rule fits all 14 lines measured. At 800×600 the model fits to the pixel
// (what is unknown is the original's rounding at 1,28). A known fault.
fn line(font: &'static str, text: String, color: Color, x: f32, y: f32, width: f32, justify: Justify) -> impl Bundle {
    let mut t = BmText::new(font, color, 1.0, width, justify, false);
    t.text = text;
    (Node { position_type: PositionType::Absolute, ..default() }, At(x, y, width, 20.0), Line(width), t)
}

/// The lines, at the window's scale: one more `fit`, for the glyphs.
fn rescale(windows: Query<&Window>, mut lines: Query<(Ref<Line>, &mut BmText)>, mut last: Local<(f32, f32)>) {
    let Ok(w) = windows.single() else { return };
    let changed = *last != (w.width(), w.height());
    *last = (w.width(), w.height());
    let s = (w.width() / 800.0).min(w.height() / 600.0).max(0.1);
    for (l, mut t) in &mut lines {
        if changed || l.is_added() {
            let new = BmText::new(t.font, t.color, s, l.0 * s, t.justify, t.rim);
            let text = std::mem::take(&mut t.text);
            *t = new;
            t.text = text;
        }
    }
}

/// The HUD does not show behind the briefing: every other root node is hidden, each frame as their systems may show them, and left as it was at READY.
fn hide_the_rest(briefing: Res<Briefing>, game: Option<Res<Game>>, mut roots: Query<(Entity, &mut Visibility), (With<Node>, Without<ChildOf>, Without<Screen>, Without<crate::menu::Root>)>, mut hidden: Local<std::collections::HashMap<Entity, Visibility>>) {
    if briefing.up && game.is_some() {
        for (e, mut v) in &mut roots {
            hidden.entry(e).or_insert(*v);
            *v = Visibility::Hidden;
        }
    } else if !hidden.is_empty() {
        for (e, was) in hidden.drain() {
            if let Ok((_, mut v)) = roots.get_mut(e) {
                *v = was;
            }
        }
    }
}

fn image(h: Option<Handle<Image>>) -> ImageNode {
    h.map(|image| ImageNode { image, image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() }).unwrap_or_default()
}

/// A flag of a team: its soldier's `setTicketIcon` (`flag_ticket_ger`, as the HUD's tickets), or else its `setTeamFlagIcon`.
fn flag(g: &Game, cache: &mut Cache, images: &mut Assets<Image>, team: usize) -> Option<Handle<Image>> {
    let sim = &g.session.sim;
    let skin = sim.modes.conquest.as_ref()?.teams.skins.get(team)?.clone()?;
    let icon = crate::respawn::last_arg(g, &skin, "setTicketIcon").or_else(|| crate::respawn::last_arg(g, &skin, "setTeamFlagIcon"))?;
    let stem = icon.rsplit_once('.').map_or(icon.as_str(), |s| s.0);
    cache.texture(&sim.world.vfs, &format!("menu/Texture/{stem}"), images)
}

fn setup(mut commands: Commands, g: Res<Game>, mut cache: ResMut<Cache>, mut images: ResMut<Assets<Image>>, briefing: Res<Briefing>, old: Query<Entity, With<Screen>>, mut fonts: ResMut<crate::bmfont::BmFonts>, mut made: Local<bool>) {
    if *made && !g.is_added() {
        return;
    }
    *made = true;
    for e in &old {
        commands.entity(e).despawn();
    }
    if !briefing.up {
        return;
    }
    let world = g.session.sim.world;
    let lex = crate::lang::load(&world.vfs);
    let t = |k: &str| tr(&lex, k);
    let level = &world.level.name;
    let menu_text = |c: &str| level_menu_arg(&world.vfs, level, c).map(|k| t(&k)).unwrap_or_default();
    // In CTF and TDM, the mode alone, the score limit and the mode's text; in objective mode, the mode alone, two rows and the level's text; in conquest
    // and coop, the map's type, the ticket ratio and the level's text (`layout`). With no map type, GC's levels: «CONQUEST -», as the original.
    let (map_type, third, mode_text) = layout(&world.mode);
    let subtitle = if map_type { format!("{} - {}", t(mode_key(&world.mode)), menu_text("game.setMultiplayerBriefingMapType")) } else { t(mode_key(&world.mode)) }.to_uppercase();
    let ratio = g.session.sim.modes.conquest.as_ref().map_or(1.0, |c| c.rules.ticket_ratio);
    let third = match third {
        Third::Score => Some(("MULTIPLAYER_BRIEFING_HEADING_4", g.session.sim.rules.score_limit.to_string())),
        Third::Ratio => Some(("MULTIPLAYER_BRIEFING_HEADING_3", format!("{:.0}%", ratio * 100.0))),
        Third::None => None,
    };
    let mut p = |n: &str| cache.texture(&world.vfs, &format!("menu/Texture/{n}"), &mut images);
    let (panel, bar, button) = (p("Briefing/mp_briefing_512x512"), p("Ingame/respawn/ingame_respawn_long_512x64"), p("Menu/knapp3_N"));
    let flags = [flag(&g, &mut cache, &mut images, 0), flag(&g, &mut cache, &mut images, 1)];
    let objectives_text = mode_text.map_or_else(|| menu_text("game.setMultiplayerBriefingObjectives"), |k| t(k));
    let font = fonts.get(&world.vfs, "standard6", &mut images);
    let objectives = font.as_ref().map_or_else(Vec::new, |f| crate::bmfont::wrap(f, objectives_text.trim(), TEXT_WIDTH));
    let (comments_y, comments_pitch, comments_most) = COMMENTS;
    let comments: Vec<Vec<String>> = g.session.sim.rules.welcome.iter().take(comments_most).map(|l| font.as_ref().map_or_else(|| vec![l.clone()], |f| crate::bmfont::wrap(f, l, TEXT_WIDTH))).collect();
    let (x, y) = PANEL;
    commands.spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, GlobalZIndex(11), Screen)).with_children(|c| {
        c.spawn((node(x, y, 512.0, 512.0), image(panel)));
        c.spawn(line("Trebuchet MS18", level_title(&lex, level), Color::BLACK, x + 5.0, y, 500.0, Justify::Center));
        c.spawn((node(x + 190.0, y + 33.0, 16.0, 16.0), image(flags[0].clone())));
        c.spawn((node(x + 300.0, y + 33.0, 16.0, 16.0), image(flags[1].clone())));
        c.spawn(line("standard6", t("MULTIPLAYER_BRIEFING_VS"), Color::BLACK, x + 215.0, y + 34.5, 80.0, Justify::Center));
        c.spawn(line("Trebuchet MS11 - Latin", subtitle, Color::BLACK, x + 4.0, y + 58.0, 504.0, Justify::Center));
        // the server's rules (`Sim::rules`; on a server of ours, the snapshot's): FRIENDLY FIRE is the soldiers' share, the slider's
        // TODO: `SINGLEPLAYER_NR_OF_LIVES` instead of the ratio in a campaign.
        let rules = &g.session.sim.rules;
        let nose_cam = t(if rules.server.nose_cam { "INFO_MENU_ON" } else { "INFO_MENU_OFF" });
        let rows = [Some(("MULTIPLAYER_BRIEFING_HEADING_1", format!("{:.0}%", rules.server.friendly_fire_percent()))), Some(("MULTIPLAYER_BRIEFING_HEADING_2", nose_cam)), third];
        for (i, (key, value)) in rows.into_iter().flatten().enumerate() {
            let ry = y + 80.0 + 15.0 * i as f32;
            c.spawn(line("standard6", t(key), TEXT, x + 10.0, ry, 380.0, Justify::Left));
            c.spawn(line("standard6", value, TEXT, x + 415.0, ry, 80.0, Justify::Right));
        }
        for (sy, key) in [(128.0, "MULTIPLAYER_BRIEFING_OBJECTIVES"), (230.0, "MULTIPLAYER_BRIEFING_COMMENTS")] {
            c.spawn((node(x + 4.0, y + sy, 502.0, 17.0), BackgroundColor(Color::BLACK)));
            c.spawn((node(x + 5.0, y + sy + 1.0, 501.0, 15.0), BackgroundColor(STRIP)));
            c.spawn(line("standard6", t(key), Color::BLACK, x + 10.0, y + sy + 2.0, 200.0, Justify::Left));
        }
        for (i, l) in objectives.iter().enumerate() {
            c.spawn(line("standard6", l.clone(), TEXT, x + 10.0, y + 145.0 + OBJECTIVES_PITCH * i as f32, TEXT_WIDTH, Justify::Left));
        }
        for (i, pieces) in comments.iter().enumerate() {
            for (j, l) in pieces.iter().enumerate() {
                let ly = y + comments_y + comments_pitch * i as f32 + COMMENT_WRAP_PITCH * j as f32;
                c.spawn(line("standard6", l.clone(), TEXT, x + 10.0, ly, TEXT_WIDTH, Justify::Left));
            }
        }
        let (bx, by) = BAR;
        c.spawn((node(bx, by, 512.0, 64.0), image(bar)));
        let (rx, ry, rw, _) = READY;
        c.spawn((node(bx + rx, by + ry, 128.0, 128.0), image(button), Ready));
        c.spawn(line("standard6", heading(&lex, "MULTIPLAYER_BRIEFING_READY"), TEXT, bx + rx, by + ry + 6.0, rw, Justify::Center));
    });
}

/// READY or Enter. Chained after the spawn screen's clicks, which look at `up` first: the same Enter does not also say «done choosing» there.
#[allow(clippy::too_many_arguments)]
pub fn press(
    mut commands: Commands,
    g: Res<Game>,
    mut briefing: ResMut<Briefing>,
    menu: Option<Res<Menu>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut ready: Query<(&ComputedNode, &UiGlobalTransform, &mut ImageNode), With<Ready>>,
    screens: Query<Entity, With<Screen>>,
    mut cache: ResMut<Cache>,
    mut images: ResMut<Assets<Image>>,
    mut sounds: Option<ResMut<Sounds>>,
    mut over: Local<bool>,
) {
    if !briefing.up || menu.is_some() {
        return;
    }
    let vfs = &g.session.sim.world.vfs;
    let at = windows.single().ok().and_then(|w| w.physical_cursor_position());
    let mut on = false;
    for (n, t, mut im) in &mut ready {
        // (the picture is 128 px, the button its top 25×109)
        let (size, centre) = (n.size(), t.translation);
        let (left, top) = (centre.x - size.x / 2.0, centre.y - size.y / 2.0);
        on = at.is_some_and(|a| a.x >= left && a.x < left + size.x * READY.2 / 128.0 && a.y >= top && a.y < top + size.y * READY.3 / 128.0);
        if on != *over {
            let name = if on { "menu/Texture/Menu/knapp3_MO" } else { "menu/Texture/Menu/knapp3_N" };
            if let Some(h) = cache.texture(vfs, name, &mut images) {
                im.image = h;
            }
            if on && let Some(s) = sounds.as_mut() {
                s.play_flat(vfs, MENU_SOUNDS, MENU_HIGHLIGHT);
            }
        }
    }
    *over = on;
    if (on && buttons.just_pressed(MouseButton::Left)) || crate::controls::current().just(&keys, &buttons, "enter") {
        briefing.up = false;
        if let Some(s) = sounds.as_mut() {
            s.play_flat(vfs, MENU_SOUNDS, MENU_START_GAME);
        }
        for e in &screens {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_levels_title_is_the_lexicons_or_its_folder() {
        let mut b = vec![0u8; 4];
        b.extend(2u32.to_le_bytes());
        for s in ["LANGUAGE", "English", "ABERDEEN", "OPERATION ABERDEEN"] {
            b.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
            b.extend([0, 0]);
        }
        let lex = bytefield_formats::lexicon::Lexicon::parse(&b).unwrap();
        assert_eq!(level_title(&lex, "Aberdeen"), "OPERATION ABERDEEN");
        assert_eq!(level_title(&lex, "GC_Anchorhead"), "GC ANCHORHEAD");
        assert_eq!(level_title(&lex, "GC_Endor"), "GC ENDOR");
        assert_eq!(level_title(&lex, "GC_Mini_Caves"), "GC MINI CAVES");
    }

    #[test]
    fn a_levels_title_and_its_modes() {
        assert_eq!(mode_key("GPM_CQ"), "CONQUEST");
        assert_eq!(mode_key("gpm_objectivemode"), "OBJECTIVEMODE");
        assert_eq!(layout("GPM_TDM"), (false, Third::Score, Some("MULTIPLAYER_BRIEFING_TEAM_DEATH_MATCH")));
        assert_eq!(layout("GPM_COOP"), (true, Third::Ratio, None));
        assert_eq!(layout("gpm_objectivemode"), (false, Third::None, None));
    }

    #[test]
    fn what_a_levels_menu_says() {
        let text = "rem x\r\ngame.setLoadPicture a/b.tga\r\nGame.setLoadPicture Load/c.tga\r\ngame.setObjectives \"Two words\"\r\n";
        assert_eq!(menu_arg(text, "game.setLoadPicture").as_deref(), Some("Load/c.tga"));
        assert_eq!(menu_arg(text, "game.setObjectives").as_deref(), Some("Two words"));
        assert_eq!(menu_arg(text, "game.none"), None);
    }

    /// Every level of each mod here names a load picture that is there, or falls to the menu's default.
    #[test]
    fn the_load_pictures_are_there() {
        let game = &bytefield_testdata::game();
        if !game.join("Mods/bf1942/init.con").exists() {
            return;
        }
        for m in ["bf1942", "XPack1", "DC_Final", "GCMOD"] {
            let Ok((_, vfs)) = bytefield_con::session::mount(game, m) else { continue };
            let levels = bytefield_con::session::levels(&vfs);
            assert!(!levels.is_empty(), "{m}");
            let mut missing = Vec::new();
            let mut named = 0;
            for (level, _) in &levels {
                if let Some(p) = level_menu_arg(&vfs, level, "game.setLoadPicture") {
                    named += 1;
                    if crate::menu::load_picture_path(&vfs, level, &p).is_none() {
                        missing.push(format!("{level}: {p}"));
                    }
                }
            }
            eprintln!("{m}: {} levels, {named} name a picture, missing {missing:?}", levels.len());
            assert!(named > 0, "{m}");
            assert!(missing.len() * 10 <= named, "{m}: {missing:?}");
        }
    }
}
