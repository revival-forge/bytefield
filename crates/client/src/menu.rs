//! The menu: the original's screens made again by hand in `bevy_ui`, from what its
//! menu files (`menu.rfa`, MemeFile 2.0) say: where each thing is on an 800×600 screen, the
//! buttons' pictures (normal, under the mouse, pressed) and the lexicon's keys of the texts, in the
//! chosen language. What it does is ours: the main bar, multiplayer → create game (mod, level,
//! game type, team, bots) and start, which loads the round as the command line does.

use std::path::{Path, PathBuf};

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;
use bytefield_formats::lexicon::Lexicon;
use bytefield_vfs::Vfs;

use crate::assets::Cache;
use crate::settings::{LANGUAGES, Settings};

/// The original's menu screen, in its own pixels: everything is placed on it and scaled to the window.
const W: f32 = 800.0;
const H: f32 = 600.0;
/// The game types the bench plays (the others start as conquest: not offered)
const MODES: &[(&str, &str)] = &[("conquest", "CONQUEST"), ("coop", "COOP")];
/// Rows of the levels' list that fit (its area is 260 px high, `CreateGameMenu`)
const LEVEL_ROWS: usize = 17;
/// The rows of the mods' list (`CustomGameMenu`'s list box)
const MOD_ROWS: usize = 6;
const ROW: f32 = 13.0;
const MAX_BOTS: u32 = 64;
/// The controls' rows on the page, and how far apart.
/// The controls' page (`CommonControlsPage1`): 12 rows, 25 apart.
const CONTROL_ROWS: usize = 12;
const CONTROL_PITCH: f32 = 25.0;

/// The texts' colours: beige, a little darker in the lists.
const BUTTON_TEXT: Color = Color::srgb(0.93, 0.89, 0.75);
const LIST_TEXT: Color = Color::srgb(0.82, 0.80, 0.70);
const LIST_PICKED: Color = Color::srgba(0.45, 0.40, 0.25, 0.55);
/// The mods' page's headings (NAME and VERSION, INFO): a bar in a black frame. TODO: the original's `ColorEffect` holds its colour
/// in fields not read yet; this grey is by eye.
const HEADER_FILL: Color = Color::srgb(0.30, 0.29, 0.25);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Main,
    Multiplayer,
    CreateGame,
    /// the mods (`CustomGameMenu`): one is picked and activated, and the menu is that mod's
    CustomGame,
    /// join a server by its address
    Join,
    Options,
    /// the options' graphics (`graphics::VIDEO`), and the experimental ones
    Video,
    Effects,
    /// the keys (`controls.rs`), a map at a time
    Controls,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Act {
    Go(Page),
    /// the same from the options' second bar (its button is pressed on its page only)
    Tab(Page),
    Exit,
    Mod(i32),
    /// a row of the mods' list (its place in the menu's `mods`), and the list's scroll
    PickMod(usize),
    ModScroll(i32),
    /// the picked mod becomes the menu's (`CUSTOM_GAME_ACTIVATE`)
    Activate,
    Level(usize),
    Scroll(i32),
    Mode(usize),
    Team,
    Bots(i32),
    FriendlyFire(i32),
    Start,
    Volume(i32),
    Lang(i32),
    /// all the packs on or off
    Hd,
    /// one pack on or off (its place in `content.cfg`)
    Pack(usize),
    /// a graphics setting one step (`graphics::Row`)
    Gfx(crate::graphics::Row, i32),
    /// the folder typed on the options' page, added (Enter)
    AddFolder,
    Join,
    /// leave the round, back to the menu
    Disconnect,
    /// ask first (`CONFIRMATION_QUIT`, `CONFIRMATION_DISCONNECT_GAME`)
    Ask(Ask),
    /// the question's NO
    No,
    /// the controls' map to show
    ControlsMap(crate::controls::Map),
    /// the map's next or previous page
    ControlsPage(i32),
    /// the map's mouse sensitivity, in tenths
    ControlsMouse(u8),
    /// the map's keyboard sensitivity, in tenths
    ControlsKeyboard(u8),
    ControlsInvert,
    /// to the file (the original's SAVE: until then they last for the session)
    ControlsSave,
    /// wait for the key of this row's first or second column
    Define(usize, usize),
    /// this map's keys as they come
    ControlsDefault,
}

/// What the confirmation asks about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ask {
    Exit,
    Disconnect,
}

/// A level of the mod: its directory's name and the game types it has (of `MODES`).
struct LevelInfo {
    name: String,
    modes: Vec<usize>,
}

#[derive(Resource)]
pub struct Menu {
    game: PathBuf,
    mods: Vec<String>,
    /// what each mod's init.con says of it (the mods' page)
    mod_info: Vec<bytefield_con::session::ModInit>,
    mod_index: usize,
    /// the row picked on the mods' page, and its list's first row
    mod_pick: usize,
    mod_scroll: usize,
    vfs: Vfs,
    /// the mod's and bf1942's
    lexicon: [Lexicon; 2],
    textures: Cache,
    levels: Vec<LevelInfo>,
    level: usize,
    scroll: usize,
    mode: usize,
    team: u32,
    bots: u32,
    /// FRIENDLY FIRE, %
    friendly_fire: u32,
    page: Page,
    dirty: bool,
    /// the question over the screen: only its YES and NO answer
    asking: Option<Ask>,
    /// frames left before loading (the "loading" text shows first)
    loading: Option<u32>,
    /// a check's steps: press start this many frames after opening
    auto_start: Option<u32>,
    frames: u32,
    /// over a round (Esc): the round goes on behind
    pub in_game: bool,
    /// the server's address being typed (`Join`)
    address: String,
    /// why the last join failed
    error: Option<String>,
    /// a step's act, done as a click
    pending: Option<Act>,
    /// the button under the mouse (a new one sounds)
    hovered: Option<Act>,
    /// the controls' page: its map, its first row, the key waited for (row, column), and who lost its key to the last one
    controls_map: crate::controls::Map,
    controls_page: usize,
    /// the page says that SAVE has saved
    controls_saved: bool,
    define: Option<(usize, usize)>,
    conflict: Option<&'static str>,
    /// the press that gave a key is not also a click
    swallow: bool,
    /// a pack's or a mod's folder being typed on the options' page, and what adding the last one said (its line, or why not)
    folder: String,
    folder_said: Option<Result<String, String>>,
}

/// What the menu chose, for `main` to load.
#[derive(Resource)]
pub struct Chosen {
    pub mod_name: String,
    pub level: String,
    pub mode: String,
    pub team: u32,
    pub bots: usize,
    /// FRIENDLY FIRE, %
    pub friendly_fire: u32,
    /// join this server instead (`host[:port]`)
    pub connect: Option<String>,
}

/// The round being played: where it came from (the menu over it is of its mod), and the server
/// run here for it (`--host`), stopped with the round.
#[derive(Resource)]
pub struct Round {
    pub game: PathBuf,
    pub mod_name: String,
    /// whether it runs, and its thread (waited for: the port is free after)
    /// its `World`, leaked for the simulation (`'static`): freed after the teardown (`main::free_world`)
    pub world: usize,
    pub hosting: Option<(std::sync::Arc<std::sync::atomic::AtomicBool>, std::sync::Mutex<Option<std::thread::JoinHandle<()>>>)>,
}

/// A check's steps through the menus (`--steps "60:esc;90:options;120:volume+"`): at each frame of
/// the app, an act as if its button were pressed. `esc` is the key; `type=x` types on the join
/// page; `shot` takes the screenshot (`--screenshot`) and the app ends when it is written.
#[derive(Resource, Default)]
pub struct Steps {
    pub list: Vec<(u32, String)>,
    pub frames: u32,
    esc: bool,
    pub shot: bool,
}

impl Steps {
    pub fn parse(s: &str) -> Result<Steps, String> {
        let mut list = Vec::new();
        for part in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let (f, act) = part.split_once(':').ok_or(format!("--steps: no ':' in {part:?}"))?;
            list.push((f.trim().parse().map_err(|_| format!("--steps: bad frame {f:?}"))?, act.trim().to_string()));
        }
        Ok(Steps { list, ..default() })
    }
}

fn step_act(name: &str) -> Option<Act> {
    Some(match name {
        "main" => Act::Go(Page::Main),
        "multiplayer" => Act::Go(Page::Multiplayer),
        "create" => Act::Go(Page::CreateGame),
        "custom" => Act::Go(Page::CustomGame),
        "mod+" => Act::PickMod(usize::MAX),
        "activate" => Act::Activate,
        "join-page" => Act::Go(Page::Join),
        "options" => Act::Go(Page::Options),
        "start" => Act::Start,
        "join" => Act::Join,
        "disconnect" => Act::Disconnect,
        "exit" => Act::Exit,
        "ask-exit" => Act::Ask(Ask::Exit),
        "ask-disconnect" => Act::Ask(Ask::Disconnect),
        "no" => Act::No,
        "team" => Act::Team,
        "hd" => Act::Hd,
        n if n.starts_with("pack") && n[4..].parse::<usize>().is_ok() => Act::Pack(n[4..].parse().unwrap()),
        "graphics" => Act::Gfx(crate::graphics::Row::Profile, 1),
        "video" => Act::Go(Page::Video),
        "effects" => Act::Go(Page::Effects),
        "controls" => Act::Go(Page::Controls),
        "controls-common" => Act::ControlsMap(crate::controls::Map::Common),
        "controls-land" => Act::ControlsMap(crate::controls::Map::Land),
        "controls-air" => Act::ControlsMap(crate::controls::Map::Air),
        "controls-infantry" => Act::ControlsMap(crate::controls::Map::Infantry),
        "controls-right" => Act::ControlsPage(1),
        "controls-left" => Act::ControlsPage(-1),
        "controls-save" => Act::ControlsSave,
        "controls-default" => Act::ControlsDefault,
        "define" => Act::Define(1, 0),
        "volume+" => Act::Volume(1),
        "volume-" => Act::Volume(-1),
        "lang+" => Act::Lang(1),
        "lang-" => Act::Lang(-1),
        "level+" => Act::Level(usize::MAX),
        // a graphics row by its key: `bloom+`, `aa-`…
        _ => {
            let (key, d) = name.strip_suffix('+').map(|k| (k, 1)).or_else(|| name.strip_suffix('-').map(|k| (k, -1)))?;
            Act::Gfx(crate::graphics::Row::all().find(|r| r.key() == key)?, d)
        }
    })
}

