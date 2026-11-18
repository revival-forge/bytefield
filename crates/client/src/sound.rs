//! Sound: the objects' sound scripts (`.ssc`, `bytefield_formats::ssc`) played on the
//! simulation's events. A `Mixer` sums the voices (a wav at a speed, with a gain per ear); the
//! game thread works out each voice's gain and pitch every frame from its sample's effects (the
//! distance to the listener, the time since it started or was released) and the 3D falloff, and
//! `cpal` takes the mix (feature `audio`; without it, or without a device, all stays silent).
//!
//! What sounds: shots, reloads and effects; the vehicles' engines, tracks and turrets
//! while they have a driver (`Default` is the engine's rpm or the turret's speed, `Speed` the
//! vehicle's); the soldiers' steps by the ground's material and their voices when hit or killed
//! (`SoldierSound`); the level's ambience (`EnvironmentSound.load`) and its `AreaObject`s (a lake
//! along a line). At most `VOICES` are heard at once, by priority (`setHardwareVoiceLimit 32`).
//!
//! Inferred from the data, not yet read from BF1942.exe: what a firearm's patches are for (their
//! comments: 0 fire, 1 reload, 2 release, 3 shell bounce, 4 MG distance, 5 fire loop), a ramp's four
//! params (`x0 x1 y0 dy`), the falloff `minDistance / d` (`Sound.setRolloffFactor 1`),
//! `randomStartPitch a / b` as 1 ± a, which step patch is which material (the scripts' comments)
//! and how the steps are timed (`setWalkFrequency` as the seconds between two).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bytefield_formats::ssc::{Sample, SoundScript};

use crate::play::Game;

/// A decoded wav, mono or interleaved stereo, as f32.
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    pub data: Vec<f32>,
}

impl Pcm {
    fn frames(&self) -> usize {
        self.data.len() / self.channels.max(1) as usize
    }
}

/// RIFF/WAVE, 8 or 16-bit PCM.
pub fn parse_wav(b: &[u8]) -> Option<Pcm> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut at, mut fmt, mut data) = (12usize, None, None);
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let len = u32::from_le_bytes(b[at + 4..at + 8].try_into().ok()?) as usize;
        let body = &b[at + 8..(at + 8 + len).min(b.len())];
        match id {
            b"fmt " if body.len() >= 16 => {
                let u16_at = |i: usize| u16::from_le_bytes([body[i], body[i + 1]]);
                fmt = Some((u16_at(0), u16_at(2), u32::from_le_bytes(body[4..8].try_into().ok()?), u16_at(14)));
            }
            b"data" => data = Some(body),
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    let (format, channels, rate, bits) = fmt?;
    let data = data?;
    if format != 1 || channels == 0 {
        return None;
    }
    let data = match bits {
        16 => data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
        8 => data.iter().map(|&v| (v as f32 - 128.0) / 128.0).collect(),
        _ => return None,
    };
    Some(Pcm { rate, channels, data })
}

/// One playing wav.
struct Voice {
    pcm: Arc<Pcm>,
    /// in frames of the wav
    pos: f64,
    /// the wav's rate × pitch
    pitch: f32,
    gain: [f32; 2],
    looped: bool,
    /// a loop asked to stop at the end of its round (`stop FinishSample`)
    finishing: bool,
    done: bool,
    /// a Bink movie's or the music's sound, which the master volume scales on its own curve
    music: bool,
}

/// The master volume (the options', the original's `game.setMasterVolume` / 100; 0.75 in its profiles), as f32 bits.
static VOLUME: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f40_0000);

/// The gains the master volume `v` gives the effects and the music, as the original's (level 3: recorded with
/// at `setMasterVolume` 100, 75 and 50 on GC_Hoth; the effects (wind, steps, shots) came out at 0, −4.4…−5.1
/// and −11.3…−12.1 dB, as v²; the loading music (Bink) at +0.2, −3.9 and −9.6 dB against its file, as v^1.6).
pub fn master_gains(v: f32) -> (f32, f32) {
    let v = v.clamp(0.0, 1.0);
    (v * v, v.powf(1.6))
}

