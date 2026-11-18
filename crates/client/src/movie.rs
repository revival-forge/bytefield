//! The Bink movies and music: the start's movies (`Game/Startup.con`:
//! `renderer.playBink <file> <seconds>`, EA, Dice, Intro and Legal, each skippable once that long
//! has gone) and the menu's music (`Game.setMenuMusicFilename` of the mod's `init.con`).
//!
//! Bink is decoded by `ffmpeg` (it has Bink video and audio decoders), when it is installed:
//! without it there are no movies nor music. The sound is converted once to a wav under
//! `data/out/cache/bink/` and played by the mixer; the picture is read from ffmpeg's output as
//! it plays (RGBA frames into a texture).

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::sound::{Pcm, Sounds};

/// Where the converted sound is kept.
fn cache() -> std::path::PathBuf {
    bytefield_content::first_run::out_dir("cache/bink")
}

/// `ffmpeg` and `ffprobe` answer.
fn have_ffmpeg() -> bool {
    static OK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OK.get_or_init(|| ["ffmpeg", "ffprobe"].iter().all(|c| Command::new(c).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())))
}

/// `rel` (as the game's files name it, any case) in the mod's folder, then bf1942's (the mod
/// paths), then the game's (the start's movies are there).
// (The menu's background is not looked up here: only in the mod's own folder, `background`.)
pub fn resolve(game: &Path, mod_name: &str, rel: &str) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![game.join("Mods").join(mod_name)];
    if !mod_name.eq_ignore_ascii_case("bf1942") {
        roots.push(game.join("Mods").join("bf1942"));
    }
    roots.push(game.to_path_buf());
    roots.iter().find_map(|r| find_nocase(r, rel))
}

/// A `Game.set*MusicFilename` (`music/x.bik`): only in the mod's own folder. The original does not fall back to bf1942
/// (measured with `+file`: DC_Final without `Music/` tries `mods/DC_Final/music/slaughter4.{BIK,DDS,RCM,TGA}`
/// and nothing else; it opens every music file of `init.con` this way at the start). Level 1.
pub fn music_file(game: &Path, mod_name: &str, rel: &str) -> Option<PathBuf> {
    find_nocase(&game.join("Mods").join(mod_name), rel)
}

/// The menu's background: only the mod's own `Movies/background.bik`. The original does not fall back to bf1942 or to another
/// mod: without it, plain grey.
pub fn background(game: &Path, mod_name: &str) -> Option<PathBuf> {
    find_nocase(&game.join("Mods").join(mod_name), "Movies/background.bik")
}

fn find_nocase(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut at = root.to_path_buf();
    for part in rel.split(['/', '\\']).filter(|p| !p.is_empty()) {
        let next = std::fs::read_dir(&at).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?;
        at = next.path();
    }
    at.is_file().then_some(at)
}

