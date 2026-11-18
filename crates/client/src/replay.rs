//! Repeatable tickets. A round of one's own is recorded from its first tick: what is held in each
//! tick (`play::held_now`'s, before the radio and the spawn screen take their keys, which they do again on a replay) and
//! the few things that reach the simulation from outside the keys (the spawn screen's choices, its «done», the console's
//! `suicide`), as ranges of ticks in `--script`'s format. Ctrl+F11 (`debug.rs`) leaves in the ticket `script.txt`,
//! `args.txt` (the command line, one argument a line), `env.txt` (the `BYTEFIELD_*` variables) and `replay.txt` (the tick,
//! the state's hash, as `examples/monkey`'s, and whether the first spawn waited). `bytefield --replay <ticket>` plays it
//! again with those, one tick a frame, and at that tick says whether the hash is the same (and exits).
//!
//! A round from the menu is recorded with an equivalent command line.
//! Not recorded (a replay then differs): a round on a server, ours or the original's;
//! the menu's options changed in the round; and what reads the clock (nothing in the simulation, `World` has no clock).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bevy::prelude::*;

use crate::play::{Game, Held, Segment};

/// What the round was started with: the command line (without `--script`, `--screenshot`, `--shot-tick`, `--replay`)
/// and whether the first spawn waited for the player. Set by `main` each time a round starts (the menu starts several).
static LAUNCH: Mutex<Option<(Vec<String>, bool)>> = Mutex::new(None);

/// (Two rounds in a row from the menu: the second's line replaces the first's, and its ticket replays the same, tried end to
/// end with `--menu create,start --steps "300:esc;330:disconnect;420:create;450:level+;480:start"`.)
pub fn set_launch(args: Vec<String>, choose: bool) {
    *LAUNCH.lock().unwrap() = Some((args, choose));
}

/// The menu's options, replaced by what it chose (`menu_argv`); `--menu` and `--no-intro` go, being the menu's own.
const MENU_ARGS: [&str; 5] = ["--mode", "--team", "--bots", "--friendly-fire", "--menu"];

/// A round chosen in the menu as the command line that starts it alone: the game's folder, the mod, the level, the mode,
/// the team, the bots and FRIENDLY FIRE, with the rest of the command line the menu was started with (`argv`, whose
/// first argument is the game's folder and the second, unless an option, the mod).
pub fn menu_argv(argv: &[String], mod_name: &str, level: &str, mode: &str, team: u32, bots: usize, friendly_fire: u32) -> Vec<String> {
    let mut out: Vec<String> = vec![argv.first().cloned().unwrap_or_default(), mod_name.into(), level.into()];
    out.extend(["--mode".into(), mode.into(), "--team".into(), team.to_string(), "--bots".into(), bots.to_string(), "--friendly-fire".into(), friendly_fire.to_string()]);
    let mut i = if argv.get(1).is_some_and(|a| !a.starts_with("--")) { 2 } else { 1 };
    while i < argv.len() {
        if MENU_ARGS.contains(&argv[i].as_str()) {
            i += 2;
            continue;
        }
        if argv[i] != "--no-intro" {
            out.push(argv[i].clone());
        }
        i += 1;
    }
    out
}

/// The arguments of a command line that make a check (they are replaced on a replay). `--steps` too, the menu's: a round
/// started from them kept them, and its replay pressed Esc and DISCONNECT in the round.
const CHECK_ARGS: [&str; 5] = ["--script", "--screenshot", "--shot-tick", "--replay", "--steps"];

/// The command line without the check's own arguments.
pub fn strip_check_args(a: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < a.len() {
        if CHECK_ARGS.contains(&a[i].as_str()) {
            i += 2;
            continue;
        }
        out.push(a[i].clone());
        i += 1;
    }
    out
}

/// The recording: the held keys and mouse in runs of equal ticks, and the events apart (one tick each).
#[derive(Default)]
pub struct Recording {
    held: Vec<Segment>,
    events: Vec<Segment>,
}

impl Recording {
    pub fn held(&mut self, tick: u64, h: &Held) {
        let mut keys: Vec<String> = h.keys.iter().cloned().collect();
        keys.sort();
        if let Some(last) = self.held.last_mut()
            && last.to == tick
            && last.keys == keys
            && last.mouse == h.mouse
        {
            last.to += 1;
            return;
        }
        if keys.is_empty() && h.mouse == Vec2::ZERO {
            return;
        }
        self.held.push(Segment { from: tick, to: tick + 1, keys, mouse: h.mouse });
    }

    pub fn event(&mut self, tick: u64, name: String) {
        self.events.push(Segment { from: tick, to: tick + 1, keys: vec![name], mouse: Vec2::ZERO });
    }