/// Runs the steps due at this frame.
fn run_steps(mut steps: ResMut<Steps>, menu: Option<ResMut<Menu>>) {
    steps.frames += 1;
    let f = steps.frames;
    let due: Vec<String> = steps.list.iter().filter(|(at, _)| *at == f).map(|(_, a)| a.clone()).collect();
    let mut menu = menu;
    for a in due {
        match a.as_str() {
            "esc" => steps.esc = true,
            "shot" => steps.shot = true,
            // (to try the crash's ticket, `tickets.rs`)
            "panic" => panic!("--steps panic"),
            t if t.starts_with("type=") => {
                if let Some(m) = menu.as_mut() {
                    m.address = t[5..].to_string();
                    m.dirty = true;
                }
            }
            // (typed on the options' page and Enter: `folder=~/packs/hd`)
            t if t.starts_with("folder=") => {
                if let Some(m) = menu.as_mut() {
                    m.folder = t[7..].to_string();
                    m.pending = Some(Act::AddFolder);
                }
            }
            other => match (step_act(other), menu.as_mut()) {
                (Some(act), Some(m)) => m.pending = Some(act),
                _ => warn!("--steps: {other:?} at frame {f}: no such act, or no menu"),
            },
        }
    }
}

/// The loading screen's «press escape to cancel» (measured on the original's).
const CANCEL_TEXT: Color = Color::srgb(0.84, 0.725, 0.643);

/// Asks `main` to take the round down (back to the menu).
#[derive(Resource)]
pub struct Teardown;

#[derive(Component)]
pub struct Root;
/// What a round's teardown leaves (the menu's own)
#[derive(Component)]
pub struct Keep;
/// A round has just loaded from the menu. The level loads within one frame, deaf to the keys: an Esc pressed meanwhile arrives in the
/// first frames after it, and is the loading screen's «press escape to cancel», not the menu over the round.
#[derive(Resource)]
pub struct JustLoaded {
    frames: u32,
    page: Page,
}

impl JustLoaded {
    /// `page`: the menu it was started from, where cancelling goes back to (in the original: CREATE GAME).
    pub fn new(page: Page) -> Self {
        Self { frames: 3, page }
    }
}

/// The menu's own camera (there is no level's yet)
#[derive(Component)]
struct MenuCamera;
/// The picture of a button whose area is its parent's
#[derive(Component)]
struct Picture {
    act: Act,
    pictures: Option<[Handle<Image>; 3]>,
}
#[derive(Component)]
struct Button {
    act: Act,
    pictures: Option<[Handle<Image>; 3]>,
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Steps>()
            .add_systems(First, run_steps)
            // (each on its own: Esc takes the Menu away in the sync points of this chain, which panicked its next system)
            .add_systems(Update, (dropped, define_key, build, buttons, wheel, typing, scale, load).chain().distributive_run_if(resource_exists::<Menu>))
            .add_systems(Update, escape.before(crate::play::chat_box).before(define_key).run_if(resource_exists::<crate::play::Game>));
    }
}

/// The text of `key` in the chosen language: the mod's, else bf1942's (a mod's lexicon may lack
/// some), else the key itself.
fn text(lex: &[Lexicon; 2], key: &str) -> String {
    lex.iter().find_map(|l| l.get(key, &crate::settings::language())).unwrap_or(key).to_string()
}

/// The mods of the install and of the folders kept apart,
/// leaving out those that need one that is not there.
fn mods(game: &std::path::Path) -> (Vec<String>, Vec<bytefield_con::session::ModInit>) {
    let all = bytefield_content::detect::mods(game, &bytefield_con::session::mods_dirs());
    for m in all.iter().filter(|m| !m.playable()) {
        warn!("mod {} left out: needs {}", m.folder, m.missing.join(", "));
    }
    all.into_iter().filter(|m| m.playable()).map(|m| (m.folder, m.init)).unzip()
}

/// The levels of a mod's files: each `bf1942/levels/<name>/init.con`, with its game types.
fn levels(vfs: &Vfs) -> Vec<LevelInfo> {
    let mut out: Vec<LevelInfo> = Vec::new();
    for path in vfs.list() {
        let p: Vec<&str> = path.split('/').collect();
        if p.len() == 4 && p[0].eq_ignore_ascii_case("bf1942") && p[1].eq_ignore_ascii_case("levels") && p[3].eq_ignore_ascii_case("init.con") {
            let modes = MODES.iter().enumerate().filter(|(_, (m, _))| vfs.exists(&format!("bf1942/levels/{}/GameTypes/{m}.con", p[2]))).map(|(i, _)| i).collect();
            out.push(LevelInfo { name: p[2].to_string(), modes });
        }
    }
    out.retain(|l| !l.modes.is_empty());
    out
}

impl Menu {
    /// The menu of the install at `game`, for the mod `mod_name` (its menu, texts and levels).
    /// With no `mod_name` (empty): the last one played, else bf1942. The level, team and bots are
    /// the last ones chosen too (`settings`).
    pub fn new(game: PathBuf, mod_name: &str, page: Page, auto_start: Option<u32>, settings: &Settings) -> Result<Menu, String> {
        let (mods, mod_info) = mods(&game);
        let find = |n: &str| mods.iter().position(|m| m.eq_ignore_ascii_case(n));
        let mod_index = if mod_name.is_empty() { find(&settings.last_mod).or_else(|| find("bf1942")) } else { find(mod_name) }.ok_or(format!("no mod {mod_name}"))?;
        let mut m = Menu {
            game,
            mods,
            mod_info,
            mod_index,
            mod_pick: mod_index,
            mod_scroll: mod_index.saturating_sub(MOD_ROWS - 1),
            vfs: Vfs::new(),
            lexicon: Default::default(),
            textures: Cache::default(),
            levels: Vec::new(),
            level: 0,
            scroll: 0,
            mode: 0,
            team: settings.team,
            bots: settings.bots,
            friendly_fire: settings.friendly_fire,
            page,
            dirty: true,
            asking: None,
            loading: None,
            auto_start,
            frames: 0,
            in_game: false,
            address: settings.last_server.clone(),
            error: None,
            pending: None,
            hovered: None,
            controls_map: crate::controls::Map::Common,
            controls_page: 0,
            controls_saved: false,
            define: None,
            conflict: None,
            swallow: false,
            folder: String::new(),
            folder_said: None,
        };
        let base = bytefield_con::session::resolve_ci(&m.game, "Mods/bf1942/lexiconAll.dat").and_then(|p| std::fs::read(p).ok());
        m.lexicon[1] = base.and_then(|b| Lexicon::parse(&b)).unwrap_or_default();
        m.mount()?;
        if let Some(l) = m.levels.iter().position(|l| l.name.eq_ignore_ascii_case(&settings.last_level)) {
            m.level = l;
            m.scroll = l.saturating_sub(LEVEL_ROWS / 2);
            m.mode = m.modes().iter().position(|&i| MODES[i].0 == settings.last_mode).unwrap_or(0);
        }
        Ok(m)
    }

    /// The mod's files, mounted.
    /// A round chosen, its picture on the screen.
    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    pub fn vfs(&self) -> &Vfs {
        &self.vfs
    }

    fn mount(&mut self) -> Result<(), String> {
        let (_, vfs) = bytefield_con::session::mount(&self.game, &self.mods[self.mod_index])?;
        self.lexicon[0] = vfs.read("lexiconAll.dat").ok().and_then(|b| Lexicon::parse(&b)).unwrap_or_default();
        self.levels = levels(&vfs);
        self.vfs = vfs;
        self.textures = Cache::default();
        self.level = self.levels.iter().position(|l| l.name.eq_ignore_ascii_case("El_Alamein")).unwrap_or(0);
        self.scroll = self.level.saturating_sub(LEVEL_ROWS / 2);
        self.mode = 0;
        Ok(())
    }

    pub fn game(&self) -> &Path {
        &self.game
    }

    pub fn mod_name(&self) -> &str {
        &self.mods[self.mod_index]
    }

    /// Build the screen again (its background began to play).
    pub fn redraw(&mut self) {
        self.dirty = true;
    }

    /// A picture of the menu as its files name it (`Menu/knapp_N.tga`: `menu/Texture/…`, a `.dds` or `.tga`).
    fn picture(&mut self, name: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        let stem = name.rsplit_once('.').map_or(name, |s| s.0);
        let path = if stem.starts_with("../") { stem.trim_start_matches("../").to_string() } else { format!("menu/Texture/{stem}") };
        self.textures.texture(&self.vfs, &path, images)
    }

    /// Whether `act` opened the page shown (its button stays pressed)
    fn page_of(&self, act: Act) -> bool {
        match act {
            Act::Go(Page::Multiplayer) => matches!(self.page, Page::Multiplayer | Page::CreateGame | Page::Join),
            Act::Go(Page::Options) => matches!(self.page, Page::Options | Page::Video | Page::Effects | Page::Controls),
            Act::ControlsMap(map) => self.controls_map == map,
            Act::Go(p) | Act::Tab(p) => self.page == p,
            _ => false,
        }
    }

    /// Opened after a join that failed: says why.
    pub fn failed(&mut self, why: String) {
        self.error = Some(why);
        self.page = Page::Join;
    }

    fn modes(&self) -> &[usize] {
        self.levels.get(self.level).map_or(&[], |l| &l.modes)
    }
}

/// Frames the loading screen shows before the level loads (and stops everything: `launch`). `BYTEFIELD_LOAD_FRAMES`, to capture it.
fn load_frames() -> u32 {
    std::env::var("BYTEFIELD_LOAD_FRAMES").ok().and_then(|n| n.parse().ok()).unwrap_or(2)
}

