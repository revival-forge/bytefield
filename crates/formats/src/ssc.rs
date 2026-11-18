//! Sound scripts (`.ssc`): what an object plays, read by `ObjectTemplate.loadSoundScript`.
//!
//! A script is a list of patches (`newPatch`); each patch layers samples (`load <wav>`) and each
//! sample carries its flags and effects (`beginEffect … endEffect`: a control source such as the
//! distance to the listener or the time since the trigger, mapped through an envelope onto the
//! volume or the pitch). Words are those of BF1942.exe's parser; what the engine does with them is
//! documented in the project notes.
//!
//! `#templateLevel HIGH|MEDIUM|LOW` picks a quality block (we keep HIGH; a script without levels is
//! one block) and `#include` splices another script, resolved by the caller relative to this one.

/// A whole script: its patches in order (objects address them by index).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SoundScript {
    pub patches: Vec<Patch>,
    /// Words the parser did not know (typos in the data, such as `oad`), for the sweeps.
    pub unknown: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Patch {
    pub samples: Vec<Sample>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// As written: `@ROOT/Sound/@RTD/k98LR.wav`.
    pub path: String,
    pub looped: bool,
    pub stereo: bool,
    pub doppler: bool,
    /// `stop FinishSample | Immediate | ImmediateReset`.
    pub stop: Option<String>,
    pub priority: i32,
    pub min_distance: f32,
    pub volume: f32,
    pub pan: f32,
    /// `randomStartPitch a / b`.
    pub random_start_pitch: (f32, f32),
    pub relative_position: [f32; 3],
    pub random_play: bool,
    /// `trigger Volume`: the controls that restart with each trigger.
    pub triggers: Vec<String>,
    pub groups: Vec<(String, String)>,
    pub effects: Vec<Effect>,
}

impl Sample {
    /// A sample of that path with the defaults of a `load` line.
    pub fn new(path: &str) -> Self {
        Sample {
            path: path.to_string(),
            looped: false,
            stereo: false,
            doppler: true,
            stop: None,
            priority: 0,
            min_distance: 1.0,
            volume: 1.0,
            pan: 0.0,
            random_start_pitch: (0.0, 0.0),
            relative_position: [0.0; 3],
            random_play: false,
            triggers: Vec::new(),
            groups: Vec::new(),
            effects: Vec::new(),
        }
    }

    /// The file name under `Sound/<rate>/`: `@ROOT/Sound/@RTD/k98LR.wav` -> `k98LR.wav`.
    pub fn file_name(&self) -> &str {
        self.path.rsplit(['/', '\\']).next().unwrap_or(&self.path)
    }

    /// The path under `Sound/<rate>/`, with the folders after `@RTD/` (Desert Combat keeps its samples in
    /// `@RTD/DesertCombat/M16/`) and without a leading `@Language/` (the caller puts the soldier's language
    /// there): `@ROOT/Sound/@RTD/DesertCombat/M16/M16_dist.wav` -> `DesertCombat/M16/M16_dist.wav`.
    pub fn rtd_path(&self) -> String {
        let p = self.path.replace('\\', "/");
        let lower = p.to_ascii_lowercase();
        let rest = match lower.find("@rtd/") {
            Some(i) => &p[i + 5..],
            None => self.file_name(),
        };
        let rest = if rest.to_ascii_lowercase().starts_with("@language/") { &rest[10..] } else { rest };
        rest.to_string()
    }