pub fn set_volume(v: f32) {
    VOLUME.store(v.clamp(0.0, 1.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Sums the voices; shared between the game thread and the device's.
#[derive(Default)]
pub struct Mixer {
    voices: HashMap<u64, Voice>,
    next: u64,
}

impl Mixer {
    pub fn play(&mut self, pcm: Arc<Pcm>, looped: bool, gain: [f32; 2], pitch: f32) -> u64 {
        self.next += 1;
        self.voices.insert(self.next, Voice { pcm, pos: 0.0, pitch, gain, looped, finishing: false, done: false, music: false });
        self.next
    }

    /// A movie's sound or the music: flat, on the master volume's music curve ([`master_gains`]).
    pub fn play_music(&mut self, pcm: Arc<Pcm>, looped: bool) -> u64 {
        let id = self.play(pcm, looped, [1.0; 2], 1.0);
        if let Some(v) = self.voices.get_mut(&id) {
            v.music = true;
        }
        id
    }

    pub fn set(&mut self, id: u64, gain: [f32; 2], pitch: f32) {
        if let Some(v) = self.voices.get_mut(&id) {
            v.gain = gain;
            v.pitch = pitch;
        }
    }

    /// `immediate`, or at the end of the loop's round.
    pub fn stop(&mut self, id: u64, immediate: bool) {
        if let Some(v) = self.voices.get_mut(&id) {
            if immediate {
                v.done = true;
            } else {
                v.finishing = true;
            }
        }
    }

    pub fn clear(&mut self) {
        self.voices.clear();
    }

    pub fn playing(&self, id: u64) -> bool {
        self.voices.get(&id).is_some_and(|v| !v.done)
    }

    /// Adds every voice into `out` (interleaved stereo at `rate`), dropping the finished.
    pub fn mix(&mut self, out: &mut [f32], rate: u32) {
        out.fill(0.0);
        let (effects, music) = master_gains(f32::from_bits(VOLUME.load(std::sync::atomic::Ordering::Relaxed)));
        for v in self.voices.values_mut() {
            let master = if v.music { music } else { effects };
            let n = v.pcm.frames();
            if n == 0 {
                v.done = true;
            }
            let ch = v.pcm.channels as usize;
            let step = v.pcm.rate as f64 * v.pitch.max(0.01) as f64 / rate as f64;
            if v.gain == [0.0, 0.0] {
                // not heard (far, or past the voice limit): it only moves on
                v.pos += step * (out.len() / 2) as f64;
                if v.pos >= n as f64 {
                    if v.looped && !v.finishing {
                        v.pos %= n.max(1) as f64;
                    } else {
                        v.done = true;
                    }
                }
                continue;
            }
            for frame in out.chunks_exact_mut(2) {
                if v.done {
                    break;
                }
                let i = v.pos as usize;
                let f = (v.pos - i as f64) as f32;
                let j = if i + 1 < n { i + 1 } else if v.looped && !v.finishing { 0 } else { i };
                for (k, o) in frame.iter_mut().enumerate() {
                    let c = if ch == 1 { 0 } else { k.min(ch - 1) };
                    let a = v.pcm.data[i * ch + c];
                    let b = v.pcm.data[j * ch + c];
                    *o += (a + (b - a) * f) * v.gain[k] * master;
                }
                v.pos += step;
                if v.pos >= n as f64 {
                    if v.looped && !v.finishing {
                        v.pos -= n as f64;
                    } else {
                        v.done = true;
                    }
                }
            }
        }
        self.voices.retain(|_, v| !v.done);
        for o in out.iter_mut() {
            *o = o.clamp(-1.0, 1.0);
        }
    }
}

/// Where a sound is heard from: the listener's position and its right.
#[derive(Clone, Copy)]
pub struct Ear {
    pub pos: Vec3,
    pub right: Vec3,
}

/// What drives a sample's effects now.
#[derive(Clone, Copy, Default)]
pub struct Controls {
    pub distance: f32,
    /// since it was triggered
    pub time: f32,
    /// since it was released (0 until then)
    pub release: Option<f32>,
    /// `Default`: an engine's |rpm|, a turret's speed (degrees a second); None for the rest
    pub default: Option<f32>,
    /// `Speed`: the vehicle's, m/s
    pub speed: Option<f32>,
}

/// The effects of one destination (`Volume`, `Pitch`) multiplied; the sources we do not drive
/// (`Extern`, `Altitude`, `Acceleration`, and `Default`/`Speed` on what has none) leave it alone,
/// but a `Linear` on `Default` is read at 0 (`0.9 0`: a fixed pitch).
fn effect(s: &Sample, dest: &str, c: &Controls, with_time: bool) -> f32 {
    let mut v = 1.0;
    for e in s.effects.iter().filter(|e| e.destination.eq_ignore_ascii_case(dest)) {
        let x = match e.source.to_ascii_lowercase().as_str() {
            "distance" => c.distance,
            "time" if with_time => c.time,
            "timerelease" if with_time => match c.release {
                Some(r) => r,
                None => continue,
            },
            "default" => match c.default {
                Some(x) => x,
                None if e.envelope.eq_ignore_ascii_case("linear") => 0.0,
                None => continue,
            },
            "speed" => match c.speed {
                Some(x) => x,
                None => continue,
            },
            _ => continue,
        };
        v *= e.value(x);
    }
    v
}

/// A sample's gain per ear: its volume and effects, the falloff past `minDistance` and, for a
/// positional one, the side it is on.
pub fn gains(s: &Sample, at: Vec3, ear: &Ear, c: &Controls, with_time: bool) -> [f32; 2] {
    let g = (s.volume * effect(s, "Volume", c, with_time)).max(0.0);
    if s.stereo {
        return [g, g];
    }
    let d = c.distance.max(s.min_distance).max(0.01);
    let g = g * s.min_distance.max(0.01) / d;
    let dir = (at - ear.pos).normalize_or_zero();
    let pan = dir.dot(ear.right).clamp(-1.0, 1.0);
    [g * ((1.0 - pan) * 0.5).sqrt(), g * ((1.0 + pan) * 0.5).sqrt()]
}

/// Where a playing sound is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    Point(Vec3),
    /// the simulation's vehicle (soldiers are vehicles too)
    Vehicle(usize),
    /// with the listener (the level's ambience)
    Ear,
    /// the nearest point of an `AreaObject`'s line ([`Sounds::areas`])
    Area(usize),
}

/// What drives a sample's `Default` and `Speed`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Drive {
    #[default]
    None,
    /// a vehicle's engine part: its |rpm|
    Engine(usize, usize),
    /// a vehicle's `RotationalBundle` part: its fastest axis
    Bundle(usize, usize),
    /// only the vehicle's speed (the tracks)
    Speed(usize),
    /// a value set when it started (a sonar's ping: `max(d/r, 0.1)`)
    Fixed(f32),
}

/// How many voices are heard at once (`Default.con`: `setHardwareVoiceLimit 32`); the rest keep
/// playing unheard, the lowest `priority` first, then the quietest.
pub const VOICES: usize = 32;

/// A sound script: its patches, each its samples.
type Patches = Arc<Vec<Vec<Arc<Sample>>>>;

/// The step patches of `SoldierWalk.ssc` and its kin, by their comments; `addMaterialToSound`
/// names one of these for each material.
const STEP_PATCHES: [&str; 11] = ["sand", "metal", "wood", "concrete", "grass", "gravel", "frozen", "mud", "fabric", "harness", "water"];
const HARNESS: usize = 9;

/// The `c_Sst*` sound triggers repeated while the state lasts (walking, running, crouching or
/// crawling along, swimming, climbing) and their `SoldierSound.set*Frequency` names.
const PERIODIC: [(i32, &str); 6] = [(2, "walk"), (3, "run"), (8, "crouch"), (11, "crawl"), (13, "swim"), (23, "ladder")];
/// The soldiers' scripts are their `loadSoundScript`s in order: trigger `c_Sst*` n plays the
/// (n − 1)th (`c_SstStand` 1 is `SoldierStop.ssc`); these are the hurt and the killed.
const HIT_DAMAGE: usize = 18;
const KILLED: usize = 20;
/// and the voice of the radio said out loud (`SoldierVoice.ssc`)
const VOICE: usize = 23;
/// The team's radio: one patch per message, heard by the whole team as it is
const MENU_RADIO: &str = "menu/MenuRadioSound.ssc";

/// A vehicle's engine, tracks and turrets while someone drives it.
struct Hum {
    voices: Vec<u64>,
    released: Option<f32>,
}

/// A soldier's steps: its sound trigger now and when the next step falls.
#[derive(Default)]
struct Steps {
    sound: i32,
    next: f32,
    hurt: f32,
}

struct Playing {
    voice: u64,
    sample: Arc<Sample>,
    anchor: Anchor,
    started: f32,
    released: Option<f32>,
    pitch: f32,
    /// the effect whose bundle it is
    effect: Option<u64>,
    drive: Drive,
}

/// The loaded scripts and wavs, and what plays.
#[derive(Resource, Default)]
pub struct Sounds {
    pub mixer: Arc<Mutex<Mixer>>,
    scripts: HashMap<String, Option<Patches>>,
    wavs: HashMap<String, Option<Arc<Pcm>>>,
    playing: Vec<Playing>,
    /// automatic weapons firing: (vehicle, weapon template) -> the voices of their loop, and the
    /// time of the last shot
    loops: HashMap<(usize, String), (Vec<usize>, f32)>,
    rng: u64,
    /// `BYTEFIELD_SOUND_DEBUG`: each sample that starts
    debug: bool,
    #[allow(dead_code)]
    device: Option<Device>,
    /// the soldiers reloading at the last frame
    reloading: std::collections::HashSet<usize>,
    /// the entries seen of `sim.fired`
    seen: usize,
    /// and of `sim.damage_log`, `sim.radio_log`
    seen_damage: usize,
    seen_radio: usize,
    /// the vehicles humming
    hums: HashMap<usize, Hum>,
    /// a vehicle template's parts with a script: (template, kind)
    parts: HashMap<String, Arc<Vec<(String, String)>>>,
    steps: HashMap<usize, Steps>,
    /// `SoldierSound.set*Frequency`, `setRandom*Frequency` (lowercase, without `set`)
    soldier: HashMap<String, f32>,
    /// `SoldierSound.addMaterialToSound`: terrain material -> step patch
    material_patch: HashMap<u32, usize>,
    /// the level's `AreaObject`s
    areas: Vec<Area>,
    /// the level's ambience and areas are playing
    level_started: bool,
    /// the level's name, whose own `sound/` folder [`Sounds::wav`] looks in after the mod's
    level: String,
    /// the ear at the last frame and its velocity (Doppler)
    last_ear: Option<Vec3>,
    ear_vel: Vec3,
    /// the scanning sonars (by vehicle): their timers and their voices
    pings: HashMap<usize, (bytefield_sim::sonar::Ping, Vec<u64>)>,
}

impl Sounds {
    /// Silence, and forget the round (a new one may be another level): the device stays.
    pub fn reset(&mut self) {
        if let Ok(mut m) = self.mixer.lock() {
            m.clear();
        }
        *self = Sounds { mixer: self.mixer.clone(), device: self.device.take(), debug: self.debug, rng: self.rng, ..Default::default() };
    }
}

/// The menu's sounds: its patches, in the order of the file (the menu files name them
/// `Sound/PlayMenuOk`, `Sound/PlayMenuHighLight`… as the headings of its comments).
pub const MENU_SOUNDS: &str = "menu/MenuSound.ssc";
pub const MENU_OK: usize = 5;
pub const MENU_HIGHLIGHT: usize = 8;
pub const MENU_START_GAME: usize = 11;

impl Sounds {
    /// A patch heard flat, at its samples' volume (the menu's: no place, no effects).
    pub fn play_flat(&mut self, vfs: &bytefield_vfs::Vfs, path: &str, patch: usize) {
        let Some(p) = self.file_script(vfs, path).and_then(|p| p.get(patch).cloned()) else { return };
        for s in p.iter().filter(|s| !s.triggers.iter().any(|t| t.eq_ignore_ascii_case("release"))) {
            let Some(pcm) = self.wav(vfs, s, None) else { continue };
            if self.debug {
                info!("sound: {} (menu)", s.file_name());
            }
            self.mixer.lock().unwrap().play(pcm, false, [s.volume; 2], 1.0);
        }
    }
}

/// The patches of a template's script: `ObjectTemplate.loadSoundScript`, relative to the file
/// that created it.
/// A soldier has many (`SoldierSound.inc`): `n` picks one. Those come from an `include` in
/// `../Common/Sounds`, which the template does not record: that folder is tried too.
fn load_script(vfs: &bytefield_vfs::Vfs, db: &bytefield_con::templates::TemplateDb, template: &str, n: usize) -> Option<Patches> {
    let t = db.object_template(template)?;
    let rel = t.all("loadSoundScript").get(n)?.first()?.clone();
    let file = t.origin.rsplit_once(':').map_or(t.origin.as_str(), |(f, _)| f);
    let dir = file.rsplit_once(['/', '\\']).map_or("", |(d, _)| d).to_string();
    let common = format!("{}/Common/Sounds", dir.rsplit_once(['/', '\\']).map_or("", |(d, _)| d));
    let read = |dir: &str, rel: &str| -> Option<(String, String)> {
        let path = format!("{dir}/{}", rel.trim_start_matches(['/', '\\']));
        let b = vfs.read(&path).ok()?;
        let d = path.rsplit_once(['/', '\\']).map_or("", |(d, _)| d).to_string();
        Some((String::from_utf8_lossy(&b).into_owned(), d))
    };
    let (text, base) = read(&dir, &rel).or_else(|| read(&common, &rel))?;
    let script: SoundScript = bytefield_formats::ssc::parse(&text, &mut |p| read(&base, p).map(|(t, _)| t));
    Some(Arc::new(script.patches.into_iter().map(|p| p.samples.into_iter().map(Arc::new).collect()).collect()))
}

/// A script by its path in the archives (the menu's).
fn load_file_script(vfs: &bytefield_vfs::Vfs, path: &str) -> Option<Patches> {
    let base = path.rsplit_once('/').map_or("", |(d, _)| d).to_string();
    let text = String::from_utf8_lossy(&vfs.read(path).ok()?).into_owned();
    let script: SoundScript = bytefield_formats::ssc::parse(&text, &mut |p| vfs.read(&format!("{base}/{}", p.trim_start_matches(['/', '\\']))).ok().map(|b| String::from_utf8_lossy(&b).into_owned()));
    Some(Arc::new(script.patches.into_iter().map(|p| p.samples.into_iter().map(Arc::new).collect()).collect()))
}

impl Sounds {
    fn file_script(&mut self, vfs: &bytefield_vfs::Vfs, path: &str) -> Option<Patches> {
        self.scripts.entry(path.to_ascii_lowercase()).or_insert_with(|| load_file_script(vfs, path)).clone()
    }

    fn script(&mut self, vfs: &bytefield_vfs::Vfs, db: &bytefield_con::templates::TemplateDb, template: &str) -> Option<Patches> {
        self.script_n(vfs, db, template, 0)
    }

    fn script_n(&mut self, vfs: &bytefield_vfs::Vfs, db: &bytefield_con::templates::TemplateDb, template: &str, n: usize) -> Option<Patches> {
        let debug = self.debug;
        self.scripts
            .entry(format!("{}#{n}", template.to_ascii_lowercase()))
            .or_insert_with(|| {
                let p = load_script(vfs, db, template, n);
                // (a template without a script is normal; one whose script does not load is silent by mistake)
                if debug && p.is_none() && db.object_template(template).is_some_and(|t| t.all("loadSoundScript").get(n).is_some()) {
                    info!("sound: missing script {n} of {template}");
                }
                p
            })
            .clone()
    }

    /// A sample's wav, where [`Sample::wav_paths`] says, with `@Language/` in the soldier's `radioLanguage` folder.
    fn wav(&mut self, vfs: &bytefield_vfs::Vfs, s: &Sample, lang: Option<&str>) -> Option<Arc<Pcm>> {
        let name = s.rtd_path();
        let debug = self.debug;
        let sub = match lang {
            Some(l) if s.path.to_ascii_lowercase().contains("@language") => format!("{l}/"),
            _ => String::new(),
        };
        let level = self.level.clone();
        self.wavs
            .entry(format!("{sub}{name}").to_ascii_lowercase())
            .or_insert_with(|| {
                let pcm = s
                    .wav_paths(&sub, Some(&level))
                    .into_iter()
                    .find_map(|path| vfs.read(&path).ok())
                    .and_then(|b| parse_wav(&b))
                    .map(Arc::new);
                if debug && pcm.is_none() {
                    info!("sound: missing sample {name}");
                }
                pcm
            })
            .clone()
    }

    fn random(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Starts every sample of a patch; returns the indices in `playing`.
    fn start(&mut self, vfs: &bytefield_vfs::Vfs, patch: &[Arc<Sample>], anchor: Anchor, at: Vec3, ear: &Ear, now: f32) -> Vec<usize> {
        self.start_with(vfs, patch, anchor, at, ear, now, Start::default())
    }

    /// `start` with what drives it, its language and which samples: those of `trigger Release`
    /// (played when it stops) or the others.
    #[allow(clippy::too_many_arguments)]
    fn start_with(&mut self, vfs: &bytefield_vfs::Vfs, patch: &[Arc<Sample>], anchor: Anchor, at: Vec3, ear: &Ear, now: f32, how: Start) -> Vec<usize> {
        let mut out = Vec::new();
        let on_release = |s: &Sample| s.triggers.iter().any(|t| t.eq_ignore_ascii_case("release"));
        let patch: Vec<Arc<Sample>> = patch.iter().filter(|s| on_release(s) == how.release).cloned().collect();
        let picked: Vec<&Arc<Sample>> = if patch.iter().any(|s| s.random_play) {
            // `randomPlay` (once in a patch, after its last sample): one of them all
            let pool: Vec<&Arc<Sample>> = patch.iter().collect();
            let i = (self.random() * pool.len() as f32) as usize;
            vec![pool[i.min(pool.len() - 1)]]
        } else {
            patch.iter().collect()
        };
        for s in picked {
            let c = Controls { distance: (at - ear.pos).length(), ..Default::default() };
            // a near layer heard from afar (or a far one from near) never starts
            if !s.looped && gains(s, at, ear, &c, false).iter().all(|g| *g < 1e-3) {
                continue;
            }
            let Some(pcm) = self.wav(vfs, s, how.lang.as_deref()) else { continue };
            let a = s.random_start_pitch.0;
            let pitch = 1.0 + a * (2.0 * self.random() - 1.0);
            let g = gains(s, at, ear, &c, true);
            let voice = self.mixer.lock().unwrap().play(pcm, s.looped, g, pitch);
            if self.debug {
                info!("sound: {} at {:.0} m, gain {:.2}/{:.2}{}", s.file_name(), c.distance, g[0], g[1], if s.looped { ", loop" } else { "" });
            }
            self.playing.push(Playing { voice, sample: s.clone(), anchor, started: now, released: None, pitch, effect: None, drive: how.drive });
            out.push(self.playing.len() - 1);
        }
        out
    }
}

/// How [`Sounds::start_with`] starts a patch.
#[derive(Default)]
struct Start {
    drive: Drive,
    lang: Option<String>,
    release: bool,
}

/// The parts of a vehicle template (itself included) that have a sound script, but its weapons
/// and projectiles (the shots play those): (template, kind).
fn sound_parts(db: &bytefield_con::templates::TemplateDb, name: &str, out: &mut Vec<(String, String)>, depth: usize) {
    let Some(t) = db.object_template(name) else { return };
    if depth > 12 || matches!(t.kind.to_ascii_lowercase().as_str(), "firearms" | "projectile") {
        return;
    }
    if t.get("loadSoundScript").is_some() {
        out.push((t.name.clone(), t.kind.to_ascii_lowercase()));
    }
    for c in t.all("addTemplate") {
        if let Some(c) = c.first() {
            sound_parts(db, c, out, depth + 1);
        }
    }
}

/// An `AreaObject` (a lake, a coastline): heard while the ear is within `triggerRadius` of its
/// closed line, from the line's nearest point. In the game's
/// frame; the line's points relative to the object, in x/z.
#[derive(Clone, Debug, Default)]
pub struct Area {
    pub at: [f32; 3],
    pub line: Vec<[f32; 2]>,
    pub radius: f32,
    /// the template's box, x/z: from the origin, each point pushed out by twice the
    /// radius set when it was added, and ±2·radius at each `triggerRadius`
    pub lo: [f32; 2],
    pub hi: [f32; 2],
    /// the patch, and whether the ear was in at the last frame
    patch: Vec<Arc<Sample>>,
    inside: bool,
}

impl Area {
    /// The template's `triggerRadius` and `addLinePoint`s, in their order.
    pub fn new(t: &bytefield_con::templates::Template, at: [f32; 3]) -> Area {
        let mut a = Area { at, ..Default::default() };
        let grow = |a: &mut Area, p: [f32; 2]| {
            for i in 0..2 {
                a.lo[i] = a.lo[i].min(p[i]);
                a.hi[i] = a.hi[i].max(p[i]);
            }
        };
        for p in &t.props {
            match p.name.as_str() {
                "triggerradius" | "settriggerradius" => {
                    a.radius = p.args.first().and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0).abs();
                    let r = 2.0 * a.radius;
                    grow(&mut a, [r, r]);
                    grow(&mut a, [-r, -r]);
                }
                "addlinepoint" => {
                    let v: Vec<f32> = p.args.first().map(|x| x.split('/').filter_map(|c| c.parse().ok()).collect()).unwrap_or_default();
                    let (Some(&x), Some(&z)) = (v.first(), v.get(1)) else { continue };
                    a.line.push([x, z]);
                    let out = |c: f32| if c >= 0.0 { 2.0 * a.radius } else { -2.0 * a.radius };
                    let q = [x + out(x), z + out(z)];
                    grow(&mut a, q);
                }
                _ => {}
            }
        }
        a
    }

    /// Where it is heard from with the ear at `ear` (the game's frame), or None: fewer than three
    /// points, the ear outside the box or farther than the radius from every side.
    pub fn heard(&self, ear: [f32; 3]) -> Option<[f32; 3]> {
        let n = self.line.len();
        let d = [ear[0] - self.at[0], ear[2] - self.at[2]];
        if n < 3 || !(0..2).all(|i| self.lo[i] <= d[i] && d[i] <= self.hi[i]) {
            return None;
        }
        let mut best = (f32::MAX, [0.0f32; 2]);
        for i in 0..n {
            let (a, b) = (self.line[i], self.line[(i + 1) % n]);
            let ab = [b[0] - a[0], b[1] - a[1]];
            let len = (ab[0] * ab[0] + ab[1] * ab[1]).sqrt();
            let t = if len > 0.0 { ((d[0] - a[0]) * ab[0] + (d[1] - a[1]) * ab[1]) / len } else { -1.0 };
            let q = if t < 0.0 {
                a
            } else if t <= len {
                [a[0] + ab[0] / len * t, a[1] + ab[1] / len * t]
            } else {
                b
            };
            let e = (q[0] - d[0]).powi(2) + (q[1] - d[1]).powi(2);
            if e < best.0 {
                best = (e, q);
            }
        }
        (best.0 < self.radius * self.radius).then(|| [self.at[0] + best.1[0], self.at[1], self.at[2] + best.1[1]])
    }
}

/// The audio device (feature `audio`).
#[cfg(feature = "audio")]
struct Device(#[allow(dead_code)] cpal::Stream);
#[cfg(not(feature = "audio"))]
struct Device;

// SAFETY: the stream is only kept alive, never touched from another thread.
unsafe impl Send for Device {}
unsafe impl Sync for Device {}

#[cfg(feature = "audio")]
fn open_device(mixer: Arc<Mutex<Mixer>>) -> Option<Device> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let config = device.default_output_config().ok()?;
    let rate = config.sample_rate();
    let channels = config.channels() as usize;
    let mut buf = Vec::new();
    let stream = device
        .build_output_stream(
            config.into(),
            move |out: &mut [f32], _| {
                let frames = out.len() / channels;
                buf.resize(frames * 2, 0.0);
                mixer.lock().unwrap().mix(&mut buf, rate);
                for (f, o) in out.chunks_exact_mut(channels).enumerate() {
                    for (k, s) in o.iter_mut().enumerate() {
                        *s = if k < 2 { buf[f * 2 + k] } else { 0.0 };
                    }
                }
            },
            |e| warn!("audio: {e}"),
            None,
        )
        .ok()?;
    stream.play().ok()?;
    info!("sound: {rate} Hz, {channels} channels");
    Some(Device(stream))
}

#[cfg(not(feature = "audio"))]
fn open_device(_: Arc<Mutex<Mixer>>) -> Option<Device> {
    None
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        let mut s = Sounds { rng: 0x9e3779b97f4a7c15, debug: std::env::var_os("BYTEFIELD_SOUND_DEBUG").is_some(), ..Default::default() };
        s.device = open_device(s.mixer.clone());
        if s.device.is_none() {
            info!("sound: no audio device, silent");
        }
        app.insert_resource(s).add_systems(Update, update.after(crate::effects::EffectsUpdate).run_if(resource_exists::<Game>));
    }
}