/// Where a level's `setLoadPicture` is in the archives, without its extension. Its name goes from `menu/Texture` (`Load/Desert.tga`; Liberation_of_Caen's XPack levels: `../../bf1942/levels/…`), or is in the level's own `Menu/Texture` (GC's `Load/hoth.tga`).
pub fn load_picture_path(vfs: &Vfs, level: &str, name: &str) -> Option<String> {
    let stem = name.rsplit_once('.').map_or(name, |s| s.0);
    [format!("menu/Texture/{stem}"), format!("bf1942/levels/{level}/menu/texture/{stem}")]
        .into_iter()
        .map(|p| bytefield_vfs::normalize(&p))
        .find(|p| ["dds", "tga"].iter().any(|e| vfs.exists(&format!("{p}.{e}"))))
}

/// The node of a rectangle of the original's screen, relative to its parent.
fn rect(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node { position_type: PositionType::Absolute, left: Val::Px(x), top: Val::Px(y), width: Val::Px(w), height: Val::Px(h), ..default() }
}

fn font(px: f32) -> TextFont {
    TextFont { font_size: FontSize::Px(px), ..default() }
}

/// The menu without its mod's `Movies/background.bik`: the original's plain grey.
const MENU_NO_BACKGROUND: Color = Color::srgb_u8(76, 72, 74);

fn picture(h: Option<Handle<Image>>) -> impl Bundle {
    h.map(|image| ImageNode { image, image_mode: NodeImageMode::Stretch, ..default() }).unwrap_or_default()
}

/// Rebuilds the screen when what it shows changed.
fn build(mut commands: Commands, mut menu: ResMut<Menu>, settings: Res<Settings>, roots: Query<Entity, With<Root>>, cameras: Query<(), With<MenuCamera>>, mut images: ResMut<Assets<Image>>, backdrop: Res<crate::movie::Backdrop>) {
    if cameras.is_empty() {
        // (over the round's, when there is one)
        commands.spawn((Camera2d, Camera { order: 10, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() }, bevy::camera::CompositingSpace::Srgb, MenuCamera, Keep));
    }
    if !menu.dirty {
        return;
    }
    menu.dirty = false;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let m = &mut *menu;
    let [multiplay, options, exit, disconnect, create, join, start, general, video, effects, custom, activate] =
        ["MENU_MULTIPLAY", "MENU_OPTIONS", "MENU_EXIT", "MENU_DISCONNECT", "MENU_CREATE_GAME", "MENU_JOIN", "MENU_START", "GFX_GENERAL", "MENU_VIDEO", "GFX_EXPERIMENTAL", "MENU_CUSTOM_GAME", "CUSTOM_GAME_ACTIVATE"]
            .map(|k| crate::lang::ours(k).map(String::from).unwrap_or_else(|| text(&m.lexicon, k)).trim().trim_end_matches(':').to_string());
    // (`Movies/background.bik` playing, else its still)
    let bg = backdrop.image().or_else(|| (!backdrop.absent()).then(|| m.picture("Menu/Background.tga", &mut images)).flatten());
    let knapp = [m.picture("Menu/knapp_N.tga", &mut images), m.picture("Menu/knapp_MO.tga", &mut images), m.picture("Menu/knapp_MC.tga", &mut images)];
    let knapp2 = [m.picture("Menu/knapp2_N.tga", &mut images), knapp[1].clone(), knapp[2].clone()];
    let knapp3 = [m.picture("Menu/knapp3_N.tga", &mut images), m.picture("Menu/knapp3_MO.tga", &mut images), m.picture("Menu/knapp3_N.tga", &mut images)];
    let pics = |p: &[Option<Handle<Image>>; 3]| p.iter().cloned().collect::<Option<Vec<_>>>().map(|v| [v[0].clone(), v[1].clone(), v[2].clone()]);
    let loading = m.loading.is_some();
    // (`LoadMenu`: the level's `setLoadPicture` at full size; `Load/Pacific.tga` is the node's default. Joining, the level is not known yet)
    let load = loading.then(|| {
        let name = (m.page != Page::Join).then(|| m.levels.get(m.level)).flatten().map(|l| l.name.clone());
        // TODO: GC_Alaris_Prime names a Bink here (`../../mods/GCMOD/Movies/AlarisLS.bik`), GC_Anchorhead a file its archive does not have: both get the default.
        let path = name.as_ref().and_then(|l| load_picture_path(&m.vfs, l, &crate::briefing::level_menu_arg(&m.vfs, l, "game.setLoadPicture")?));
        let bar = [m.picture("Briefing/menu_loading", &mut images), m.picture("loading_full_256x16", &mut images)];
        let picture = match path {
            Some(p) => m.textures.texture(&m.vfs, &p, &mut images),
            None => m.picture("Load/Pacific", &mut images),
        };
        (picture, bar, name.map_or(String::new(), |l| text(&m.lexicon, &l).trim().to_string()))
    });
    let loading_title = text(&m.lexicon, "BRIEFING_LOADING").trim().to_string();
    let cancel_title = text(&m.lexicon, "LOADING_PRESS_ESCAPE_TO_CANCEL").trim().to_string();

    // the whole window, black, over the game's HUD; the screen in it, centred (`scale` sizes it)
    let mut root = commands.spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(Color::BLACK), GlobalZIndex(100), Root, Keep));
    root.with_children(|r| {
        r.spawn((Node { width: Val::Px(W), height: Val::Px(H), overflow: Overflow::clip(), ..default() }, BackgroundColor(if backdrop.absent() { MENU_NO_BACKGROUND } else { Color::NONE }), picture(bg))).with_children(|s| {
            // the main bar (`MainMenuNavigation`): buttons of 97×18 text on 128-wide pictures
            // (the pictures are 128×128, the bar at their top: shown through a 128×32 window)
            let button = |s: &mut ChildSpawnerCommands, x: f32, y: f32, label: String, act: Act, pic: &[Option<Handle<Image>>; 3], on: bool| {
                s.spawn((Node { overflow: Overflow::clip(), ..rect(x, y, 128.0, 32.0) }, Button { act, pictures: None })).with_children(|b| {
                    b.spawn((rect(0.0, 0.0, 128.0, 128.0), picture(pic[if on { 2 } else { 0 }].clone()), Picture { act, pictures: pics(pic) }));
                    // (a long one smaller, on one line: Spanish's «JUEGO MODIFICADO»)
                    let px = if label.chars().count() > 13 { 9.0 } else { 11.0 };
                    b.spawn(Node { justify_content: JustifyContent::Center, ..rect(8.0, 9.0, 97.0, 18.0) }).with_child((Text::new(label), font(px), TextColor(BUTTON_TEXT), TextLayout { linebreak: LineBreak::NoWrap, ..default() }));
                });
            };
            let page = m.page;
            let multi = matches!(page, Page::Multiplayer | Page::CreateGame | Page::Join);
            button(s, 4.0, 4.0, multiplay, Act::Go(Page::Multiplayer), &knapp, multi);
            let opts = matches!(page, Page::Options | Page::Video | Page::Effects | Page::Controls);
            button(s, 112.0, 4.0, options, Act::Go(Page::Options), &knapp, opts);
            if m.in_game {
                // (`ExitMenu`: MENU_DISCONNECT)
                button(s, 220.0, 4.0, disconnect, Act::Ask(Ask::Disconnect), &knapp, false);
            } else {
                // after OPTIONS, as in the original's bar (`MainNavigationCustomGame`); not over a round, whose mod is the server's
                button(s, 220.0, 4.0, custom, Act::Go(Page::CustomGame), &knapp, page == Page::CustomGame);
            }
            button(s, 328.0, 4.0, exit, Act::Ask(Ask::Exit), &knapp, false);
            if multi {
                // the second bar (`MultiplayerNavigation`, 108 apart)
                button(s, 112.0, 30.0, create, Act::Go(Page::CreateGame), &knapp2, page == Page::CreateGame);
                button(s, 220.0, 30.0, join.clone(), Act::Go(Page::Join), &knapp2, page == Page::Join);
            }
            if page == Page::CreateGame {
                create_game(s, m, &mut images);
                // `CreateGameNavigation` (500, 535)
                button(s, 660.0, 545.0, start, Act::Start, &knapp3, false);
            }
            if page == Page::CustomGame {
                custom_game(s, m, &mut images);
                // `CustomGameNavigation` (500, 535): ACTIVATE at 62, VISIT WEB PAGE at 170 (where CREATE GAME has START). The
                // web page is left out: it would open a browser on whatever a mod's init.con says.
                button(s, 552.0, 545.0, activate, Act::Activate, &knapp3, false);
            }
            if page == Page::Join {
                join_page(s, m, &mut images);
                button(s, 660.0, 545.0, join, Act::Join, &knapp3, false);
            }
            if opts {
                // ours, like the multiplayer's second bar: the general options, then the graphics
                button(s, 112.0, 30.0, general, Act::Tab(Page::Options), &knapp2, page == Page::Options);
                button(s, 220.0, 30.0, video, Act::Go(Page::Video), &knapp2, page == Page::Video);
                button(s, 328.0, 30.0, effects, Act::Go(Page::Effects), &knapp2, page == Page::Effects);
                button(s, 436.0, 30.0, text(&m.lexicon, "MENU_CONTROLS").trim().to_string(), Act::Go(Page::Controls), &knapp2, page == Page::Controls);
            }
            if page == Page::Controls {
                // (`ControlsNavigation`: under the page's own button, as the original's third bar)
                button(s, 328.0, 56.0, text(&m.lexicon, "MENU_DEFAULT").trim().to_string(), Act::ControlsDefault, &knapp3, false);
                button(s, 436.0, 56.0, text(&m.lexicon, "MENU_SAVE").trim().to_string(), Act::ControlsSave, &knapp3, false);
            }
            match page {
                Page::Options => options_page(s, m, &settings, &mut images),
                Page::Video => graphics_page(s, m, &settings, &mut images, crate::graphics::VIDEO),
                Page::Effects => graphics_page(s, m, &settings, &mut images, crate::graphics::EFFECTS),
                Page::Controls => controls_page(s, m, &mut images),
                _ => {}
            }
            if let Some(ask) = m.asking {
                // the original's: the screen dimmed, a box in the middle
                let (question, yes) = match ask {
                    Ask::Exit => ("CONFIRMATION_QUIT", Act::Exit),
                    Ask::Disconnect => ("CONFIRMATION_DISCONNECT_GAME", Act::Disconnect),
                };
                let say = |k: &str| text(&m.lexicon, k).trim().to_string();
                s.spawn((rect(0.0, 0.0, W, H), BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45))));
                s.spawn((rect(280.0, 200.0, 278.0, 112.0), BackgroundColor(Color::srgb(0.45, 0.44, 0.41)))).with_children(|b| {
                    b.spawn((rect(9.0, 8.0, 260.0, 18.0), Text::new(say("CONFIRMATION_HEADING")), font(11.0), TextColor(BUTTON_TEXT)));
                    b.spawn((Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..rect(5.0, 27.0, 268.0, 80.0) }, BackgroundColor(Color::srgb(0.16, 0.16, 0.16)))).with_children(|d| {
                        d.spawn((Node { margin: UiRect::bottom(Val::Px(30.0)), ..default() }, Text::new(say(question)), font(11.0), TextColor(BUTTON_TEXT), TextLayout { justify: Justify::Center, ..default() }));
                    });
                });
                button(s, 285.0, 271.0, say("CONFIRMATION_YES"), yes, &knapp, false);
                button(s, 423.0, 271.0, say("CONFIRMATION_NO"), Act::No, &knapp, false);
            }
            if let Some((pic, [panel, fill], level)) = load {
                // (the bar stands full: the level loads in one frame, `launch`, 0,5 s from the disk cache, with nothing drawn until it ends. The user, 4-nov: it stays so)
                s.spawn((rect(0.0, 0.0, W, H), BackgroundColor(Color::BLACK), picture(pic)));
                s.spawn((rect(260.0, 465.0, 512.0, 64.0), picture(panel))).with_children(|p| {
                    let mut title = crate::bmfont::BmText::new("Trebuchet MS8", Color::BLACK, 1.0, 268.0, Justify::Left, false);
                    title.text = format!("{loading_title} {level}").trim().to_string();
                    p.spawn((rect(10.0, 6.0, 268.0, 16.0), title));
                    p.spawn((rect(17.0, 33.0, 260.0, 18.0), BackgroundColor(Color::srgb(0.5468, 0.5468, 0.5468))));
                    p.spawn((rect(18.0, 34.0, 257.0, 16.0), BackgroundColor(Color::BLACK)));
                    p.spawn((rect(19.0, 35.0, 256.0, 14.0), picture(fill)));
                });
                let mut cancel = crate::bmfont::BmText::new("standard6", CANCEL_TEXT, 1.0, 400.0, Justify::Center, false);
                cancel.text = cancel_title;
                s.spawn((rect(200.0, 533.5, 400.0, 12.0), cancel));
            }
        });
    });
}