    /// Where the original looks for the wav, in order: `sound/<rate>/` of the mod with the folders after `@RTD/` (Desert Combat's),
    /// then the bare name; then the same under the level's own `bf1942/levels/<level>/`, and a path without `@RTD`
    /// (`@ROOT/Sound/earthquake.wav`, GC_Lok's) in its `sound/`. `sub`: the radio language's folder (`German/`) or empty.
    /// The level's folders: level 1 by their effect (recorded in the original, GC_Hoth's archive with a tone of its own in each
    /// folder: both play, and a level's copy of a wav the mod also has does not; it picked the `44khz` one).
    pub fn wav_paths(&self, sub: &str, level: Option<&str>) -> Vec<String> {
        let name = self.rtd_path();
        let rated: Vec<String> = [name.as_str(), self.file_name()].iter().flat_map(|n| ["44khz", "22khz", "11khz"].map(|r| format!("sound/{r}/{sub}{n}"))).collect();
        let mut out = rated.clone();
        if let Some(l) = level.filter(|l| !l.is_empty()) {
            out.extend(rated.iter().map(|p| format!("bf1942/levels/{l}/{p}")));
            let p = self.path.replace('\\', "/");
            if let Some(r) = p.to_ascii_lowercase().strip_prefix("@root/sound/").filter(|r| !r.starts_with("@rtd/")) {
                out.push(format!("bf1942/levels/{l}/sound/{}", &p[p.len() - r.len()..]));
            }
        }
        out
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Effect {
    /// `Volume`, `Pitch`.
    pub destination: String,
    /// `Distance`, `Time`, `TimeRelease`, `Default`, `Extern`, `Speed`, `Rpm`, …
    pub source: String,
    /// `#map<HeartBeat::Pitch>` after an `Extern` source.
    pub map: Option<String>,
    /// `Ramp`, `Linear`.
    pub envelope: String,
    pub params: Vec<f32>,
}

impl Effect {
    /// The ramp of the first four params: `x0 x1 y0 dy`, flat outside `[x0, x1]`
    /// (`30 50 1 -1`: full volume up to 30 m, silent from 50 m; `1.18 1.18 0 1`: silent for
    /// 1.18 s, then full). With six (the engines' pitch, `0 1.1 0.85 0.15 2 1.9`) we read the last
    /// two as a further point `(x2, y2)` the line goes on to, when `x2 > x1` (a guess: the rpm
    /// seldom passes 1.1, so it hardly matters).
    pub fn ramp(&self, x: f32) -> f32 {
        let p = |i: usize| self.params.get(i).copied().unwrap_or(0.0);
        let (x0, x1, y0, dy) = (p(0), p(1), p(2), p(3));
        if self.params.len() >= 6 && p(4) > x1 && x > x1 && x1 >= x0 {
            let t = ((x - x1) / (p(4) - x1)).min(1.0);
            return y0 + dy + (p(5) - y0 - dy) * t;
        }
        let t = if x1 > x0 {
            ((x - x0) / (x1 - x0)).clamp(0.0, 1.0)
        } else if x < x0 {
            0.0
        } else {
            1.0
        };
        y0 + dy * t
    }

    /// The envelope at `x`: `Linear` is `a + b·x` (`0.5 0.01` over the speed: the tracks' pitch;
    /// `0.9 0`: a fixed pitch), anything else the ramp.
    pub fn value(&self, x: f32) -> f32 {
        if self.envelope.eq_ignore_ascii_case("linear") {
            let p = |i: usize| self.params.get(i).copied().unwrap_or(0.0);
            return p(0) + p(1) * x;
        }
        self.ramp(x)
    }
}

/// Parses `text`; `include` returns the text of an `#include`d path (relative to this script).
pub fn parse(text: &str, include: &mut dyn FnMut(&str) -> Option<String>) -> SoundScript {
    let mut lines = Vec::new();
    flatten(text, include, &mut lines, 0);
    let mut out = SoundScript::default();
    let mut effect: Option<Effect> = None;
    let mut skipping = false;
    let mut in_comment = false;
    let mut in_map = false;
    for line in &lines {
        let line = line.trim();
        if in_comment {
            if line.contains("*/") {
                in_comment = false;
            }
            continue;
        }
        if line.starts_with("/*") {
            in_comment = !line.contains("*/");
            continue;
        }
        if line.is_empty() || line.starts_with("**") || line.starts_with("//") || line.starts_with(';') {
            continue;
        }
        let mut words = line.split_whitespace();
        let Some(word) = words.next() else { continue };
        let rest: Vec<&str> = words.collect();
        if skipping {
            skipping = word != "endSkip";
            continue;
        }
        if in_map {
            in_map = word != "#endMap";
            continue;
        }
        match word {
            "#beginMap" => {
                in_map = true;
                continue;
            }
            _ if word.starts_with('#') => continue,
            "beginSkip" => {
                skipping = true;
                continue;
            }
            "endSkip" => continue,
            _ => {}
        }
        if let Some(e) = effect.as_mut() {
            match word {
                "controlDestination" => e.destination = arg(&rest),
                "controlSource" => {
                    e.source = arg(&rest);
                    e.map = rest
                        .iter()
                        .find_map(|w| w.strip_prefix("#map<").map(|m| m.trim_end_matches('>').to_string()));
                }
                "envelope" => e.envelope = arg(&rest),
                "param" | "pram" => e.params.push(num(&rest)),
                "endEffect" | "endEffec" => {
                    let e = effect.take().unwrap();
                    if let Some(s) = last_sample(&mut out) {
                        s.effects.push(e);
                    }
                }
                _ => out.unknown.push(line.to_string()),
            }
            continue;
        }
        match word {
            "newPatch" => out.patches.push(Patch::default()),
            "load" => {
                if out.patches.is_empty() {
                    out.patches.push(Patch::default());
                }
                out.patches.last_mut().unwrap().samples.push(Sample::new(&arg(&rest)));
            }
            "beginEffect" => effect = Some(Effect::default()),
            _ => {
                let Some(s) = last_sample(&mut out) else {
                    out.unknown.push(line.to_string());
                    continue;
                };
                match word {
                    "loop" => s.looped = true,
                    "stereo" => s.stereo = true,
                    "dopplerOff" => s.doppler = false,
                    "stop" => s.stop = Some(arg(&rest)),
                    "priority" => s.priority = num(&rest) as i32,
                    "minDistance" => s.min_distance = num(&rest),
                    "volume" => s.volume = num(&rest),
                    "pan" => s.pan = num(&rest),
                    "randomStartPitch" => {
                        let v: Vec<f32> =
                            rest.iter().filter(|w| **w != "/").filter_map(|w| w.trim_matches('/').parse().ok()).collect();
                        s.random_start_pitch = (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0));
                    }
                    "relativePosition" => {
                        let v: Vec<f32> = arg(&rest).split('/').map(|x| x.parse().unwrap_or(0.0)).collect();
                        for (i, x) in v.iter().take(3).enumerate() {
                            s.relative_position[i] = *x;
                        }
                    }
                    "randomPlay" => s.random_play = rest.first().map_or(true, |w| *w != "0"),
                    "trigger" => s.triggers.push(arg(&rest)),
                    "addGroup" => s.groups.push((arg(&rest), rest.get(1).unwrap_or(&"").to_string())),
                    _ => out.unknown.push(line.to_string()),
                }
            }
        }
    }
    out
}

fn last_sample(s: &mut SoundScript) -> Option<&mut Sample> {
    s.patches.last_mut()?.samples.last_mut()
}

fn arg(rest: &[&str]) -> String {
    rest.first().map(|w| w.to_string()).unwrap_or_default()
}

fn num(rest: &[&str]) -> f32 {
    rest.first().and_then(|w| w.parse().ok()).unwrap_or(0.0)
}

/// Splices includes and keeps the HIGH block (or everything, outside of `#templateLevel`s).
fn flatten(text: &str, include: &mut dyn FnMut(&str) -> Option<String>, out: &mut Vec<String>, depth: usize) {
    let mut keep = true;
    for line in text.split(['\n', '\r']) {
        let t = line.trim();
        if let Some(level) = t.strip_prefix("#templateLevel") {
            keep = level.trim().eq_ignore_ascii_case("HIGH");
            continue;
        }
        if !keep {
            continue;
        }
        if let Some(path) = t.strip_prefix("#include") {
            if depth < 8 {
                if let Some(sub) = include(path.trim()) {
                    flatten(&sub, include, out, depth + 1);
                }
            }
            continue;
        }
        out.push(line.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIGH: &str = "newPatch\nload @ROOT/Sound/@RTD/shot.wav\nstereo\ndopplerOff\npriority 10\n\
        randomStartPitch 0.02 / 0.01\n*** Distance Volume ***\nbeginEffect\n\tcontrolDestination Volume\n\
        \tcontrolSource Distance\n\tenvelope Ramp\n\tparam 1\n\tparam 1\n\tparam 1\n\tparam -1\nendEffect\n\
        load @ROOT/Sound/@RTD/far.wav\nloop\nrelativePosition -.2/.4/.5\ntrigger Volume\n\
        #####\nnewPatch\nload @ROOT/Sound/@RTD/reload.wav\nstop FinishSample\noad typo.wav\n";

    #[test]
    fn rtd_path_keeps_the_folders() {
        assert_eq!(Sample::new("@ROOT/Sound/@RTD/DesertCombat/M16/M16_dist.wav").rtd_path(), "DesertCombat/M16/M16_dist.wav");
        assert_eq!(Sample::new("@ROOT/Sound/@RTD/k98LR.wav").rtd_path(), "k98LR.wav");
        assert_eq!(Sample::new("@ROOT\\Sound\\@RTD\\@Language\\Medic.wav").rtd_path(), "Medic.wav");
    }

    #[test]
    fn wav_paths_end_in_the_levels_folder() {
        let p = Sample::new("@ROOT/Sound/@RTD/ewoks1.wav").wav_paths("", Some("GC_Endor"));
        assert_eq!((p[0].as_str(), p.len()), ("sound/44khz/ewoks1.wav", 12));
        assert_eq!(p[6], "bf1942/levels/GC_Endor/sound/44khz/ewoks1.wav");
        let p = Sample::new("@ROOT/Sound/earthquake.wav").wav_paths("", Some("GC_Lok"));
        assert_eq!(p.last().unwrap(), "bf1942/levels/GC_Lok/sound/earthquake.wav");
        assert_eq!(Sample::new("@ROOT/Sound/@RTD/x.wav").wav_paths("German/", None)[1], "sound/22khz/German/x.wav");
    }

    #[test]
    fn levels_includes_and_samples() {
        let top = "#templateLevel HIGH\n\t#include High.ssc\n#templateLevel LOW\n\t#include Low.ssc";
        let mut asked = Vec::new();
        let s = parse(top, &mut |p| {
            asked.push(p.to_string());
            Some(HIGH.to_string())
        });
        assert_eq!(asked, ["High.ssc"]);
        assert_eq!(s.patches.len(), 2);
        let shot = &s.patches[0].samples[0];
        assert_eq!(shot.file_name(), "shot.wav");
        assert!(shot.stereo && !shot.doppler && !shot.looped);
        assert_eq!(shot.priority, 10);
        assert_eq!(shot.random_start_pitch, (0.02, 0.01));
        assert_eq!(shot.effects.len(), 1);
        assert_eq!(shot.effects[0].source, "Distance");
        assert_eq!(shot.effects[0].ramp(0.5), 1.0);
        assert_eq!(shot.effects[0].ramp(2.0), 0.0);
        let far = &s.patches[0].samples[1];
        assert!(far.looped);
        assert_eq!(far.relative_position, [-0.2, 0.4, 0.5]);
        assert_eq!(far.triggers, ["Volume"]);
        assert_eq!(s.patches[1].samples[0].stop.as_deref(), Some("FinishSample"));
        assert_eq!(s.unknown, ["oad typo.wav"]);
    }

    #[test]
    fn ramp_between() {
        let e = Effect { params: vec![30.0, 50.0, 1.0, -1.0], ..Default::default() };
        assert_eq!(e.ramp(10.0), 1.0);
        assert!((e.ramp(40.0) - 0.5).abs() < 1e-6);
        assert_eq!(e.ramp(60.0), 0.0);
    }

    #[test]
    fn linear_and_six_params() {
        let e = Effect { envelope: "Linear".into(), params: vec![0.5, 0.01], ..Default::default() };
        assert!((e.value(10.0) - 0.6).abs() < 1e-6);
        let e = Effect { envelope: "Ramp".into(), params: vec![0.0, 1.1, 0.85, 0.15, 2.0, 1.9], ..Default::default() };
        assert!((e.value(1.1) - 1.0).abs() < 1e-6);
        assert!((e.value(2.0) - 1.9).abs() < 1e-6);
    }
}