/// The soldier's or vehicle's position now.
fn anchor_pos(g: &Game, areas: &[Area], ear: &Ear, a: Anchor) -> Option<Vec3> {
    match a {
        Anchor::Point(p) => Some(p),
        Anchor::Vehicle(i) => g.session.sim.vehicles.get(i).map(|v| crate::assets::to_bevy(v.craft.body.pos)),
        Anchor::Ear => Some(ear.pos),
        Anchor::Area(k) => areas.get(k).and_then(|l| l.heard(from_bevy(ear.pos))).map(crate::assets::to_bevy),
    }
}

/// `Default` and `Speed` now.
fn drive_values(g: &Game, d: Drive) -> (Option<f32>, Option<f32>) {
    let vs = &g.session.sim.vehicles;
    let speed = |i: usize| vs.get(i).map(|v| Vec3::from(v.craft.body.vel).length());
    match d {
        Drive::None => (None, None),
        Drive::Speed(i) => (None, speed(i)),
        Drive::Fixed(x) => (Some(x), None),
        Drive::Engine(i, k) => {
            let rpm = vs.get(i).and_then(|v| v.craft.parts.get(k)).and_then(|p| match &p.kind {
                bytefield_sim::craft::Kind::Engine(e) => Some(e.rpm.abs()),
                _ => None,
            });
            (rpm, speed(i))
        }
        Drive::Bundle(i, k) => {
            let w = vs.get(i).and_then(|v| v.craft.parts.get(k)).and_then(|p| p.bundle.as_ref()).map(|b| b.speed.iter().fold(0.0f32, |m, x| m.max(x.abs())));
            (w, speed(i))
        }
    }
}