/// A panel like create game's settings: a title and rows of `label  < value >`.
fn panel(s: &mut ChildSpawnerCommands, m: &mut Menu, images: &mut Assets<Image>, title: String, rows: Vec<(String, String, Act, Act)>, note: Option<String>) {
    let xl = m.picture("Menu/menu_creategameXL_512x512.tga", images);
    let left = [m.picture("Menu/buttons/menu_rubr_pilV_16x32.tga", images), m.picture("Menu/buttons/menu_rubr_pilV_MO_16x32.tga", images), m.picture("Menu/buttons/menu_rubr_pilV_MO_16x32.tga", images)];
    let right = [m.picture("Menu/buttons/menu_rubr_pilH_16x32.tga", images), m.picture("Menu/buttons/menu_rubr_pilH_MO_16x32.tga", images), m.picture("Menu/buttons/menu_rubr_pilH_MO_16x32.tga", images)];
    let pics = |p: &[Option<Handle<Image>>; 3]| p.iter().cloned().collect::<Option<Vec<_>>>().map(|v| [v[0].clone(), v[1].clone(), v[2].clone()]);
    let n_rows = rows.len();
    // (more than ten rows closer together, inside the frame's dark part)
    let pitch = (270.0 / n_rows.saturating_sub(1).max(1) as f32).min(30.0);
    s.spawn(Node { overflow: Overflow::clip(), ..rect(10.0, 100.0, 780.0, 440.0) }).with_children(|f| {
        f.spawn((rect(15.0, 25.0, 512.0, 512.0), picture(xl))).with_children(|p| {
            p.spawn(rect(17.0, 8.0, 300.0, 20.0)).with_child((Text::new(title), font(12.0), TextColor(BUTTON_TEXT)));
            for (k, (label, value, less, more)) in rows.into_iter().enumerate() {
                let y = 45.0 + pitch * k as f32;
                p.spawn(rect(17.0, y + 4.0, 170.0, 20.0)).with_child((Text::new(label), font(11.0), TextColor(LIST_TEXT)));
                if less != more || matches!(less, Act::Team | Act::Hd | Act::Pack(_)) {
                    p.spawn((rect(190.0, y - 4.0, 16.0, 32.0), picture(left[0].clone()), Button { act: less, pictures: pics(&left) }));
                    p.spawn((rect(320.0, y - 4.0, 16.0, 32.0), picture(right[0].clone()), Button { act: more, pictures: pics(&right) }));
                }
                p.spawn(Node { justify_content: JustifyContent::Center, ..rect(208.0, y + 4.0, 110.0, 20.0) }).with_child((Text::new(value), font(11.0), TextColor(LIST_TEXT)));
            }
            if let Some(n) = note {
                p.spawn(rect(17.0, (45.0 + pitch * n_rows as f32).max(200.0), 330.0, 60.0)).with_child((Text::new(n), font(11.0), TextColor(Color::srgb(0.95, 0.55, 0.4))));
            }
        });
    });
}

/// A folder dropped on the menu's window: a pack, a mod or a folder of mods, added as
/// `bytefield-content add` does and saved; the mods' list and the options' pack rows follow at once.
fn dropped(mut drops: MessageReader<bevy::window::FileDragAndDrop>, mut menu: ResMut<Menu>, mut settings: ResMut<Settings>) {
    for d in drops.read() {
        let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = d else { continue };
        add_folder(&mut menu, &mut settings, path_buf);
    }
}

/// A folder added, dropped or typed: saved, and the mods' list and the pack rows follow.
fn add_folder(m: &mut Menu, settings: &mut Settings, dir: &std::path::Path) {
    let said = settings.content.add_dir(dir);
    m.dirty = true;
    match &said {
        Ok(line) => info!("added {line}"),
        Err(e) => warn!("{e}"),
    }
    let ok = said.is_ok();
    m.folder_said = Some(said);
    if !ok {
        return;
    }
    m.folder.clear();
    settings.save();
    settings.apply_content(None, None);
    let current = m.mods[m.mod_index].clone();
    (m.mods, m.mod_info) = mods(&m.game);
    m.mod_index = m.mods.iter().position(|x| *x == current).unwrap_or(0);
    m.mod_pick = m.mod_index;
    if let Err(e) = m.mount() {
        warn!("{e}");
    }
}

/// A typed folder as a path: `~` is the home folder.
fn typed_path(typed: &str) -> PathBuf {
    let t = typed.trim();
    match (t.strip_prefix('~'), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => PathBuf::from(home).join(rest.trim_start_matches('/')),
        _ => PathBuf::from(t),
    }
}

/// The end of a long typed text, as much as the value's box holds.
fn tail(s: &str, chars: usize) -> String {
    let n = s.chars().count();
    if n <= chars { s.to_string() } else { format!("…{}", s.chars().skip(n + 1 - chars).collect::<String>()) }
}

/// Packs that get a row each on the options' page; more, one row for all.
const MAX_PACK_ROWS: usize = 5;

/// The options we have (the original's are spread over `GeneralOptionsMenu`, `ControlsMenu` and
/// `SoundMenu`): the master volume, the texts' language, and the view's (`graphics::GENERAL`). The mouse's are on the controls' page, a map each.
fn options_page(s: &mut ChildSpawnerCommands, m: &mut Menu, settings: &Settings, images: &mut Assets<Image>) {
    let t = |k: &str| text(&m.lexicon, k);
    let rows = vec![
        (t("SOUND_OPTIONS_MASTER_VOLUME"), format!("{}%", (settings.volume * 100.0).round()), Act::Volume(-1), Act::Volume(1)),
        (crate::lang::ours("LANGUAGE").unwrap_or("LANGUAGE").to_string(), native(&crate::settings::language()).to_string(), Act::Lang(-1), Act::Lang(1)),
    ];
    let mut rows = rows;
    // (ours, from the next round on: the graphics packs if any, one row each while they fit)
    // (a folder dropped on the window, or typed here and Enter, is added as `bytefield-content add` does: [`add_folder`])
    rows.push((crate::lang::ours("ADD_FOLDER").unwrap_or("ADD FOLDER").to_string(), format!("{}_", tail(&m.folder, 16)), Act::AddFolder, Act::AddFolder));
    let packs = &settings.content.packs;
    let yes_no = |on: bool| if on { t("CONFIRMATION_YES") } else { t("CONFIRMATION_NO") };
    if std::env::var_os("BYTEFIELD_HD").is_some() || packs.len() > MAX_PACK_ROWS {
        rows.push((crate::lang::ours("HD_PACKS").unwrap_or("HD PACKS").to_string(), yes_no(settings.packs_on()), Act::Hd, Act::Hd));
    } else {
        for (i, p) in packs.iter().enumerate() {
            rows.push((p.name.to_uppercase(), yes_no(p.on), Act::Pack(i), Act::Pack(i)));
        }
    }
    // (the view's distance, the field of view and the vsync: `graphics::GENERAL`)
    let tg = |k: &str| crate::lang::ours(k).map(String::from).unwrap_or_else(|| text(&m.lexicon, k)).trim().trim_end_matches(':').trim().to_string();
    for &r in crate::graphics::GENERAL {
        rows.push((tg(r.label()), r.value(settings, &tg), Act::Gfx(r, -1), Act::Gfx(r, 1)));
    }
    let title = t("MENU_OPTIONS");
    // (ours: version, licence and the trademark notice, as an «About»; above it, what the last folder added said)
    let about = crate::lang::about();
    let note = match &m.folder_said {
        Some(Ok(line)) => match line.split_once(": ") {
            Some((what, path)) => format!("{what}: {}\n\n{about}", tail(path, 40)),
            None => format!("{line}\n\n{about}"),
        },
        Some(Err(e)) => format!("{}: {e}\n\n{about}", crate::lang::ours("FOLDER_NOT_ADDED").unwrap_or("NOT ADDED")),
        None => about,
    };
    panel(s, m, images, title, rows, Some(note));
}