    /// In `--script`'s format (`play::parse_script`); the mouse as Rust prints an f32, which reads back to the same bits.
    pub fn script(&self) -> String {
        let mut parts = Vec::new();
        for s in self.held.iter().chain(&self.events) {
            let mut items = s.keys.clone();
            if s.mouse.x != 0.0 {
                items.push(format!("mx={}", s.mouse.x));
            }
            if s.mouse.y != 0.0 {
                items.push(format!("my={}", s.mouse.y));
            }
            parts.push(if s.to == s.from + 1 { format!("{}:{}", s.from, items.join(",")) } else { format!("{}-{}:{}", s.from, s.to, items.join(",")) });
        }
        parts.join(";")
    }
}

/// The world's state as one number: `monkey`'s (FNV-1a over the whole snapshot, encoded).
pub fn state_hash(sim: &bytefield_sim::Sim) -> u64 {
    let bytes = bytefield_sim::replica::Snapshot::of(sim).delta(None).encode();
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// The events in what a replay holds this tick: done as the spawn screen and the console did them.
pub fn apply_events(g: &mut Game, now: &Held) {
    for k in now.keys.iter() {
        if let Some(v) = k.strip_prefix("spawn=") {
            let Some((group, kit)) = v.split_once('/') else { continue };
            let group = if group == "-" { None } else { group.parse().ok() };
            if let Ok(kit) = kit.parse() {
                g.choose_spawn(group, kit);
            }
        } else if k == "release" {
            g.hold_spawn(false);
        } else if k == "suicide" {
            let p = g.player;
            g.session.sim.suicide(p);
        }
    }
}

/// Ctrl+F11's part: the recording and what the round was started with, in the ticket's folder. Not on a server (then a
/// `replay.txt` that says why).
pub fn write_ticket(dir: &Path, g: &Game) {
    let launch = LAUNCH.lock().unwrap().clone();
    let why_not = if g.net.is_some() || g.on_orig() {
        Some("on a server: not recorded")
    } else if launch.as_ref().is_none_or(|(a, _)| a.len() < 3 || a[0].starts_with("--")) {
        Some("no command line to repeat")
    } else {
        None
    };
    let res = (|| -> std::io::Result<()> {
        if let Some(w) = why_not {
            return std::fs::write(dir.join("replay.txt"), format!("not replayable: {w}\n"));
        }
        let (args, choose) = launch.as_ref().unwrap();
        std::fs::write(dir.join("script.txt"), g.recording.script())?;
        std::fs::write(dir.join("args.txt"), args.join("\n") + "\n")?;
        let env: Vec<String> = std::env::vars().filter(|(k, _)| k.starts_with("BYTEFIELD_") && k != "BYTEFIELD_MARK_TICK").map(|(k, v)| format!("{k}={v}")).collect();
        std::fs::write(dir.join("env.txt"), env.join("\n") + "\n")?;
        std::fs::write(dir.join("replay.txt"), format!("tick {}\nhash {:016x}\nchoose {}\ncommit {}\n", g.tick, state_hash(&g.session.sim), *choose as u8, crate::tickets::COMMIT))
    })();
    if let Err(e) = res {
        warn!("tickets: the replay: {e}");
    }
}

/// `--replay <ticket>`: the command line to play it again (its arguments, the recording as `--script`, `--choose` if
/// the first spawn waited) and the check (tick and hash). Sets the ticket's `BYTEFIELD_*` variables.
pub fn expand(dir: &Path) -> Result<(Vec<String>, Check), String> {
    let read = |n: &str| std::fs::read_to_string(dir.join(n)).map_err(|e| format!("--replay: {}: {e}", dir.join(n).display()));
    let rep = read("replay.txt")?;
    if let Some(w) = rep.strip_prefix("not replayable: ") {
        return Err(format!("--replay: the ticket is not replayable: {}", w.trim()));
    }
    let field = |k: &str| rep.lines().find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string())).ok_or(format!("--replay: no «{k}» in replay.txt"));
    let tick: u64 = field("tick ")?.parse().map_err(|_| "--replay: tick")?;
    let hash = u64::from_str_radix(&field("hash ")?, 16).map_err(|_| "--replay: hash")?;
    let choose = field("choose ")? == "1";
    for l in read("env.txt")?.lines().filter(|l| l.starts_with("BYTEFIELD_")) {
        if let Some((k, v)) = l.split_once('=') {
            // SAFETY: at the start of `main`, before any other thread
            unsafe { std::env::set_var(k, v) };
        }
    }
    let mut args: Vec<String> = read("args.txt")?.lines().filter(|l| !l.is_empty()).map(String::from).collect();
    args = strip_check_args(&args);
    args.retain(|a| a != "--choose");
    let script = read("script.txt")?;
    // (an empty recording: one tick of nothing, so that it is still a script and runs a tick a frame)
    args.extend(["--script".to_string(), if script.trim().is_empty() { "0:".to_string() } else { script }]);
    if choose {
        args.push("--choose".into());
    }
    Ok((args, Check { tick, hash, dir: dir.to_path_buf(), done: false }))
}