/// The material of the object (a building, a vehicle) a soldier stands on: the last contact pass's
/// impulses against an object whose normal points up.
fn under_foot(v: &bytefield_sim::mover::Vehicle) -> Option<u32> {
    v.contacts.iter().flat_map(|c| &c.last_impulses).filter(|i| i.with.is_some() && i.n[1] > 0.5).max_by(|a, b| a.n[1].total_cmp(&b.n[1])).map(|i| i.materials.1)
}

/// Reads the global `SoldierSound.*` settings (they are not templates): the step frequencies and
/// which step patch each terrain material plays.
fn load_soldier_settings(vfs: &bytefield_vfs::Vfs, s: &mut Sounds) {
    for f in ["objects/soldiers/common/sounds/soldiersound.inc", "objects/soldiers/common/sounds/materialtosound.con"] {
        let Ok(b) = vfs.read(f) else { continue };
        for line in String::from_utf8_lossy(&b).lines() {
            let w: Vec<&str> = line.split_whitespace().collect();
            let Some(cmd) = w.first().and_then(|c| c.to_ascii_lowercase().strip_prefix("soldiersound.").map(str::to_string)) else { continue };
            if cmd == "addmaterialtosound" {
                if let (Some(m), Some(n)) = (w.get(1).and_then(|x| x.parse().ok()), w.get(2)) {
                    if let Some(k) = STEP_PATCHES.iter().position(|p| p.eq_ignore_ascii_case(n)) {
                        s.material_patch.insert(m, k);
                    }
                }
            } else if let (Some(name), Some(v)) = (cmd.strip_prefix("set"), w.get(1).and_then(|x| x.parse().ok())) {
                s.soldier.insert(name.to_string(), v);
            }
        }
    }
}

/// The level's ambience (`Sounds/Environment.con`: `EnvironmentSound.load Environment.ssc`) with
/// the listener, and the `AreaObject`s placed in it (`addLinePoint`s around their position).
fn start_level(g: &Game, s: &mut Sounds, ear: &Ear, now: f32) {
    let sim = &g.session.sim;
    let (vfs, db, level) = (&sim.world.vfs, &sim.world.db, &sim.world.level);
    let dir = format!("bf1942/levels/{}/sounds", level.name);
    let read = |p: &str| vfs.read(&format!("{dir}/{p}")).ok().map(|b| String::from_utf8_lossy(&b).into_owned());
    if let Some(con) = read("environment.con") {
        for line in con.lines() {
            let w: Vec<&str> = line.split_whitespace().collect();
            if w.len() >= 2 && w[0].eq_ignore_ascii_case("EnvironmentSound.load") {
                let Some(text) = read(w[1]) else { continue };
                let script = bytefield_formats::ssc::parse(&text, &mut |p| read(p));
                for p in script.patches {
                    let p: Vec<Arc<Sample>> = p.samples.into_iter().map(Arc::new).collect();
                    s.start(vfs, &p, Anchor::Ear, ear.pos, ear, now);
                }
            }
        }
    }
    let placed = emitters(db, level);
    if s.debug {
        info!("sound: {} placed objects with a sound script", placed.len());
    }
    for (template, at) in placed {
        if let Some(p) = s.script(vfs, db, &template).and_then(|p| p.first().cloned()) {
            s.start(vfs, &p, Anchor::Point(crate::assets::to_bevy(at)), crate::assets::to_bevy(at), ear, now);
        }
    }
    for placed in &level.placed {
        let Some(t) = db.object_template(&placed.template) else { continue };
        if !t.kind.eq_ignore_ascii_case("AreaObject") {
            continue;
        }
        let Some(patches) = s.script(vfs, db, &placed.template) else { continue };
        let mut area = Area::new(t, placed.pos.map(|c| c as f32));
        area.patch = patches.first().cloned().unwrap_or_default();
        s.areas.push(area);
    }
}

/// The level's placed `SimpleObject`s with a sound script (GC's birds, crickets, radio chatter, waves, Lok's earthquake, Endor's
/// ewoks; Desert Combat's bushes with insects and its rivers): their first patch plays from the object for the whole round, with the usual falloff; `triggerRadius` does not gate it.
/// Level 2: recorded in the original (two such objects added to GC_Hoth's archive), one 30 m away with a
/// `triggerRadius` of 10 was heard 9 dB under one 3 m away (the falloff past its `minDistance` 10), before and after spawning.
/// TODO: whether the original stops them farther away (a radius of its own, the channel limit of `game.setChannels 32`) is not
/// measured; ours are many loops (GC_Corellia's crickets and birds) that the mixer's voice limit silences.
fn emitters(db: &bytefield_con::templates::TemplateDb, level: &bytefield_world::Level) -> Vec<(String, [f32; 3])> {
    level
        .placed
        .iter()
        .filter(|p| db.object_template(&p.template).is_some_and(|t| t.kind.eq_ignore_ascii_case("SimpleObject") && !t.all("loadSoundScript").is_empty()))
        .map(|p| (p.template.clone(), p.pos.map(|c| c as f32)))
        .collect()
}

/// The areas the ear came into start their patch, those it left stop it.
fn update_areas(s: &mut Sounds, ear: &Ear, now: f32, vfs: &bytefield_vfs::Vfs) {
    for k in 0..s.areas.len() {
        let at = s.areas[k].heard(from_bevy(ear.pos));
        if at.is_some() == s.areas[k].inside {
            continue;
        }
        s.areas[k].inside = at.is_some();
        match at {
            Some(at) => {
                let p = s.areas[k].patch.clone();
                s.start(vfs, &p, Anchor::Area(k), crate::assets::to_bevy(at), ear, now);
            }
            None => {
                let mut m = s.mixer.lock().unwrap();
                for p in s.playing.iter().filter(|p| p.anchor == Anchor::Area(k)) {
                    m.stop(p.voice, true);
                }
            }
        }
    }
}