/// The graphics settings of a page: the original's names where its video menu has
/// them. Those marked `*` wait for the next round.
fn graphics_page(s: &mut ChildSpawnerCommands, m: &mut Menu, settings: &Settings, images: &mut Assets<Image>, page: &[crate::graphics::Row]) {
    let t = |k: &str| crate::lang::ours(k).map(String::from).unwrap_or_else(|| text(&m.lexicon, k)).trim().trim_end_matches(':').trim().to_string();
    let rows = page
        .iter()
        .map(|&r| {
            let label = t(r.label()) + if r.next_round() { " *" } else { "" };
            (label, r.value(settings, &t), Act::Gfx(r, -1), Act::Gfx(r, 1))
        })
        .collect();
    let title = t(if page == crate::graphics::VIDEO { "MENU_VIDEO" } else { "GFX_EXPERIMENTAL" });
    let note = page.iter().any(|r| r.next_round()).then(|| t("GFX_NOTE"));
    panel(s, m, images, title, rows, note);
}

/// The original's page (`CommonControlsMenu`, `CommonControlsPage1`…): the tabs on the big
/// panel, 12 rows a page with a key and a second key, and on the small panel the map's mouse. A click waits for the key
/// ([`define_key`]). The two panels are where its menus say, and what lies on them is measured on its capture.
fn controls_page(s: &mut ChildSpawnerCommands, m: &mut Menu, images: &mut Assets<Image>) {
    use crate::controls::Map;
    let map = m.controls_map;
    let say = |m: &Menu, k: &str| text(&m.lexicon, k).trim().to_string();
    let tab = 1 + Map::ALL.iter().position(|x| *x == map).unwrap_or(0);
    let (xl, tabs) = (m.picture("Menu/options/menu_control_XL_512x512.tga", images), m.picture(&format!("Menu/options/menu_control{tab}_tab_512x32.tga"), images));
    let small = m.picture("Menu/options/menu_control_small_512x256.tga", images);
    let (left, right) = (m.picture("Menu/buttons/menu_rubr_pilV_16x32.tga", images), m.picture("Menu/buttons/menu_rubr_pilH_16x32.tga", images));
    let c = crate::controls::current();
    let rows = c.rows(map);
    let pages = rows.len().div_ceil(CONTROL_ROWS).max(1);
    let page = m.controls_page.min(pages - 1);
    let footer = format!("{}  {}/{}", say(m, map.label()), page + 1, pages);
    // (a key the lexicon does not have comes back as it is: a letter's, a number's)
    let key_text = |m: &Menu, i: crate::controls::Input| match crate::controls::lexicon_key(i).map(|k| (say(m, &k), k)).filter(|(t, k)| t != k) {
        Some((t, _)) => t,
        None => crate::controls::input_name(i).unwrap_or("").to_uppercase(),
    };
    let lines: Vec<_> = rows.into_iter().enumerate().skip(page * CONTROL_ROWS).take(CONTROL_ROWS).map(|(i, r)| (i, say(m, r.label), r.keys.map(|k| k.map(|k| key_text(m, k))))).collect();
    let names = Map::ALL.map(|x| say(m, x.label()));
    let (mouse_label, keyboard_label, invert_label) = (say(m, "CONTROLS_MOUSE_SENSITIVITY"), say(m, "CONTROLS_KEYBOARD_SENSITIVITY"), say(m, "CONTROLS_INVERT_MOUSE"));
    let mut note = m.conflict.map(|x| format!("{}: {}", say(m, "CONTROLS_CONFLICT_DETECTED"), say(m, x)));
    if m.controls_saved {
        note = Some(say(m, "CONFIRMATION_CONTROLS_SUCCESS"));
    }
    if let Some((_, column)) = m.define {
        note = Some(say(m, if column == 0 { "CONTROLS_DEFINE_KEY" } else { "CONTROLS_DEFINE_ALT_KEY" }));
    }
    let cell = |p: &mut ChildSpawnerCommands, x: f32, y: f32, text_of: String, waiting: bool, act: Act| {
        p.spawn((rect(x, y, 90.0, 18.0), BackgroundColor(Color::srgb(0.75, 0.75, 0.72)), Button { act, pictures: None })).with_children(|b| {
            let inner = if waiting { LIST_PICKED } else { Color::BLACK };
            b.spawn((Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..rect(1.0, 1.0, 88.0, 16.0) }, BackgroundColor(inner))).with_child((Text::new(text_of), font(9.5), TextColor(Color::WHITE)));
        });
    };
    s.spawn((rect(27.0, 125.0, 512.0, 512.0), picture(xl))).with_children(|p| {
        p.spawn((rect(0.0, 0.0, 512.0, 32.0), picture(tabs)));
        // (the tabs' places on the picture)
        for (i, (x, w)) in [(0.0, 84.0), (88.0, 84.0), (176.0, 84.0), (264.0, 132.0)].into_iter().enumerate() {
            p.spawn((Node { justify_content: JustifyContent::Center, ..rect(x, 0.0, w, 24.0) }, Button { act: Act::ControlsMap(Map::ALL[i]), pictures: None })).with_child((Text::new(names[i].clone()), font(10.5), TextColor(Color::BLACK), Node { top: Val::Px(7.0), ..default() }));
        }
        for (n, (i, label, keys)) in lines.into_iter().enumerate() {
            let y = 35.0 + CONTROL_PITCH * n as f32;
            p.spawn(rect(10.0, y + 2.0, 180.0, 16.0)).with_child((Text::new(label), font(9.5), TextColor(Color::WHITE)));
            for (column, key) in keys.into_iter().enumerate() {
                cell(p, 190.0 + 100.0 * column as f32, y, key.unwrap_or_default(), m.define == Some((i, column)), Act::Define(i, column));
            }
        }
        p.spawn(Node { justify_content: JustifyContent::Center, ..rect(10.0, 353.0, 376.0, 16.0) }).with_child((Text::new(footer), font(10.5), TextColor(Color::BLACK)));
        if page > 0 {
            p.spawn((rect(6.0, 347.0, 16.0, 24.0), picture(left), Button { act: Act::ControlsPage(-1), pictures: None }));
        }
        if page + 1 < pages {
            p.spawn((rect(375.0, 347.0, 16.0, 24.0), picture(right), Button { act: Act::ControlsPage(1), pictures: None }));
        }
    });
    let slider = |p: &mut ChildSpawnerCommands, y: f32, label: String, v: f32, act: fn(u8) -> Act| {
        p.spawn(rect(15.0, y, 200.0, 16.0)).with_child((Text::new(label), font(9.5), TextColor(Color::WHITE)));
        // (`BfSliderNode`, 0–1 in tenths; the knob's measured place at 0.25 and 0.5 says so)
        p.spawn((rect(218.0, y + 1.0, 76.0, 11.0), BackgroundColor(Color::srgb(0.75, 0.75, 0.72))));
        p.spawn((rect(219.0, y + 2.0, 74.0, 9.0), BackgroundColor(Color::BLACK)));
        p.spawn((rect(220.0 + v.clamp(0.0, 1.0) * 64.0, y + 3.0, 7.0, 7.0), BackgroundColor(Color::WHITE)));
        for i in 0..=10u8 {
            let w = 76.0 / 11.0;
            p.spawn((rect(218.0 + w * i as f32, y - 2.0, w, 17.0), Button { act: act(i), pictures: None }));
        }
    };
    s.spawn((rect(450.0, 125.0, 512.0, 256.0), picture(small))).with_children(|p| {
        slider(p, 40.0, mouse_label, c.mouse(map), Act::ControlsMouse);
        let mut y = 65.0;
        if map.has_keyboard() {
            slider(p, y, keyboard_label, c.keyboard(map), Act::ControlsKeyboard);
            y += 28.0;
        } else {
            y += 3.0;
        }
        if map.has_invert() {
            p.spawn(rect(15.0, y, 200.0, 16.0)).with_child((Text::new(invert_label), font(9.5), TextColor(Color::WHITE)));
            p.spawn((rect(218.0, y - 4.0, 20.0, 20.0), BackgroundColor(Color::srgb(0.75, 0.75, 0.72)), Button { act: Act::ControlsInvert, pictures: None })).with_children(|b| {
                b.spawn((rect(1.0, 1.0, 18.0, 18.0), BackgroundColor(Color::BLACK)));
                if c.invert(map) {
                    b.spawn((rect(5.0, 5.0, 10.0, 10.0), BackgroundColor(Color::srgb(0.5, 0.52, 0.27))));
                }
            });
        }
        if let Some(n) = note {
            p.spawn(rect(15.0, 180.0, 300.0, 30.0)).with_child((Text::new(n), font(9.5), TextColor(Color::srgb(0.95, 0.75, 0.4))));
        }
    });
}

/// After a click on a row of the controls, the next press is its key; Esc leaves it as it was, and what cannot be
/// defined (Enter, the console's) is waited past.
fn define_key(mut menu: ResMut<Menu>, keys: Res<ButtonInput<KeyCode>>, mouse: Res<ButtonInput<MouseButton>>, mut wheel: MessageReader<MouseWheel>) {
    let turned: f32 = wheel.read().map(|e| e.y).sum();
    let Some((row, column)) = menu.define else { return };
    use crate::controls::{Controls, Input};
    if keys.just_pressed(KeyCode::Escape) {
        menu.define = None;
        menu.dirty = true;
        return;
    }
    let Some(input) = crate::controls::pressed_now(&keys, &mouse, turned) else { return };
    let map = menu.controls_map;
    if !Controls::can_define(map, input) {
        menu.swallow = matches!(input, Input::Button(_));
        return;
    }
    let mut c = crate::controls::current();
    menu.conflict = c.define(map, row, column, input);
    crate::controls::set(c);
    menu.define = None;
    menu.swallow = matches!(input, Input::Button(_));
    menu.dirty = true;
}