/// A replay's check: at `tick` the state's hash against the ticket's.
#[derive(Clone, Debug)]
pub struct Check {
    pub tick: u64,
    pub hash: u64,
    pub dir: PathBuf,
    pub done: bool,
}

/// After each frame's ticks: at the ticket's tick, the hash, a line in the log and `replay-result.txt`, and out.
pub fn check(mut g: ResMut<Game>, mut exit: MessageWriter<AppExit>) {
    let Some(c) = g.replay.clone().filter(|c| !c.done && g.tick >= c.tick) else { return };
    let h = state_hash(&g.session.sim);
    let same = g.tick == c.tick && h == c.hash;
    let line = format!(
        "REPLAY tick {} (ticket {}): hash {h:016x}, ticket {:016x}: {}",
        g.tick,
        c.tick,
        c.hash,
        if same { "SAME" } else { "DIFFERENT" }
    );
    if same {
        info!("{line}");
    } else {
        warn!("{line}");
    }
    let _ = std::fs::write(c.dir.join("replay-result.txt"), line + "\n");
    if let Some(r) = g.replay.as_mut() {
        r.done = true;
    }
    exit.write(if same { AppExit::Success } else { AppExit::from_code(3) });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A round from the menu: what it chose, in place of the menu's own options, with the rest of the command line.
    #[test]
    fn a_round_from_the_menu_as_a_command_line() {
        let argv: Vec<String> = ["data/game", "DC_Final", "--menu", "create,start", "--no-intro", "--bots", "4", "--kickback", "50"].map(String::from).to_vec();
        let a = menu_argv(&argv, "DC_Final", "DC_Al_Nas", "conquest", 2, 16, 75);
        assert_eq!(a, ["data/game", "DC_Final", "DC_Al_Nas", "--mode", "conquest", "--team", "2", "--bots", "16", "--friendly-fire", "75", "--kickback", "50"]);
        let b = menu_argv(&["data/game".to_string(), "--no-intro".to_string()], "bf1942", "Berlin", "coop", 1, 0, 100);
        assert_eq!(&b[..3], ["data/game", "bf1942", "Berlin"]);
        assert_eq!(b.len(), 11);
    }

    /// What is recorded reads back as the same held keys and mouse, tick by tick (the mouse to the bit).
    #[test]
    fn a_recording_reads_back_as_what_was_held() {
        let mut r = Recording::default();
        let mut ticks = Vec::new();
        for t in 0..40u64 {
            let mut h = Held::default();
            if (5..20).contains(&t) {
                h.keys.insert("w".into());
            }
            if t % 3 == 0 && t > 10 {
                h.keys.insert("lmb".into());
                h.mouse = Vec2::new(0.1 + t as f32 * 0.0137, -1.0 / 3.0);
            }
            r.held(t, &h);
            ticks.push(h);
        }
        r.event(12, "spawn=3/1".into());
        r.event(30, "release".into());
        let script = crate::play::parse_script(&r.script()).unwrap();
        for (t, h) in ticks.iter().enumerate() {
            let mut back = Held::default();
            for s in script.iter().filter(|s| (s.from..s.to).contains(&(t as u64))) {
                back.keys.extend(s.keys.iter().cloned());
                back.mouse += s.mouse;
            }
            back.keys.retain(|k| k != "spawn=3/1" && k != "release");
            assert_eq!(back.keys, h.keys, "tick {t}");
            assert_eq!(back.mouse.x.to_bits(), h.mouse.x.to_bits(), "tick {t}");
            assert_eq!(back.mouse.y.to_bits(), h.mouse.y.to_bits(), "tick {t}");
        }
        assert!(r.script().contains("12:spawn=3/1") && r.script().contains("30:release"));
    }

    #[test]
    fn a_check_s_own_arguments_are_left_out() {
        let a: Vec<String> = ["data/game", "bf1942", "Berlin", "--script", "0:w", "--bots", "4", "--screenshot", "x.png", "--steps", "300:esc"].map(String::from).to_vec();
        assert_eq!(strip_check_args(&a), ["data/game", "bf1942", "Berlin", "--bots", "4"]);
    }
}