/// A point of Bevy's frame in the game's.
fn from_bevy(v: Vec3) -> [f32; 3] {
    [v.x, v.y, -v.z]
}

/// How long after an automatic weapon's last shot its loop is released (a guess: the slowest
/// machine guns fire some 8 rounds a second).
const LOOP_RELEASE: f32 = 0.25;

fn update(g: Res<Game>, mut sounds: ResMut<Sounds>, mut fx: ResMut<crate::effects::Effects>, cam: Query<&GlobalTransform, With<crate::play::PlayCamera>>, time: Res<Time>) {
    let Some(cam) = cam.iter().next() else { return };
    let ear = Ear { pos: cam.translation(), right: cam.right().as_vec3() };
    // the ear's velocity, smoothed; a jump (a respawn, a camera switch) counts as standing still
    let dt = time.delta_secs();
    if let (Some(last), true) = (sounds.last_ear, dt > 0.0) {
        let v = (ear.pos - last) / dt;
        let v = if v.length() > MAX_EAR_SPEED { Vec3::ZERO } else { v };
        sounds.ear_vel = sounds.ear_vel.lerp(v, (dt * 10.0).min(1.0));
    }
    sounds.last_ear = Some(ear.pos);
    let sim = &g.session.sim;
    let vfs = &sim.world.vfs;
    let db = &sim.world.db;
    let now = sim.clock.steps as f32 * crate::play::TICK as f32;
    let s = &mut *sounds;
    if s.level.is_empty() {
        s.level = sim.world.level.name.clone();
    }
    // shots: the weapon's fire patch, or its fire loop while it keeps firing
    // (cursors count from the round's start: the sim cuts its logs, `trim_logs`)
    for (_, i, shot, proj) in bytefield_sim::log_since(&sim.journal.fired, sim.journal.log_base.fired, &mut s.seen) {
        let v = &sim.vehicles[*i];
        let name = match v.movement.as_ref() {
            Some(m) => m.weapon.as_ref().map(|w| w.template.name.clone()),
            None => {
                let pn = proj.and_then(|k| sim.projectiles.get(k)).map(|q| q.template.name.clone());
                v.weapons
                    .iter()
                    .find(|(_, w)| pn.as_ref().is_some_and(|n| *n == w.template.projectile || w.template.tracer.as_ref().is_some_and(|t| t.0 == *n)))
                    .or(v.weapons.first())
                    .map(|(_, w)| w.template.name.clone())
            }
        };
        let Some(name) = name else { continue };
        if s.debug {
            info!("sound: shot of {name} by {i} at step {}", sim.clock.steps);
        }
        let Some(patches) = s.script(vfs, db, &name) else { continue };
        let at = crate::assets::to_bevy(shot.pos);
        let anchor = Anchor::Vehicle(*i);
        let fire = patches.first().filter(|p| p.iter().any(|x| !x.file_name().eq_ignore_ascii_case("silence.wav")));
        // both, when both are there: Desert Combat's automatics keep only the distant shot in patch 0 (silent
        // within 50 m) and the near one in the loop (`AK47/Sounds/High.ssc`); vanilla's put `silence.wav` in patch 0
        if let Some(p) = fire {
            let p = p.clone();
            s.start(vfs, &p, anchor, at, &ear, now);
        }
        if let Some(lp) = patches.get(5).filter(|p| !p.is_empty()) {
            let key = (*i, name.to_ascii_lowercase());
            if let Some(l) = s.loops.get_mut(&key) {
                l.1 = now;
            } else {
                let lp = lp.clone();
                let idx = s.start(vfs, &lp, anchor, at, &ear, now);
                s.loops.insert(key, (idx.iter().map(|k| s.playing[*k].voice as usize).collect(), now));
            }
        }
    }
    // reloads: the weapon's patch 1 when its reload starts
    let mut reloading = std::mem::take(&mut s.reloading);
    for (i, v) in sim.vehicles.iter().enumerate() {
        let Some(w) = v.movement.as_ref().and_then(|m| m.weapon.as_ref()) else { continue };
        if w.reload > 0.0 && !reloading.contains(&i) {
            if let Some(p) = s.script(vfs, db, &w.template.name).and_then(|p| p.get(1).cloned()) {
                let at = crate::assets::to_bevy(v.craft.body.pos);
                s.start(vfs, &p, Anchor::Vehicle(i), at, &ear, now);
            }
        }
    }
    reloading.clear();
    reloading.extend(sim.vehicles.iter().enumerate().filter(|(_, v)| v.movement.as_ref().and_then(|m| m.weapon.as_ref()).is_some_and(|w| w.reload > 0.0)).map(|(i, _)| i));
    s.reloading = reloading;
    // effects (explosions, hits, fires): their bundle's script, its first patch
    for (name, at, on, id) in std::mem::take(&mut fx.started) {
        let Some(p) = s.script(vfs, db, &name).and_then(|p| p.first().cloned()) else { continue };
        let anchor = on.map_or(Anchor::Point(at), Anchor::Vehicle);
        let at = anchor_pos(&g, &s.areas, &ear, anchor).unwrap_or(at);
        let idx = s.start(vfs, &p, anchor, at, &ear, now);
        for k in idx {
            s.playing[k].effect = Some(id);
        }
    }
    for id in std::mem::take(&mut fx.stopped) {
        for p in s.playing.iter_mut().filter(|p| p.effect == Some(id)) {
            p.released = Some(now);
            s.mixer.lock().unwrap().stop(p.voice, false);
        }
    }
    // loops whose weapon stopped: let them finish their round, and play the release patch
    let stale: Vec<(usize, String)> = s.loops.iter().filter(|(_, l)| now - l.1 > LOOP_RELEASE).map(|(k, _)| k.clone()).collect();
    for key in stale {
        let (voices, _) = s.loops.remove(&key).unwrap();
        for p in s.playing.iter_mut().filter(|p| voices.contains(&(p.voice as usize))) {
            p.released = Some(now);
            let immediate = !p.sample.stop.as_deref().is_some_and(|x| x.eq_ignore_ascii_case("FinishSample"));
            s.mixer.lock().unwrap().stop(p.voice, immediate);
        }
        if let Some(rel) = s.script(vfs, db, &key.1).and_then(|p| p.get(2).cloned()) {
            if let Some(at) = anchor_pos(&g, &s.areas, &ear, Anchor::Vehicle(key.0)) {
                s.start(vfs, &rel, Anchor::Vehicle(key.0), at, &ear, now);
            }
        }
    }
    if !s.level_started {
        s.level_started = true;
        load_soldier_settings(vfs, s);
        start_level(&g, s, &ear, now);
    }
    update_areas(s, &ear, now, &g.session.sim.world.vfs);
    hums(&g, s, &ear, now);
    pings(&g, s, &ear, now, dt);
    steps(&g, s, &ear, now);
    radio(&g, s, &ear, now);
    // every voice: its gain and pitch now; past the limit, the lowest priority is not heard
    let mut mixer = s.mixer.lock().unwrap();
    s.playing.retain(|p| mixer.playing(p.voice));
    let mut heard: Vec<(u64, [f32; 2], f32, i32)> = Vec::with_capacity(s.playing.len());
    for p in &s.playing {
        let Some(at) = anchor_pos(&g, &s.areas, &ear, p.anchor) else { continue };
        let at = at + Vec3::from(p.sample.relative_position);
        let (default, speed) = drive_values(&g, p.drive);
        let c = Controls { distance: (at - ear.pos).length(), time: now - p.started, release: p.released.map(|r| now - r), default, speed };
        let mut pitch = p.pitch * effect(&p.sample, "Pitch", &c, true);
        if p.sample.doppler {
            let src = match p.anchor {
                Anchor::Vehicle(i) => g.session.sim.vehicles.get(i).map_or(Vec3::ZERO, |v| crate::assets::to_bevy(v.craft.body.vel)),
                _ => Vec3::ZERO,
            };
            pitch *= doppler(at, ear.pos, src, s.ear_vel);
        }
        heard.push((p.voice, gains(&p.sample, at, &ear, &c, true), pitch, p.sample.priority));
    }
    limit_voices(&mut heard, VOICES);
    for (voice, gain, pitch, _) in heard {
        mixer.set(voice, gain, pitch);
    }
}

/// Silences all but the `n` voices heard with the highest priority (then the loudest).
/// `Sound.setDopplerFactor 1`: DirectSound's real-world shift, with sound at 343 m/s; each speed
/// along the line between them is held under half of that.
pub fn doppler(at: Vec3, ear: Vec3, src_vel: Vec3, ear_vel: Vec3) -> f32 {
    let d = ear - at;
    if d.length_squared() < 1e-4 {
        return 1.0;
    }
    let d = d.normalize();
    let lim = SOUND_SPEED * 0.5;
    let vs = src_vel.dot(d).clamp(-lim, lim);
    let vl = ear_vel.dot(d).clamp(-lim, lim);
    (SOUND_SPEED - vl) / (SOUND_SPEED - vs)
}