/// The movie's sound, converted once (None: it has none, or no ffmpeg).
pub fn sound(path: &Path) -> Option<Arc<Pcm>> {
    if !have_ffmpeg() {
        return None;
    }
    let name: String = path.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let wav = cache().join(format!("{name}.wav"));
    if !wav.is_file() {
        std::fs::create_dir_all(cache()).ok()?;
        let tmp = wav.with_extension("part");
        let ok = Command::new("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(path).args(["-vn", "-ac", "2", "-f", "wav"]).arg(&tmp).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if !ok {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        std::fs::rename(&tmp, &wav).ok()?;
    }
    crate::sound::parse_wav(&std::fs::read(&wav).ok()?).filter(|p| !p.data.is_empty()).map(Arc::new)
}

/// A movie's picture as ffmpeg decodes it: its size and the frame of now, which a thread reads
/// at the movie's rate by the clock (so a slow window drops frames, it does not slow the movie).
struct Picture {
    child: Child,
    /// the latest frame not yet shown, and whether the frames are over
    latest: Arc<Mutex<(Option<Vec<u8>>, bool)>>,
    size: (u32, u32),
}

impl Picture {
    /// `looping`: over and over (ffmpeg's `-stream_loop -1`), never ending.
    fn open(path: &Path, looping: bool) -> Option<Picture> {
        let (w, h, fps) = probe(path)?;
        let looped: &[&str] = if looping { &["-stream_loop", "-1"] } else { &[] };
        let mut child = Command::new("ffmpeg").args(["-v", "error"]).args(looped).arg("-i").arg(path).args(["-an", "-f", "rawvideo", "-pix_fmt", "rgba", "-"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let mut out = child.stdout.take()?;
        let latest = Arc::new(Mutex::new((None, false)));
        let slot = latest.clone();
        let n = (w * h * 4) as usize;
        std::thread::spawn(move || {
            let start = Instant::now();
            for i in 0u32.. {
                let mut f = vec![0u8; n];
                if out.read_exact(&mut f).is_err() {
                    break;
                }
                if let Some(wait) = Duration::from_secs_f64(i as f64 / fps).checked_sub(start.elapsed()) {
                    std::thread::sleep(wait);
                }
                slot.lock().unwrap().0 = Some(f);
            }
            slot.lock().unwrap().1 = true;
        });
        Some(Picture { child, latest, size: (w, h) })
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The movie on screen.
struct Showing {
    picture: Option<Picture>,
    image: Handle<Image>,
    root: Entity,
    voice: Option<u64>,
    /// its length (the sound's when the picture has none: the end of its frames otherwise)
    length: Option<f32>,
    skip_after: f32,
    started: f32,
    shown: u64,
    ended: bool,
}

/// The movies still to play, in order, and the one playing.
#[derive(Resource, Default)]
pub struct Movies {
    queue: VecDeque<(PathBuf, f32)>,
    now: Option<Showing>,
}

impl Movies {
    /// The start's movies of `Game/Startup.con` (the vfs's), the files where the game has them.
    pub fn start(vfs: &bytefield_vfs::Vfs, game: &Path, mod_name: &str) -> Movies {
        let text = ["bf1942/game/startup.con", "Bf1942/Game/Startup.con"].iter().find_map(|p| vfs.read(p).ok()).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let queue = text
            .lines()
            .filter_map(|l| {
                let mut w = l.split_whitespace();
                if !w.next()?.eq_ignore_ascii_case("renderer.playBink") {
                    return None;
                }
                let file = w.next()?.trim_matches('"');
                let skip = w.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                Some((resolve(game, mod_name, file)?, skip))
            })
            .collect();
        Movies { queue, now: None }
    }

    /// Without the first `n`.
    pub fn skip(&mut self, n: usize) {
        self.queue.drain(..n.min(self.queue.len()));
    }
}

/// The menu's music, looping while the menu is up and no movie plays.
/// TODO: the original's profiles also bring `game.setMenuMusicVolume 50`, which the loading music does not follow (recorded at master
/// 100 it sounds as its file); whether the menu's does is not measured: record the original's menu
/// (`MENU_MUSIC=100` against 50) and compare with its file.
#[derive(Resource, Default)]
pub struct MenuMusic {
    /// the mod's `setMenuMusicFilename`, found
    pub file: Option<PathBuf>,
    /// whose it is (another mod activated in the menu brings its own)
    mod_name: String,
    pcm: Option<Option<Arc<Pcm>>>,
    voice: Option<u64>,
}

impl MenuMusic {
    pub fn for_mod(game: &Path, mod_name: &str) -> MenuMusic {
        let file = std::fs::read_to_string(game.join("Mods").join(mod_name).join("init.con")).ok().and_then(|t| {
            t.lines().find_map(|l| {
                let mut w = l.split_whitespace();
                w.next()?.eq_ignore_ascii_case("Game.setMenuMusicFilename").then(|| w.next().map(|f| f.trim_matches('"').to_string()))?
            })
        });
        MenuMusic { file: file.and_then(|f| music_file(game, mod_name, &f)), mod_name: mod_name.to_string(), ..default() }
    }
}

pub struct MoviePlugin;

impl Plugin for MoviePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuMusic>().init_resource::<LoadMusic>().init_resource::<Backdrop>().add_systems(Update, ((play.run_if(resource_exists::<Movies>), music, backdrop).chain(), play_textures, load_music));
    }
}

/// The game and mod of the round, for the textures that name a movie (set before it loads).
static GAME: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);

pub fn set_game(game: &Path, mod_name: &str) {
    *GAME.lock().unwrap() = Some((game.to_path_buf(), mod_name.to_string()));
}

/// A movie on a mesh: all its frames, decoded once, looping at its rate.
struct Looping {
    path: PathBuf,
    image: Handle<Image>,
    frames: Vec<Vec<u8>>,
    fps: f64,
    shown: usize,
}

static TEXTURES: Mutex<Vec<Looping>> = Mutex::new(Vec::new());

/// The longest side and the bytes a movie texture keeps (the rest of its frames are dropped: a
/// shorter loop). GC's are 128–256 px and 6–84 frames but for two 720×480 holograms.
const TEXTURE_SIDE: u32 = 256;
const TEXTURE_BYTES: usize = 32 << 20;

/// A texture that names a Bink movie, from the game's folder (GC: `texture "Mods\GCMOD\Movies\PodRacerCockpit"`,
/// the pod's screen; R2's projector, the TIE's HUD): its frames on a loop at the movie's rate, without
/// sound.
pub fn texture(name: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let (game, mod_name) = GAME.lock().unwrap().clone()?;
    let rel = if name.to_ascii_lowercase().ends_with(".bik") { name.to_string() } else { format!("{name}.bik") };
    let path = resolve(&game, &mod_name, &rel)?;
    let mut all = TEXTURES.lock().unwrap();
    if let Some(l) = all.iter().find(|l| l.path == path) {
        return Some(l.image.clone());
    }
    let (w, h, fps) = probe(&path)?;
    let k = (TEXTURE_SIDE as f64 / w.max(h) as f64).min(1.0);
    let (w, h) = (((w as f64 * k) as u32).max(1), ((h as f64 * k) as u32).max(1));
    let out = Command::new("ffmpeg").args(["-v", "error", "-i"]).arg(&path).args(["-an", "-vf", &format!("scale={w}:{h}"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"]).stderr(Stdio::null()).output().ok()?;
    let n = (w * h * 4) as usize;
    let frames: Vec<Vec<u8>> = out.stdout.chunks_exact(n).take((TEXTURE_BYTES / n).max(1)).map(|f| f.to_vec()).collect();
    let mut image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, frames.first()?.clone(), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    image.sampler = crate::assets::repeat_sampler();
    let image = images.add(image);
    all.push(Looping { path, image: image.clone(), frames, fps, shown: 0 });
    Some(image)
}

fn play_textures(mut images: ResMut<Assets<Image>>, time: Res<Time>) {
    let t = time.elapsed_secs_f64();
    for l in TEXTURES.lock().unwrap().iter_mut() {
        let i = (t * l.fps) as usize % l.frames.len();
        if i != l.shown
            && let Some(mut img) = images.get_mut(&l.image)
        {
            img.data = Some(l.frames[i].clone());
            l.shown = i;
        }
    }
}

/// A movie's picture: its width, height and frames a second.
fn probe(path: &Path) -> Option<(u32, u32, f64)> {
    if !have_ffmpeg() {
        return None;
    }
    let probe = Command::new("ffprobe").args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,r_frame_rate", "-of", "csv=p=0"]).arg(path).output().ok()?;
    let line = String::from_utf8_lossy(&probe.stdout).trim().to_string();
    let mut it = line.split(',');
    let (w, h): (u32, u32) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    let fps = it.next().and_then(|r| r.split_once('/')).and_then(|(a, b)| Some(a.parse::<f64>().ok()? / b.parse::<f64>().ok()?)).filter(|f| *f > 0.0)?;
    Some((w, h, fps))
}

fn play(mut commands: Commands, mut movies: ResMut<Movies>, mut images: ResMut<Assets<Image>>, sounds: Option<Res<Sounds>>, time: Res<Time<Real>>, keys: Res<ButtonInput<KeyCode>>, mouse: Res<ButtonInput<MouseButton>>) {
    let now = time.elapsed_secs();
    let skip = keys.any_just_pressed([KeyCode::Escape, KeyCode::Space, KeyCode::Enter]) || mouse.just_pressed(MouseButton::Left);
    if let Some(s) = movies.now.as_mut() {
        let t = now - s.started;
        if let Some(p) = s.picture.as_ref() {
            let (frame, over) = {
                let mut l = p.latest.lock().unwrap();
                (l.0.take(), l.1)
            };
            s.ended = over;
            if let (Some(f), Some(mut img)) = (frame, images.get_mut(&s.image)) {
                img.data = Some(f);
                s.shown += 1;
                if s.shown == 1 {
                    debug!("movie: first frame at {t:.2} s");
                }
            }
        }
        let over = s.ended || s.length.is_some_and(|l| t >= l) || (s.picture.is_none() && s.length.is_none());
        if over || (skip && t >= s.skip_after) {
            let s = movies.now.take().unwrap();
            debug!("movie over at {t:.1} s, {} frames", s.shown);
            if let (Some(v), Some(snd)) = (s.voice, sounds.as_ref()) {
                snd.mixer.lock().unwrap().stop(v, true);
            }
            commands.entity(s.root).despawn();
        } else {
            return;
        }
    }
    let Some((path, skip_after)) = movies.queue.pop_front() else {
        commands.remove_resource::<Movies>();
        return;
    };
    let picture = Picture::open(&path, false);
    let pcm = sound(&path);
    let length = pcm.as_ref().filter(|_| picture.is_none()).map(|p| p.data.len() as f32 / p.channels.max(1) as f32 / p.rate as f32);
    let voice = pcm.zip(sounds.as_ref()).map(|(p, snd)| snd.mixer.lock().unwrap().play_music(p, false));
    let (w, h) = picture.as_ref().map_or((4, 4), |p| p.size);
    let image = images.add(Image::new_fill(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 255], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default()));
    // the whole window black, the picture in it at its shape, over the menu
    let root = commands
        .spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(Color::BLACK), GlobalZIndex(1000)))
        .with_child((Node { height: Val::Vh(100.0), width: Val::Vh(100.0 * w as f32 / h as f32), max_width: Val::Vw(100.0), ..default() }, ImageNode::new(image.clone())))
        .id();
    info!("movie: {} ({}x{}, skippable after {skip_after} s)", path.display(), w, h);
    movies.now = Some(Showing { picture, image, root, voice, length, skip_after, started: now, shown: 0, ended: false });
}

/// The mod's music for a level loading (`Game.setLoadMusicFilename`), as [`MenuMusic`]'s. It goes on through the briefing until READY: level 3, nobody has heard the original.
#[derive(Resource, Default)]
pub struct LoadMusic {
    /// the mod it was looked for, and its file
    file: Option<(String, Option<PathBuf>)>,
    pcm: Option<Option<Arc<Pcm>>>,
    voice: Option<u64>,
}

fn load_music(mut m: ResMut<LoadMusic>, menu: Option<Res<crate::menu::Menu>>, chosen: Option<Res<crate::menu::Chosen>>, round: Option<Res<crate::menu::Round>>, game: Option<Res<crate::play::Game>>, briefing: Res<crate::briefing::Briefing>, sounds: Option<Res<Sounds>>) {
    let Some(snd) = sounds else { return };
    let wanted = menu.as_ref().is_some_and(|m| m.is_loading()) || chosen.is_some() || (game.is_some() && briefing.up);
    let playing = m.voice.is_some_and(|v| snd.mixer.lock().unwrap().playing(v));
    if wanted && !playing {
        // (the mod of the menu, of what was chosen or of the round; a join does not know it until it is on the server)
        let mod_name = chosen.map(|c| c.mod_name.clone()).filter(|n| !n.is_empty()).or_else(|| menu.as_ref().map(|m| m.mod_name().to_string())).or_else(|| round.as_ref().map(|r| r.mod_name.clone())).unwrap_or_default();
        let game_dir = menu.as_ref().map(|m| m.game().to_path_buf()).or_else(|| round.map(|r| r.game.clone()));
        if m.file.as_ref().is_none_or(|(n, _)| *n != mod_name) {
            let file = game_dir.and_then(|g| {
                let text = std::fs::read_to_string(g.join("Mods").join(&mod_name).join("init.con")).ok()?;
                let f = text.lines().find_map(|l| {
                    let mut w = l.split_whitespace();
                    w.next()?.eq_ignore_ascii_case("Game.setLoadMusicFilename").then(|| w.next().map(|f| f.trim_matches('"').to_string()))?
                })?;
                music_file(&g, &mod_name, &f)
            });
            (m.file, m.pcm) = (Some((mod_name, file)), None);
        }
        if m.pcm.is_none() {
            let pcm = m.file.as_ref().and_then(|(_, f)| f.as_deref()).and_then(sound);
            m.pcm = Some(pcm);
        }
        if let Some(p) = m.pcm.clone().flatten() {
            m.voice = Some(snd.mixer.lock().unwrap().play_music(p, true));
        }
    } else if !wanted && let Some(v) = m.voice.take() {
        snd.mixer.lock().unwrap().stop(v, false);
    }
}

/// The menu's music: on while the menu is up (not over a round, not loading) and no movie plays.
fn music(mut m: ResMut<MenuMusic>, menu: Option<Res<crate::menu::Menu>>, movies: Option<Res<Movies>>, sounds: Option<Res<Sounds>>) {
    let Some(snd) = sounds else { return };
    if let Some(menu) = menu.as_ref().filter(|menu| !m.mod_name.is_empty() && !menu.mod_name().eq_ignore_ascii_case(&m.mod_name)) {
        // the menu is another mod's now: its music instead
        if let Some(v) = m.voice.take() {
            snd.mixer.lock().unwrap().stop(v, false);
        }
        *m = MenuMusic::for_mod(menu.game(), menu.mod_name());
    }
    let wanted = menu.is_some_and(|m| !m.in_game && !m.is_loading()) && movies.is_none();
    let playing = m.voice.is_some_and(|v| snd.mixer.lock().unwrap().playing(v));
    if wanted && !playing {
        if m.pcm.is_none() {
            let pcm = m.file.as_deref().and_then(sound);
            m.pcm = Some(pcm);
        }
        if let Some(p) = m.pcm.clone().flatten() {
            m.voice = Some(snd.mixer.lock().unwrap().play_music(p, true));
        }
    } else if !wanted && let Some(v) = m.voice.take() {
        snd.mixer.lock().unwrap().stop(v, false);
    }
}

/// The menu's animated background (`Menu/Background`'s `BfBinkNode`, `Movies/background`: the static
/// `Menu/Background.tga` set going, 320×180 at 25 frames a second, 75,7 s), looping while the menu is up and no movie
/// plays; without ffmpeg or the file the menu keeps the picture. Level 2: when the original shows it (`PlayBink`, set
/// by the original) is not read; we show it whenever the menu is.
#[derive(Resource, Default)]
pub struct Backdrop {
    picture: Option<Picture>,
    image: Option<Handle<Image>>,
    /// the file was looked for and is not there (or does not open)
    missing: bool,
    /// the mod has no `Movies/background.bik`: the original shows plain grey, not the still
    absent: bool,
    /// whose movie it is (another mod activated in the menu brings its own)
    mod_name: String,
}

impl Backdrop {
    /// The picture to show under the menu, while it plays.
    pub fn image(&self) -> Option<Handle<Image>> {
        self.picture.as_ref().and(self.image.clone())
    }

    /// The mod has no background movie: the menu goes plain grey (`MENU_NO_BACKGROUND`), as the original's.
    pub fn absent(&self) -> bool {
        self.absent
    }
}

fn backdrop(mut b: ResMut<Backdrop>, mut menu: Option<ResMut<crate::menu::Menu>>, movies: Option<Res<Movies>>, mut images: ResMut<Assets<Image>>) {
    let wanted = menu.is_some() && movies.is_none();
    if !wanted {
        b.picture = None;
        return;
    }
    if menu.as_deref().is_some_and(|m| !m.mod_name().eq_ignore_ascii_case(&b.mod_name)) {
        // (dropping the picture stops its decoding)
        let m = menu.as_deref_mut().unwrap();
        *b = Backdrop { mod_name: m.mod_name().to_string(), ..default() };
        m.redraw();
    }
    if b.picture.is_none() && !b.missing {
        let m = menu.as_deref_mut().unwrap();
        let file = background(m.game(), m.mod_name());
        b.absent = file.is_none();
        let picture = file.and_then(|p| Picture::open(&p, true));
        let Some(p) = picture else {
            b.missing = true;
            if b.absent {
                m.redraw();
            }
            return;
        };
        let (w, h) = p.size;
        b.image = Some(images.add(Image::new_fill(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 255], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())));
        b.picture = Some(p);
        m.redraw();
    }
    let frame = b.picture.as_ref().and_then(|p| p.latest.lock().unwrap().0.take());
    if let (Some(f), Some(h)) = (frame, b.image.clone())
        && let Some(mut img) = images.get_mut(&h)
    {
        img.data = Some(f);
    }
}

#[cfg(test)]
mod tests {
    /// GC_Podrace's pod has a movie for a screen (`texture "Mods\GCMOD\Movies\PodRacerCockpit"`): a
    /// texture of its frames, the same one each time it is named.
    #[test]
    fn a_movie_is_a_texture() {
        let game = bytefield_testdata::game();
        if !game.join("Mods/GCMOD/Movies").is_dir() || !super::have_ffmpeg() {
            return;
        }
        super::set_game(&game, "GCMOD");
        let mut images = bevy::asset::Assets::<bevy::image::Image>::default();
        let h = super::texture("Mods\\GCMOD\\Movies\\PodRacerCockpit", &mut images).unwrap();
        assert_eq!(images.get(&h).unwrap().size(), bevy::math::UVec2::new(256, 256));
        assert_eq!(super::texture("mods/gcmod/movies/podracercockpit.bik", &mut images), Some(h));
        assert!(super::TEXTURES.lock().unwrap().iter().any(|l| l.frames.len() == 13));
        assert!(super::texture("Mods\\GCMOD\\Movies\\Lightsaber", &mut images).is_none());
    }
}