/// A language's own name (the CJK ones stay in English: the font has not their letters).
fn native(l: &str) -> &str {
    match l {
        "French" => "Français",
        "Italian" => "Italiano",
        "Spanish" => "Español",
        "German" => "Deutsch",
        other => other,
    }
}

/// Join a server by its address (the original's `LocalMenu`/`InternetMenu` list servers; ours is
/// typed): the address, and the team.
fn join_page(s: &mut ChildSpawnerCommands, m: &mut Menu, images: &mut Assets<Image>) {
    let t = |k: &str| text(&m.lexicon, k);
    let team = if m.team == 1 { t("SINGLEPLAYER_TEAM_AXIS") } else { t("SINGLEPLAYER_TEAM_ALLIES") };
    let rows = vec![(t("ADD_SERVER_IP"), format!("{}_", m.address), Act::Join, Act::Join), (t("SINGLEPLAYER_TEAM"), team, Act::Team, Act::Team)];
    let title = t("MENU_JOIN");
    let note = m.error.clone().map(|e| format!("{}\n{e}", t("CONFIRMATION_FAILED_TO_CONNECT")));
    panel(s, m, images, title, rows, note);
}

/// `CustomGameMenu`: a frame of 256×384 at 300,125 (`menu_mod_256x256` over `menu_mod_2_256x128`), with the mods' list
/// (name and version, in the picture's first dark box) and the picked one's `CUSTOM_GAME_INFO`: its `setCustomGameUrl` line and its `setCustomGameInfo`
/// (a lexicon key or the text). The active one is marked; ACTIVATE makes the picked one the menu's.
fn custom_game(s: &mut ChildSpawnerCommands, m: &mut Menu, images: &mut Assets<Image>) {
    let t = |m: &Menu, k: &str| text(&m.lexicon, k).trim().to_string();
    let top = m.picture("Menu/multiplayer/menu_mod_256x256.tga", images);
    let bottom = m.picture("Menu/multiplayer/menu_mod_2_256x128.tga", images);
    let line = m.picture("Menu/buttons/menu_rubr_line_4x32.tga", images);
    let up = m.picture("Menu/buttons/menu_scrollpilupp_16x8.tga", images);
    let down = m.picture("Menu/buttons/menu_scrollpilner_16x8.tga", images);
    let (title, name_h, version_h, info_h) = (t(m, "MENU_CUSTOM_GAME"), t(m, "CUSTOM_GAME_NAME"), t(m, "CUSTOM_GAME_VERSION"), t(m, "CUSTOM_GAME_INFO"));
    let rows: Vec<(usize, String, String)> = m.mods.iter().zip(&m.mod_info).enumerate().skip(m.mod_scroll).take(MOD_ROWS).map(|(i, (f, init))| (i, f.clone(), init.version.clone().unwrap_or_default())).collect();
    let picked = m.mod_info.get(m.mod_pick).cloned().unwrap_or_default();
    // (a key of the mod's lexicon or of bf1942's, else the text as it is)
    let info = picked.info.map(|i| text(&m.lexicon, &i)).unwrap_or_default();
    let about = [picked.url.unwrap_or_default(), info].into_iter().filter(|x| !x.trim().is_empty()).collect::<Vec<_>>().join("\n\n");
    let (active, pick, more) = (m.mod_index, m.mod_pick, m.mods.len() > MOD_ROWS);
    let header = |p: &mut ChildSpawnerCommands, y: f32, left: String, right: Option<String>| {
        p.spawn((rect(4.0, y, 232.0, 18.0), BackgroundColor(Color::BLACK))).with_children(|h| {
            h.spawn((rect(1.0, 1.0, 230.0, 16.0), BackgroundColor(HEADER_FILL)));
            h.spawn(rect(20.0, 3.0, 150.0, 14.0)).with_child((Text::new(left), font(10.5), TextColor(LIST_TEXT)));
            if let Some(r) = right {
                h.spawn((rect(170.0, -1.0, 4.0, 22.0), picture(line.clone())));
                h.spawn(rect(178.0, 3.0, 54.0, 14.0)).with_child((Text::new(r), font(10.5), TextColor(LIST_TEXT)));
            }
        });
    };
    s.spawn(rect(300.0, 125.0, 256.0, 384.0)).with_children(|f| {
        f.spawn((rect(0.0, 0.0, 256.0, 256.0), picture(top)));
        f.spawn((rect(0.0, 256.0, 256.0, 128.0), picture(bottom)));
        f.spawn(rect(12.0, 8.0, 230.0, 20.0)).with_child((Text::new(title), font(12.0), TextColor(BUTTON_TEXT)));
        header(f, 34.0, name_h, Some(version_h));
        if more {
            f.spawn((rect(237.0, 56.0, 16.0, 8.0), picture(up), Button { act: Act::ModScroll(-1), pictures: None }));
            f.spawn((rect(237.0, 52.0 + ROW * MOD_ROWS as f32 - 4.0, 16.0, 8.0), picture(down), Button { act: Act::ModScroll(1), pictures: None }));
        }
        for (n, (i, folder, version)) in rows.into_iter().enumerate() {
            let bg = if i == pick { LIST_PICKED } else { Color::NONE };
            let mark = if i == active { "» " } else { "" };
            f.spawn((rect(4.0, 54.0 + ROW * n as f32, 232.0, ROW), BackgroundColor(bg), Button { act: Act::PickMod(i), pictures: None })).with_children(|r| {
                r.spawn(rect(6.0, 0.0, 160.0, ROW)).with_child((Text::new(format!("{mark}{folder}")), font(10.5), TextColor(LIST_TEXT)));
                r.spawn(rect(178.0, 0.0, 54.0, ROW)).with_child((Text::new(version), font(10.5), TextColor(LIST_TEXT)));
            });
        }
        // (in the picture's second dark box, the text running on into the third)
        header(f, 151.0, info_h, None);
        f.spawn((Node { overflow: Overflow::clip(), ..rect(10.0, 174.0, 224.0, 190.0) },)).with_child((Text::new(about), font(10.0), TextColor(LIST_TEXT)));
    });
}

/// `CreateGameMenu` (in its frame at 10,100): the settings (left), the level's picture and the
/// levels (middle), the game types (right). The settings are ours: the original's are a server's.
fn create_game(s: &mut ChildSpawnerCommands, m: &mut Menu, images: &mut Assets<Image>) {
    let t = |m: &Menu, k: &str| text(&m.lexicon, k);
    let xl = m.picture("Menu/menu_creategameXL_512x512.tga", images);
    let karta = m.picture("Menu/menu_creategame_karta_256x128.tga", images);
    let list = m.picture("Menu/menu_creategame_List_256x512.tga", images);
    let types = m.picture("Menu/menu_creategameType_256x128.tga", images);
    let up = m.picture("Menu/buttons/menu_scrollpilupp_16x8.tga", images);
    let down = m.picture("Menu/buttons/menu_scrollpilner_16x8.tga", images);
    let left = [m.picture("Menu/buttons/menu_rubr_pilV_16x32.tga", images), m.picture("Menu/buttons/menu_rubr_pilV_MO_16x32.tga", images), m.picture("Menu/buttons/menu_rubr_pilV_MO_16x32.tga", images)];
    let right = [m.picture("Menu/buttons/menu_rubr_pilH_16x32.tga", images), m.picture("Menu/buttons/menu_rubr_pilH_MO_16x32.tga", images), m.picture("Menu/buttons/menu_rubr_pilH_MO_16x32.tga", images)];
    let pics = |p: &[Option<Handle<Image>>; 3]| p.iter().cloned().collect::<Option<Vec<_>>>().map(|v| [v[0].clone(), v[1].clone(), v[2].clone()]);
    let thumb = m.levels.get(m.level).map(|l| format!("../bf1942/levels/{}/Menu/thumbnail.tga", l.name)).and_then(|n| m.picture(&n, images));
    // TODO: a level without a lexicon key shows here as «GC Endor»; not measured against the original's LEVELS list (GCMOD, its create-game menu).
    let level_name = |l: &LevelInfo| {
        let n = l.name.replace('_', " ");
        let mut c = n.chars();
        c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
    };
    let rows: Vec<(usize, String)> = m.levels.iter().enumerate().skip(m.scroll).take(LEVEL_ROWS).map(|(i, l)| (i, level_name(l))).collect();
    let modes: Vec<String> = m.modes().iter().map(|&i| t(m, MODES[i].1)).collect();
    let team = if m.team == 1 { t(m, "SINGLEPLAYER_TEAM_AXIS") } else { t(m, "SINGLEPLAYER_TEAM_ALLIES") };
    let settings = [
        (t(m, "MAPVOTE_MOD").to_uppercase(), m.mods[m.mod_index].clone(), Act::Mod(-1), Act::Mod(1)),
        (t(m, "SINGLEPLAYER_TEAM"), team, Act::Team, Act::Team),
        (t(m, "GENERAL_OPTIONS_PERCENTAGE_OF_BOTS"), m.bots.to_string(), Act::Bots(-4), Act::Bots(4)),
        // the original's slider (`CREATE_GAME_FRIENDLY_FIRE`, 0–100 %); here by tens
        (t(m, "CREATE_GAME_FRIENDLY_FIRE"), format!("{} %", m.friendly_fire), Act::FriendlyFire(-10), Act::FriendlyFire(10)),
    ];
    let (level, mode) = (m.level, m.mode);
    let title = t(m, "CREATE_GAME");
    let (levels_title, types_title) = (t(m, "CREATE_GAME_LEVELS"), t(m, "CREATE_GAME_GAME_TYPE"));

    s.spawn(Node { overflow: Overflow::clip(), ..rect(10.0, 100.0, 780.0, 440.0) }).with_children(|f| {
        // the settings
        f.spawn((rect(15.0, 25.0, 512.0, 512.0), picture(xl))).with_children(|p| {
            p.spawn(rect(17.0, 8.0, 300.0, 20.0)).with_child((Text::new(title), font(12.0), TextColor(BUTTON_TEXT)));
            for (k, (label, value, less, more)) in settings.into_iter().enumerate() {
                let y = 45.0 + 30.0 * k as f32;
                p.spawn(rect(17.0, y + 4.0, 170.0, 20.0)).with_child((Text::new(label), font(11.0), TextColor(LIST_TEXT)));
                p.spawn((rect(190.0, y - 4.0, 16.0, 32.0), picture(left[0].clone()), Button { act: less, pictures: pics(&left) }));
                p.spawn(Node { justify_content: JustifyContent::Center, ..rect(208.0, y + 4.0, 110.0, 20.0) }).with_child((Text::new(value), font(11.0), TextColor(LIST_TEXT)));
                p.spawn((rect(320.0, y - 4.0, 16.0, 32.0), picture(right[0].clone()), Button { act: more, pictures: pics(&right) }));
            }
        });
        // the level's picture
        f.spawn((rect(375.0, 25.0, 256.0, 128.0), picture(karta))).with_children(|k| {
            k.spawn((rect(5.0, 5.0, 172.0, 128.0), picture(thumb)));
        });
        // the levels
        f.spawn((rect(375.0, 136.0, 256.0, 512.0), picture(list.clone()))).with_children(|l| {
            l.spawn(rect(17.0, 8.0, 150.0, 20.0)).with_child((Text::new(levels_title), font(11.0), TextColor(BUTTON_TEXT)));
            l.spawn((rect(166.0, 15.0, 16.0, 8.0), picture(up.clone()), Button { act: Act::Scroll(-(LEVEL_ROWS as i32)), pictures: None }));
            l.spawn((rect(166.0, 247.0, 16.0, 8.0), picture(down.clone()), Button { act: Act::Scroll(LEVEL_ROWS as i32), pictures: None }));
            for (n, (i, name)) in rows.into_iter().enumerate() {
                let bg = if i == level { LIST_PICKED } else { Color::NONE };
                l.spawn((rect(6.0, 29.0 + ROW * n as f32, 158.0, ROW), BackgroundColor(bg), Button { act: Act::Level(i), pictures: None })).with_child((Text::new(name), font(10.5), TextColor(LIST_TEXT)));
            }
        });
        // the game types
        f.spawn((rect(585.0, 25.0, 256.0, 128.0), picture(types))).with_children(|g| {
            g.spawn(rect(17.0, 8.0, 150.0, 20.0)).with_child((Text::new(types_title), font(11.0), TextColor(BUTTON_TEXT)));
            for (n, name) in modes.into_iter().enumerate() {
                let bg = if n == mode { LIST_PICKED } else { Color::NONE };
                g.spawn((rect(6.0, 30.0 + ROW * n as f32, 158.0, ROW), BackgroundColor(bg), Button { act: Act::Mode(n), pictures: None })).with_child((Text::new(name), font(10.5), TextColor(LIST_TEXT)));
            }
        });
    });
}