/// m/s
const SOUND_SPEED: f32 = 343.0;
/// faster than this, the ear jumped
const MAX_EAR_SPEED: f32 = 300.0;

fn limit_voices(heard: &mut [(u64, [f32; 2], f32, i32)], n: usize) {
    let loud = |g: &[f32; 2]| g[0].max(g[1]);
    let mut order: Vec<usize> = (0..heard.len()).filter(|&i| loud(&heard[i].1) > 1e-3).collect();
    if order.len() <= n {
        return;
    }
    order.sort_by(|&a, &b| heard[b].3.cmp(&heard[a].3).then(loud(&heard[b].1).total_cmp(&loud(&heard[a].1))));
    for &i in &order[n..] {
        heard[i].1 = [0.0, 0.0];
    }
}

/// How long a released hum keeps playing: its `TimeRelease` fades last 0.4 s.
const HUM_RELEASE: f32 = 1.0;

/// The vehicles' engines, tracks and turrets: their parts' first patch while an engine is on (a
/// driver is in), driven by the rpm, the turret's speed and the vehicle's; when it stops, released
/// (their `TimeRelease` fades) with the `trigger Release` samples (the engine stopping).
fn hums(g: &Game, s: &mut Sounds, ear: &Ear, now: f32) {
    let sim = &g.session.sim;
    let (vfs, db) = (&sim.world.vfs, &sim.world.db);
    for (i, v) in sim.vehicles.iter().enumerate() {
        if v.movement.is_some() {
            continue;
        }
        let on = !v.removed && v.craft.parts.iter().any(|p| matches!(&p.kind, bytefield_sim::craft::Kind::Engine(e) if e.on));
        let humming = s.hums.get(&i).is_some_and(|h| h.released.is_none());
        if on == humming {
            continue;
        }
        let parts = s
            .parts
            .entry(v.template.to_ascii_lowercase())
            .or_insert_with(|| {
                let mut out = Vec::new();
                sound_parts(db, &v.template, &mut out, 0);
                Arc::new(out)
            })
            .clone();
        let at = crate::assets::to_bevy(v.craft.body.pos);
        let mut voices = Vec::new();
        for (name, kind) in parts.iter() {
            let Some(p) = s.script(vfs, db, name).and_then(|p| p.first().cloned()) else { continue };
            let part = v.craft.parts.iter().position(|q| q.name.eq_ignore_ascii_case(name));
            let drive = match (kind.as_str(), part) {
                ("engine", Some(k)) => Drive::Engine(i, k),
                ("rotationalbundle", Some(k)) => Drive::Bundle(i, k),
                _ => Drive::Speed(i),
            };
            let idx = s.start_with(vfs, &p, Anchor::Vehicle(i), at, ear, now, Start { drive, lang: None, release: !on });
            voices.extend(idx.iter().map(|&k| s.playing[k].voice));
        }
        if on {
            if s.debug {
                info!("sound: {} ({i}) starts, {} voices", v.template, voices.len());
            }
            s.hums.insert(i, Hum { voices, released: None });
        } else if let Some(h) = s.hums.get_mut(&i) {
            h.released = Some(now);
            for p in s.playing.iter_mut().filter(|p| h.voices.contains(&p.voice)) {
                p.released = Some(now);
            }
        }
    }
    let done: Vec<usize> = s.hums.iter().filter(|(_, h)| h.released.is_some_and(|r| now - r > HUM_RELEASE)).map(|(i, _)| *i).collect();
    for i in done {
        let h = s.hums.remove(&i).unwrap();
        let mut m = s.mixer.lock().unwrap();
        for v in h.voices {
            m.stop(v, true);
        }
    }
}

/// The sonars' pings: while the player sits where
/// `subPos` or `sonarPos` is set, each sonar that scans plays its script's first patch, at its vehicle, when the other
/// team's destroyer comes near and again as its timer runs out; with `max(d/r, 0.1)` as `Default` (`SonarSub.ssc`:
/// the volume `1 − x`). A ping restarts its sonar's sound.
fn pings(g: &Game, s: &mut Sounds, ear: &Ear, now: f32, dt: f32) {
    let sim = &g.session.sim;
    let (vfs, db) = (&sim.world.vfs, &sim.world.db);
    let me = sim.players.get(g.player).filter(|p| p.alive).and_then(|p| p.soldier);
    if !me.is_some_and(|i| sim.hears_sonars(i)) {
        // (`handleUpdate` returns before its timers)
        return;
    }
    for (k, v) in sim.vehicles.iter().enumerate() {
        let Some(sonar) = v.entry.sonar.as_ref().filter(|o| o.scan && v.in_world()) else {
            s.pings.remove(&k);
            continue;
        };
        let entry = s.pings.entry(k).or_default();
        if !entry.0.update(dt, || sim.enemy_sonars(k)) {
            continue;
        }
        let value = entry.0.value;
        let old = std::mem::take(&mut entry.1);
        {
            let mut m = s.mixer.lock().unwrap();
            for voice in old {
                m.stop(voice, true);
            }
        }
        let Some(p) = s.script(vfs, db, &sonar.template).and_then(|p| p.first().cloned()) else { continue };
        let at = crate::assets::to_bevy(sim.sonar_at(k).unwrap_or(v.craft.body.pos));
        let idx = s.start_with(vfs, &p, Anchor::Vehicle(k), at, ear, now, Start { drive: Drive::Fixed(value), ..Default::default() });
        let voices = idx.iter().map(|&i| s.playing[i].voice).collect();
        if s.debug {
            info!("sound: sonar ping of {} ({k}), {value:.2}", v.template);
        }
        s.pings.get_mut(&k).unwrap().1 = voices;
    }
}

/// What the players said: the team's messages for the player's team, as they are; those said
/// out loud, at the soldier, for everyone near. In the speaker's `radioLanguage`.
fn radio(g: &Game, s: &mut Sounds, ear: &Ear, now: f32) {
    let sim = &g.session.sim;
    let (vfs, db) = (&sim.world.vfs, &sim.world.db);
    let mine = sim.players.get(g.player).map(|p| p.team);
    for e in bytefield_sim::log_since(&sim.journal.radio_log, sim.journal.log_base.radio, &mut s.seen_radio) {
        let Some(v) = e.soldier.and_then(|i| sim.vehicles.get(i)) else { continue };
        let lang = db.object_template(&v.template).and_then(|t| t.get("radioLanguage")?.first().map(|l| l.trim_matches('"').to_string()));
        let how = Start { lang, ..Default::default() };
        if e.msg.local {
            let Some(p) = s.script_n(vfs, db, &v.template, VOICE).and_then(|p| p.get(e.msg.patch).cloned()) else { continue };
            let i = e.soldier.unwrap();
            s.start_with(vfs, &p, Anchor::Vehicle(i), crate::assets::to_bevy(v.craft.body.pos), ear, now, how);
        } else if Some(e.team) == mine {
            let Some(p) = s.file_script(vfs, MENU_RADIO).and_then(|p| p.get(e.msg.patch).cloned()) else { continue };
            s.start_with(vfs, &p, Anchor::Ear, ear.pos, ear, now, how);
        }
    }
}

/// The soldiers: a step at each `c_Sst*` trigger their lower body's state sets (once on entering
/// it, or every `set*Frequency` seconds for walking, running…) with the patch of the ground's
/// material, and their voice when hurt (at most every `setHitDamageFrequency`) or killed.
fn steps(g: &Game, s: &mut Sounds, ear: &Ear, now: f32) {
    let sim = &g.session.sim;
    let (vfs, db) = (&sim.world.vfs, &sim.world.db);
    let lang = |t: &str| db.object_template(t).and_then(|t| t.get("radioLanguage")?.first().map(|l| l.trim_matches('"').to_string()));
    for (i, v) in sim.vehicles.iter().enumerate() {
        let Some(m) = v.movement.as_ref() else { continue };
        if !v.in_world() {
            s.steps.remove(&i);
            continue;
        }
        let sound = m.lower.sound(&m.machine);
        let jitter = 2.0 * s.random() - 1.0;
        let st = s.steps.entry(i).or_default();
        let periodic = PERIODIC.iter().find(|(t, _)| *t == sound).map(|(_, n)| *n);
        let play = if sound != st.sound {
            if s.debug {
                info!("sound: soldier {i} trigger {} -> {sound}", st.sound);
            }
            st.sound = sound;
            match periodic {
                Some(n) => {
                    st.next = now + s.soldier.get(&format!("{n}frequency")).copied().unwrap_or(0.5) * 0.5;
                    false
                }
                None => sound > 0,
            }
        } else if let (Some(n), true) = (periodic, now >= st.next) {
            let f = s.soldier.get(&format!("{n}frequency")).copied().unwrap_or(0.5);
            let r = s.soldier.get(&format!("random{n}frequency")).copied().unwrap_or(0.0);
            st.next = now + f + r * jitter;
            true
        } else {
            false
        };
        if !play {
            continue;
        }
        let Some(patches) = s.script_n(vfs, db, &v.template, (sound - 1) as usize) else { continue };
        let pos = v.craft.body.pos;
        let at = crate::assets::to_bevy(pos);
        let mut which = vec![0];
        if patches.len() >= STEP_PATCHES.len() - 1 {
            // by material: an object's face under the feet, else the terrain's (grass where the
            // map says nothing)
            let mat = under_foot(v).unwrap_or_else(|| sim.world.level.material_at(pos[0] as f64, pos[2] as f64));
            which = vec![s.material_patch.get(&mat).copied().unwrap_or(4)];
            if matches!(sound, 2 | 3) {
                which.push(HARNESS);
            }
        }
        for k in which {
            if let Some(p) = patches.get(k).cloned() {
                s.start_with(vfs, &p, Anchor::Vehicle(i), at, ear, now, Start { lang: lang(&v.template), ..Default::default() });
            }
        }
    }
    for e in bytefield_sim::log_since(&sim.journal.damage_log, sim.journal.log_base.damage, &mut s.seen_damage) {
        let Some(v) = sim.vehicles.get(e.vehicle) else { continue };
        if v.movement.is_none() || e.amount <= 0.0 {
            continue;
        }
        let killed = e.hp <= 0.0 && e.hp + e.amount > 0.0;
        if e.hp <= 0.0 && !killed {
            continue;
        }
        let gap = s.soldier.get("hitdamagefrequency").copied().unwrap_or(1.0);
        let st = s.steps.entry(e.vehicle).or_default();
        if !killed && now - st.hurt < gap {
            continue;
        }
        st.hurt = now;
        let n = if killed { KILLED } else { HIT_DAMAGE };
        let Some(p) = s.script_n(vfs, db, &v.template, n).and_then(|p| p.first().cloned()) else { continue };
        let at = crate::assets::to_bevy(v.craft.body.pos);
        s.start_with(vfs, &p, Anchor::Vehicle(e.vehicle), at, ear, now, Start { lang: lang(&v.template), ..Default::default() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_volume_follows_the_originals_curves() {
        // (what the original gave at setMasterVolume 75 and 50, in dB: effects −4.4…−5.1 and −11.3…−12.1, music −3.9 and −9.6)
        let db = |g: f32| 20.0 * g.log10();
        assert_eq!(master_gains(1.0), (1.0, 1.0));
        let (e, m) = master_gains(0.75);
        assert!((-5.2..-4.3).contains(&db(e)) && (-4.2..-3.7).contains(&db(m)), "{} {}", db(e), db(m));
        let (e, m) = master_gains(0.5);
        assert!((-12.2..-11.2).contains(&db(e)) && (-9.9..-9.3).contains(&db(m)), "{} {}", db(e), db(m));
    }

    fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
        let mut b = Vec::new();
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        for v in [2u16, 16] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&data);
        b
    }

    /// What the master volume of now gives the effects.
    fn effects_gain() -> f32 {
        master_gains(f32::from_bits(VOLUME.load(std::sync::atomic::Ordering::Relaxed))).0
    }

    #[test]
    fn a_wav_mixes_at_the_device_rate() {
        let pcm = Arc::new(parse_wav(&wav(&[16384; 100], 22050)).unwrap());
        assert_eq!((pcm.rate, pcm.frames()), (22050, 100));
        let mut m = Mixer::default();
        let id = m.play(pcm, false, [1.0, 0.5], 1.0);
        let mut out = vec![0.0; 2 * 300];
        m.mix(&mut out, 44100);
        let k = effects_gain();
        assert!((out[0] - 0.5 * k).abs() < 1e-3 && (out[1] - 0.25 * k).abs() < 1e-3);
        // 100 frames at 22050 Hz are 200 at 44100
        assert!(out[2 * 199] > 0.4 * k && out[2 * 201] == 0.0);
        assert!(!m.playing(id));
    }

    #[test]
    fn a_loop_finishes_its_round() {
        let pcm = Arc::new(parse_wav(&wav(&[16384; 10], 100)).unwrap());
        let mut m = Mixer::default();
        let id = m.play(pcm, true, [1.0, 1.0], 1.0);
        let mut out = vec![0.0; 2 * 25];
        m.mix(&mut out, 100);
        let k = effects_gain();
        assert!(m.playing(id) && out[2 * 24] > 0.4 * k);
        m.stop(id, false);
        m.mix(&mut out, 100);
        assert!(out[0] > 0.4 * k && out[2 * 6] == 0.0, "the round ends at frame 5");
        assert!(!m.playing(id));
    }

    #[test]
    fn the_near_layer_fades_with_distance() {
        let text = "newPatch\nload @ROOT/Sound/@RTD/near.wav\nminDistance 2\nbeginEffect\ncontrolDestination Volume\n\
            controlSource Distance\nenvelope Ramp\nparam 30\nparam 50\nparam 1\nparam -1\nendEffect\n";
        let s = bytefield_formats::ssc::parse(text, &mut |_| None).patches[0].samples[0].clone();
        let ear = Ear { pos: Vec3::ZERO, right: Vec3::X };
        let g = |x: f32| gains(&s, Vec3::new(x, 0.0, 0.0), &ear, &Controls { distance: x.abs(), ..Default::default() }, true);
        let near = g(1.0);
        assert!(near[1] > near[0], "to the right");
        assert!((near[1] - 1.0).abs() < 1e-3, "full within minDistance");
        assert!((g(4.0)[1] - 0.5).abs() < 1e-3, "2/4");
        assert_eq!(g(-60.0), [0.0, 0.0]);
    }

    #[test]
    fn the_scripts_of_the_data_load() {
        let Some(w) = crate::play::with_data::world("El_Alamein") else { return };
        let mut s = Sounds::default();
        for (t, patch, wav) in [("K98", 0, "k98LR.wav"), ("Mp40", 5, "mp40lrlp.wav"), ("e_ExplGranade", 0, "")] {
            let p = s.script(&w.vfs, &w.db, t).unwrap_or_else(|| panic!("{t}: no script"));
            let samples = &p[patch];
            assert!(!samples.is_empty(), "{t}: patch {patch} empty");
            if !wav.is_empty() {
                assert!(samples.iter().any(|x| x.file_name().eq_ignore_ascii_case(wav)), "{t}: {wav}");
            }
            for x in samples {
                assert!(s.wav(&w.vfs, x, Some("German")).is_some(), "{t}: {} does not load", x.path);
            }
        }
    }

    #[test]
    fn doppler_rises_coming_and_falls_going() {
        let (at, ear) = (Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0));
        let toward = Vec3::new(50.0, 0.0, 0.0);
        assert!((doppler(at, ear, toward, Vec3::ZERO) - 343.0 / 293.0).abs() < 1e-4);
        assert!(doppler(at, ear, -toward, Vec3::ZERO) < 1.0);
        assert!(doppler(at, ear, -toward, toward) < 1.0, "the ear going away too");
        assert!((doppler(at, ear, toward, toward) - 1.0).abs() < 1e-4, "riding together");
        assert_eq!(doppler(at, ear, Vec3::new(0.0, 0.0, 50.0), Vec3::ZERO), 1.0, "across");
    }

    #[test]
    fn the_radio_scripts_load() {
        let Some(w) = crate::play::with_data::world("El_Alamein") else { return };
        assert!(w.vfs.read("lexiconAll.dat").is_ok(), "lexicon");
        let p = load_file_script(&w.vfs, MENU_RADIO).expect("menu radio");
        assert_eq!(p.len(), 23);
        assert!(p[0][0].file_name().eq_ignore_ascii_case("RogerThat.wav"));
        assert!(p[22].len() == 2 && p[22].iter().any(|s| s.random_play), "Defend: one of two");
        let mut s = Sounds::default();
        let v = s.script_n(&w.vfs, &w.db, "GermanSoldier", VOICE).expect("voice");
        assert_eq!(v.len(), 22);
        assert!(v[20][0].file_name().eq_ignore_ascii_case("Medic.wav"));
    }

    #[test]
    fn the_voice_limit_keeps_the_highest_priority() {
        let mut heard: Vec<(u64, [f32; 2], f32, i32)> = (0..40).map(|i| (i as u64, [0.5, 0.5], 1.0, (i % 4) as i32)).collect();
        heard.push((99, [0.0, 0.0], 1.0, 50));
        limit_voices(&mut heard, VOICES);
        let on: Vec<&(u64, [f32; 2], f32, i32)> = heard.iter().filter(|h| h.1[0] > 0.0).collect();
        assert_eq!(on.len(), VOICES);
        assert_eq!(on.iter().filter(|h| h.3 == 0).count(), 2, "eight of the ten of priority 0 are not heard");
    }

    #[test]
    fn vehicles_and_soldiers_have_their_scripts() {
        let Some(w) = crate::play::with_data::world("El_Alamein") else { return };
        let mut s = Sounds::default();
        let mut parts = Vec::new();
        sound_parts(&w.db, "T34", &mut parts, 0);
        let kinds: Vec<&str> = parts.iter().map(|(_, k)| k.as_str()).collect();
        assert!(kinds.contains(&"engine") && kinds.contains(&"rotationalbundle") && kinds.contains(&"animatedbundle"), "{parts:?}");
        assert!(!parts.iter().any(|(n, _)| n.eq_ignore_ascii_case("T34GunBarrel")), "the cannon plays its shots");
        let engine = parts.iter().find(|(_, k)| k == "engine").unwrap();
        let p = s.script(&w.vfs, &w.db, &engine.0).expect("engine script");
        assert!(p[0].iter().any(|x| x.looped) && p[0].iter().any(|x| x.triggers.iter().any(|t| t.eq_ignore_ascii_case("release"))));
        // the soldier's: SoldierSound.inc's, from the `Common/Sounds` folder one level up; the voices by language
        let walk = s.script_n(&w.vfs, &w.db, "GermanSoldier", 1).expect("SoldierWalk.ssc");
        assert!(walk.len() >= 10, "{} step patches", walk.len());
        let killed = s.script_n(&w.vfs, &w.db, "GermanSoldier", KILLED).expect("SoldierKilled.ssc");
        assert!(killed[0].iter().any(|x| x.file_name().to_ascii_lowercase().starts_with("dying")));
        assert!(s.wav(&w.vfs, &killed[0][0], Some("German")).is_some());
        load_soldier_settings(&w.vfs, &mut s);
        assert_eq!(s.soldier.get("walkfrequency"), Some(&0.66));
        assert_eq!(s.material_patch.get(&10), Some(&0), "sand");
        // the level's ambience
        let env = w.vfs.read("bf1942/levels/El_Alamein/sounds/environment.con").expect("Environment.con");
        assert!(String::from_utf8_lossy(&env).contains("EnvironmentSound.load"));
        let lake = w.db.object_template("lake").expect("the lake");
        assert!(lake.kind.eq_ignore_ascii_case("AreaObject") && lake.all("addLinePoint").len() > 10);
        assert!(s.script(&w.vfs, &w.db, "lake").is_some());
    }

    /// An area object's update: a square of side 20 with `triggerRadius 5`, placed at
    /// (100, 7, 200): heard from the nearest point of its closed line (the side from the last
    /// point to the first too) at the object's height, up to 5 m off it; inside the square's box
    /// only farther than 5 m from every side, silent; past the box, silent.
    #[test]
    fn an_area_is_heard_within_its_trigger_radius() {
        let prop = |n: &str, a: &str| bytefield_con::templates::Property { name: n.into(), args: vec![a.into()] };
        let mut props = vec![prop("triggerradius", "5")];
        for q in ["-10/-10", "10/-10", "10/10", "-10/10"] {
            props.push(prop("addlinepoint", q));
        }
        let t = bytefield_con::templates::Template { namespace: "objecttemplate".into(), kind: "AreaObject".into(), name: "pond".into(), props, origin: String::new(), seq: 0, create_args: vec![] };
        let a = Area::new(&t, [100.0, 7.0, 200.0]);
        assert_eq!((a.lo, a.hi), ([-20.0, -20.0], [20.0, 20.0]));
        assert_eq!(a.heard([100.0, 50.0, 196.0]), None, "the middle, 6 m from the sides");
        assert_eq!(a.heard([88.0, 0.0, 200.0]), Some([90.0, 7.0, 200.0]), "the closing side");
        assert_eq!(a.heard([100.0, 0.0, 213.0]), Some([100.0, 7.0, 210.0]));
        assert_eq!(a.heard([100.0, 0.0, 216.0]), None);
        // two points are not an area
        let mut two = a.clone();
        two.line.truncate(2);
        assert_eq!(two.heard([88.0, 0.0, 200.0]), None);
    }

    /// Every sample of every template's sound scripts, by mod: how many wavs are found. Desert Combat keeps its samples in
    /// folders under `@RTD/` and none of them were found while only the bare name was looked up. Skips without the data.
    #[test]
    fn placed_objects_with_a_sound_script() {
        let game = bytefield_testdata::game();
        for (mod_name, level, want) in [("bf1942", "El_Alamein", None), ("bf1942", "Berlin", None), ("DC_Final", "DC_Al_Nas", Some("rivermid")), ("GCMOD", "GC_Endor", Some("ewoks1")), ("GCMOD", "GC_Lok", Some("earthquake"))] {
            let Ok(w) = bytefield_sim::World::load(&game, mod_name, level, "conquest") else { continue };
            let e = emitters(&w.db, &w.level);
            eprintln!("{level}: {} {:?}", e.len(), e.iter().map(|x| &x.0).collect::<std::collections::BTreeSet<_>>());
            match want {
                Some(t) => assert!(e.iter().any(|x| x.0.eq_ignore_ascii_case(t)), "{level}: no {t}"),
                None => assert!(e.is_empty(), "{level}"),
            }
        }
    }

    #[test]
    fn a_levels_own_samples_are_found() {
        let game = bytefield_testdata::game();
        for (level, path) in [("GC_Endor", "@ROOT/Sound/@RTD/ewoks1.wav"), ("GC_Lok", "@ROOT/Sound/earthquake.wav")] {
            let Ok(w) = bytefield_sim::World::load(&game, "GCMOD", level, "conquest") else { continue };
            let mut s = Sounds { level: level.into(), ..Default::default() };
            assert!(s.wav(&w.vfs, &Sample::new(path), None).is_some(), "{level}: {path}");
            assert!(Sounds::default().wav(&w.vfs, &Sample::new(path), None).is_none(), "{level}: {path} without the level");
        }
    }

    #[test]
    fn sound_scripts_find_their_samples() {
        for (mod_name, level, min) in [("bf1942", "El_Alamein", 0.95), ("DC_Final", "DC_Al_Nas", 0.9)] {
            let game = bytefield_testdata::game();
            if !game.join("Mods").join(mod_name).is_dir() {
                continue;
            }
            let Ok(w) = bytefield_sim::World::load(&game, mod_name, level, "conquest") else { continue };
            let mut s = Sounds::default();
            let (mut found, mut missing) = (0usize, std::collections::BTreeSet::new());
            let names: Vec<String> = w.db.templates.iter().filter(|((ns, _), t)| ns == "objecttemplate" && !t.all("loadSoundScript").is_empty()).map(|((_, n), _)| n.clone()).collect();
            for name in names {
                for n in 0..w.db.object_template(&name).map_or(0, |t| t.all("loadSoundScript").len()) {
                    let Some(p) = s.script_n(&w.vfs, &w.db, &name, n) else { continue };
                    for x in p.iter().flatten().filter(|x| !x.path.to_ascii_lowercase().contains("@language")) {
                        if s.wav(&w.vfs, x, None).is_some() {
                            found += 1;
                        } else {
                            missing.insert(x.rtd_path());
                        }
                    }
                }
            }
            let share = found as f32 / (found + missing.len()).max(1) as f32;
            println!("{mod_name}: {found} samples found, {} distinct missing ({:.1} %): {:?}", missing.len(), share * 100.0, missing.iter().take(40).collect::<Vec<_>>());
            assert!(share >= min, "{mod_name}: only {:.1} % of the samples found", share * 100.0);
        }
    }

    /// What a soldier's killed and hurt scripts resolve to, by mod: the language
    /// folder and each wav found, with its rate and length. Skips without the game's data.
    #[test]
    fn soldiers_killed_and_hurt_scripts_by_mod() {
        for (mod_name, level, soldiers) in [("bf1942", "El_Alamein", &["GermanSoldier", "USSoldier"][..]), ("DC_Final", "DC_Oil_Fields", &["IraqSoldier", "USSoldier"][..])] {
            let game = bytefield_testdata::game();
            if !game.join("Mods").join(mod_name).is_dir() {
                continue;
            }
            let Ok(w) = bytefield_sim::World::load(&game, mod_name, level, "conquest") else { continue };
            let mut s = Sounds::default();
            for sol in soldiers {
                let lang = w.db.object_template(sol).and_then(|t| t.get("radioLanguage")?.first().map(|l| l.trim_matches('"').to_string()));
                // how long the body stays (`CommonSoldierData.inc`: 10 s in the base game, 20 s in DC_Final)
                let ttl = w.db.object_template(sol).and_then(bytefield_sim::damage::ArmorTemplate::from_template).map(|a| a.time_to_live_after_death);
                println!("{mod_name} {sol} timeToLiveAfterDeath = {ttl:?}");
                assert_eq!(ttl, Some(if mod_name == "DC_Final" { 20.0 } else { 10.0 }), "{mod_name} {sol}");
                for (what, n) in [("hurt", HIT_DAMAGE), ("killed", KILLED)] {
                    let p = s.script_n(&w.vfs, &w.db, sol, n).unwrap_or_else(|| panic!("{mod_name} {sol} {what}: no script"));
                    let files: Vec<String> = p[0]
                        .iter()
                        .map(|x| format!("{}:{}", x.file_name(), s.wav(&w.vfs, x, lang.as_deref()).map_or("MISSING".into(), |a| format!("{}Hz/{:.1}s", a.rate, a.data.len() as f32 / a.channels.max(1) as f32 / a.rate as f32))))
                        .collect();
                    println!("{mod_name} {sol} lang={lang:?} {what} (script {n}): {} patches; {files:?}", p.len());
                    assert!(files.iter().all(|f| !f.ends_with("MISSING")), "{mod_name} {sol} {what}: {files:?}");
                }
            }
        }
    }
}