/// Pictures follow the mouse; a click does what the button says.
fn buttons(
    mut menu: ResMut<Menu>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut windows: Query<&mut Window>,
    mut q: Query<(&ComputedNode, &UiGlobalTransform, &Button, Option<&mut ImageNode>)>,
    mut inner: Query<(&Picture, &mut ImageNode), Without<Button>>,
    mut exit: MessageWriter<AppExit>,
    mut settings: ResMut<Settings>,
    mut commands: Commands,
    mut sounds: ResMut<crate::sound::Sounds>,
) {
    use crate::sound::{MENU_HIGHLIGHT, MENU_OK, MENU_SOUNDS, MENU_START_GAME};
    menu.frames += 1;
    let mut act = None;
    if menu.auto_start.is_some_and(|f| menu.frames == f) {
        act = Some(Act::Start);
    }
    if let Some(a) = menu.pending.take() {
        act = Some(match a {
            // (the next level of the list)
            Act::Level(usize::MAX) => Act::Level((menu.level + 1) % menu.levels.len().max(1)),
            Act::PickMod(usize::MAX) => Act::PickMod((menu.mod_pick + 1) % menu.mods.len().max(1)),
            a => a,
        });
    }
    let at = windows.single().ok().and_then(|w| w.physical_cursor_position());
    let mut hovered = None;
    let asking = menu.asking;
    for (n, t, b, img) in &mut q {
        let over = at.is_some_and(|at| n.contains_point(*t, at));
        if let (Some(p), Some(mut img)) = (&b.pictures, img) {
            let k = match (over, mouse.pressed(MouseButton::Left)) {
                (false, _) => 0,
                (true, false) => 1,
                (true, true) => 2,
            };
            if img.image != p[k] {
                img.image = p[k].clone();
            }
        }
        // (with the question up, only its answers)
        if over && mouse.just_pressed(MouseButton::Left) && (asking.is_none() || matches!(b.act, Act::No | Act::Exit | Act::Disconnect)) {
            act = Some(b.act);
        }
        if over {
            hovered = Some(b.act);
        }
    }
    if hovered != menu.hovered {
        if hovered.is_some() && menu.loading.is_none() {
            sounds.play_flat(&menu.vfs, MENU_SOUNDS, MENU_HIGHLIGHT);
        }
        menu.hovered = hovered;
    }
    for (p, mut img) in &mut inner {
        let Some(pics) = &p.pictures else { continue };
        let k = match (hovered == Some(p.act), mouse.pressed(MouseButton::Left)) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => 2,
        };
        // (a pressed one stays so: the page it opened)
        if img.image != pics[k] && !(k == 0 && img.image == pics[2] && menu.page_of(p.act)) {
            img.image = pics[k].clone();
        }
    }
    // (the click that was a key's, or any while a key is waited for)
    if std::mem::take(&mut menu.swallow) || menu.define.is_some() {
        return;
    }
    let Some(act) = act else { return };
    if menu.loading.is_some() {
        return;
    }
    let m = &mut *menu;
    sounds.play_flat(&m.vfs, MENU_SOUNDS, if matches!(act, Act::Start | Act::Join) { MENU_START_GAME } else { MENU_OK });
    match act {
        Act::Go(p) | Act::Tab(p) => {
            if p == Page::CustomGame && m.page != p {
                m.mod_pick = m.mod_index;
                m.mod_scroll = m.mod_scroll.min(m.mod_pick).max((m.mod_pick + 1).saturating_sub(MOD_ROWS));
            }
            m.page = p;
        }
        Act::Exit => {
            exit.write(AppExit::Success);
        }
        Act::Mod(d) => {
            let n = m.mods.len();
            let old = m.mod_index;
            m.mod_index = (m.mod_index as i32 + d).rem_euclid(n as i32) as usize;
            if let Err(e) = m.mount() {
                warn!("{e}");
                m.mod_index = old;
                let _ = m.mount();
            }
        }
        Act::PickMod(i) => {
            m.mod_pick = i.min(m.mods.len().saturating_sub(1));
            // (the list follows the pick)
            m.mod_scroll = m.mod_scroll.min(m.mod_pick).max((m.mod_pick + 1).saturating_sub(MOD_ROWS));
        }
        Act::ModScroll(d) => m.mod_scroll = (m.mod_scroll as i32 + d).clamp(0, m.mods.len().saturating_sub(MOD_ROWS) as i32) as usize,
        Act::Activate => {
            let old = m.mod_index;
            m.mod_index = m.mod_pick;
            match m.mount() {
                Ok(()) => {
                    // remembered, as the original's restart into the mod: the next start opens it
                    settings.last_mod = m.mods[m.mod_index].clone();
                    settings.save();
                }
                Err(e) => {
                    warn!("{e}");
                    m.mod_index = old;
                    let _ = m.mount();
                }
            }
        }
        Act::Level(i) => {
            m.level = i;
            m.mode = 0;
        }
        Act::Scroll(d) => scroll(m, d),
        Act::Mode(i) => m.mode = i,
        Act::Team => m.team = 3 - m.team,
        Act::Bots(d) => m.bots = (m.bots as i32 + d).clamp(0, MAX_BOTS as i32) as u32,
        Act::FriendlyFire(d) => m.friendly_fire = (m.friendly_fire as i32 + d).clamp(0, 100) as u32,
        Act::Start => {
            if m.levels.get(m.level).is_some() {
                m.loading = Some(load_frames());
            }
        }
        Act::Join => {
            if !m.address.trim().is_empty() {
                m.error = None;
                m.loading = Some(load_frames());
            }
        }
        Act::ControlsMap(map) => {
            m.controls_map = map;
            m.controls_page = 0;
            m.conflict = None;
            m.controls_saved = false;
        }
        Act::ControlsPage(d) => controls_page_turn(m, d),
        Act::Define(row, column) => {
            m.define = Some((row, column));
            m.conflict = None;
            m.controls_saved = false;
        }
        Act::ControlsDefault => {
            let mut c = crate::controls::current();
            c.reset(m.controls_map);
            crate::controls::set(c);
            m.conflict = None;
            m.controls_saved = false;
        }
        Act::ControlsMouse(_) | Act::ControlsKeyboard(_) | Act::ControlsInvert => {
            let mut c = crate::controls::current();
            match act {
                Act::ControlsMouse(v) => c.set_mouse(m.controls_map, v as f32 / 10.0),
                Act::ControlsKeyboard(v) => c.set_keyboard(m.controls_map, v as f32 / 10.0),
                _ => c.set_invert(m.controls_map, !c.invert(m.controls_map)),
            }
            crate::controls::set(c);
            m.controls_saved = false;
        }
        Act::ControlsSave => {
            crate::controls::current().save();
            m.controls_saved = true;
        }
        Act::Ask(a) => m.asking = Some(a),
        Act::No => m.asking = None,
        Act::Disconnect => {
            m.asking = None;
            commands.insert_resource(Teardown);
            m.in_game = false;
            m.page = Page::Main;
        }
        Act::Gfx(row, d) => {
            row.step(&mut settings, d);
            settings.save();
            settings.apply();
            settings.apply_graphics();
            if let Ok(mut w) = windows.single_mut() {
                let mode = if settings.vsync { bevy::window::PresentMode::AutoVsync } else { bevy::window::PresentMode::AutoNoVsync };
                if w.present_mode != mode {
                    w.present_mode = mode;
                }
                let screen = crate::window_mode(settings.fullscreen);
                if w.mode != screen {
                    w.mode = screen;
                }
            }
        }
        Act::AddFolder => {
            if !m.folder.trim().is_empty() {
                let dir = typed_path(&m.folder);
                add_folder(m, &mut settings, &dir);
            }
        }
        Act::Hd | Act::Pack(_) => {
            // all on or all off, or one (their order: `bytefield-content`)
            let c = &mut settings.content;
            if let Act::Pack(i) = act {
                if let Some(p) = c.packs.get_mut(i) {
                    p.on = !p.on;
                }
            } else {
                let on = !c.packs.iter().any(|p| p.on);
                c.packs.iter_mut().for_each(|p| p.on = on);
            }
            settings.save();
            settings.apply_content(None, None);
            // (the menu's own pictures may be the pack's)
            if let Err(e) = m.mount() {
                warn!("{e}");
            }
        }
        Act::Volume(_) | Act::Lang(_) => {
            let s = &mut *settings;
            match act {
                Act::Volume(d) => s.volume = ((s.volume * 10.0).round() + d as f32).clamp(0.0, 10.0) / 10.0,
                Act::Lang(d) => {
                    let now = LANGUAGES.iter().position(|(_, l)| *l == crate::settings::language()).unwrap_or(3);
                    s.lang = LANGUAGES[(now as i32 + d).rem_euclid(LANGUAGES.len() as i32) as usize].1.into();
                }
                _ => {}
            }
            s.apply();
            s.save();
        }
    }
    m.dirty = true;
}

fn scroll(m: &mut Menu, d: i32) {
    let max = m.levels.len().saturating_sub(LEVEL_ROWS) as i32;
    m.scroll = (m.scroll as i32 + d).clamp(0, max) as usize;
}

fn controls_page_turn(m: &mut Menu, d: i32) {
    let pages = crate::controls::current().rows(m.controls_map).len().div_ceil(CONTROL_ROWS).max(1) as i32;
    m.controls_page = (m.controls_page as i32 + d).clamp(0, pages - 1) as usize;
}

/// The wheel scrolls the levels, and the controls.
fn wheel(mut menu: ResMut<Menu>, mut events: MessageReader<MouseWheel>) {
    let mut d = 0.0;
    for e in events.read() {
        d += match e.unit {
            MouseScrollUnit::Line => e.y * 3.0,
            MouseScrollUnit::Pixel => e.y / 10.0,
        };
    }
    if d != 0.0 && menu.page == Page::CreateGame {
        scroll(&mut menu, -d.round() as i32);
        menu.dirty = true;
    }
    if d != 0.0 && menu.page == Page::Controls && menu.define.is_none() {
        controls_page_turn(&mut menu, -(d.signum() as i32));
        menu.dirty = true;
    }
}

/// The server's address is typed on the join page; Enter joins. On the options' page, a folder; Enter adds it.
fn typing(mut menu: ResMut<Menu>, mut keys: MessageReader<bevy::input::keyboard::KeyboardInput>) {
    use bevy::input::keyboard::Key;
    let mut changed = false;
    for k in keys.read() {
        if !k.state.is_pressed() || menu.loading.is_some() || menu.asking.is_some() {
            continue;
        }
        if menu.page == Page::Options {
            match &k.logical_key {
                Key::Character(c) => menu.folder.extend(c.chars().filter(|ch| !ch.is_control())),
                Key::Space => menu.folder.push(' '),
                Key::Backspace => _ = menu.folder.pop(),
                Key::Enter => menu.pending = Some(Act::AddFolder),
                _ => continue,
            }
            let end = menu.folder.char_indices().nth(1024).map_or(menu.folder.len(), |(i, _)| i);
            menu.folder.truncate(end);
            changed = true;
            continue;
        }
        if menu.page != Page::Join {
            continue;
        }
        match &k.logical_key {
            Key::Character(c) => {
                for ch in c.chars().filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ':' | '-')) {
                    if menu.address.len() < 64 {
                        menu.address.push(ch);
                    }
                }
            }
            Key::Backspace => {
                menu.address.pop();
            }
            Key::Enter if !menu.address.is_empty() => {
                menu.error = None;
                menu.loading = Some(load_frames());
            }
            _ => continue,
        }
        changed = true;
    }
    if changed {
        menu.dirty = true;
    }
}

/// Esc in a round opens the menu over it (the round goes on: it is a server's, or the bots'), and
/// closes it again. A script's round has no menu.
fn escape(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>, mut steps: ResMut<Steps>, game: Res<crate::play::Game>, round: Option<Res<Round>>, menu: Option<Res<Menu>>, settings: Res<Settings>, roots: Query<Entity, Or<(With<Root>, With<MenuCamera>)>>, mut ui: ResMut<UiScale>, mut loaded: Option<ResMut<JustLoaded>>) {
    let step = std::mem::take(&mut steps.esc);
    let cancels = loaded.as_ref().map(|l| l.page);
    if let Some(l) = loaded.as_mut() {
        l.frames -= 1;
        if l.frames == 0 {
            commands.remove_resource::<JustLoaded>();
        }
    }
    // (Esc in the chat box drops the line, not the round)
    if !(step || keys.just_pressed(KeyCode::Escape) && !game.scripted() && game.chat_box.is_none()) {
        return;
    }
    // (as [`Act::Disconnect`], on the page it was started from)
    if let (Some(page), None, Some(r)) = (cancels, &menu, &round) {
        match Menu::new(r.game.clone(), &r.mod_name, page, None, &settings) {
            Ok(m) => {
                commands.insert_resource(m);
                commands.insert_resource(Teardown);
                commands.remove_resource::<JustLoaded>();
            }
            Err(e) => warn!("menu: {e}"),
        }
        return;
    }
    match menu {
        // (with a key waited for, Esc is `define_key`'s)
        Some(m) if m.in_game && m.loading.is_none() && m.define.is_none() => {
            for e in &roots {
                commands.entity(e).despawn();
            }
            ui.0 = 1.0;
            commands.remove_resource::<Menu>();
        }
        Some(_) => {}
        None => {
            let Some(r) = round else { return };
            match Menu::new(r.game.clone(), &r.mod_name, Page::Main, None, &settings) {
                Ok(mut m) => {
                    m.in_game = true;
                    commands.insert_resource(m);
                }
                Err(e) => warn!("menu: {e}"),
            }
        }
    }
}

/// The 800×600 screen as large as the window lets it.
fn scale(windows: Query<&Window>, mut ui: ResMut<UiScale>) {
    let Ok(w) = windows.single() else { return };
    let s = (w.width() / W).min(w.height() / H).max(0.1);
    if (ui.0 - s).abs() > 1e-3 {
        ui.0 = s;
    }
}

/// After the "loading" text has shown: what was chosen goes to `main`, and the menu goes away.
fn load(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<Menu>, mut settings: ResMut<Settings>, roots: Query<Entity, Or<(With<Root>, With<MenuCamera>)>>, mut ui: ResMut<UiScale>) {
    let Some(n) = menu.loading else { return };
    if keys.just_pressed(KeyCode::Escape) {
        menu.loading = None;
        menu.dirty = true;
        return;
    }
    if n > 0 {
        menu.loading = Some(n - 1);
        return;
    }
    let m = &*menu;
    if m.page == Page::Join {
        let a = m.address.trim().to_string();
        settings.last_server = a.clone();
        settings.team = m.team;
        commands.insert_resource(Chosen { mod_name: String::new(), level: String::new(), mode: String::new(), team: m.team, bots: 0, friendly_fire: m.friendly_fire, connect: Some(a),
        });
    } else {
        let l = &m.levels[m.level];
        let mode = m.modes().get(m.mode).map_or("conquest", |&i| MODES[i].0);
        (settings.last_mod, settings.last_level, settings.last_mode, settings.team, settings.bots, settings.friendly_fire) = (m.mods[m.mod_index].clone(), l.name.clone(), mode.into(), m.team, m.bots, m.friendly_fire);
        commands.insert_resource(Chosen { mod_name: m.mods[m.mod_index].clone(), level: l.name.clone(), mode: mode.into(), team: m.team, bots: m.bots as usize, friendly_fire: m.friendly_fire, connect: None,
        });
    }
    settings.save();
    if m.in_game {
        commands.insert_resource(Teardown);
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    ui.0 = 1.0;
    commands.remove_resource::<Menu>();
}

#[cfg(test)]
mod data_tests {
    use super::*;

    #[test]
    fn the_menu_of_bf1942() {
        bytefield_testdata::require!(Base, Xpack1, Dc);
        // the menu lists every mod under Mods/: only the available layers' (a reference mod may be on another disk)
        let game = bytefield_testdata::install_view().unwrap();
        let m = Menu::new(game, "bf1942", Page::CreateGame, None, &Settings::default()).unwrap();
        assert!(m.mods.iter().any(|x| x == "XPack1"));
        let names: Vec<&str> = m.levels.iter().map(|l| l.name.as_str()).collect();
        assert!(names.len() >= 16 && names.contains(&"El_Alamein") && names.iter().any(|n| n.eq_ignore_ascii_case("Berlin")), "{names:?}");
        assert_eq!(m.levels[m.level].name, "El_Alamein");
        assert!(m.vfs.exists("menu/Texture/Menu/knapp_n.dds"));
        assert_eq!(text(&m.lexicon, "MENU_CREATE_GAME"), "CREAR PARTIDA");
        // a mod's lexicon lacks some keys: bf1942's has them
        let dc = Menu::new(m.game.clone(), "DC_Final", Page::CreateGame, None, &Settings::default()).unwrap();
        assert_eq!(text(&dc.lexicon, "MAPVOTE_MOD"), "Mod");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_folder_and_its_end() {
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(typed_path(" ~/packs/hd "), PathBuf::from(&home).join("packs/hd"));
            assert_eq!(typed_path("~"), PathBuf::from(&home));
        }
        assert_eq!(typed_path("~other/x"), PathBuf::from("~other/x"));
        assert_eq!(tail("/a/b", 16), "/a/b");
        assert_eq!(tail("/srv/games/packs/hd", 9), "…packs/hd");
    }
}
