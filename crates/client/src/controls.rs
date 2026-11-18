//! The keys, as the player defines them (OPCIONES → CONTROLES). Level 2: the original's four
//! maps (`Settings/Default/Controls/{Common,Infantry,Land,Air}.con`), the common one always on and one by what the player
//! controls, with a key and a second key per action (`CONTROLS_DEFINE_KEY`, `CONTROLS_DEFINE_ALT_KEY`), a mouse sensitivity
//! and inversion per map and a keyboard sensitivity in the vehicles' two. The rows go in the original's page's order.
//! A device gives a *name* ("w", "lmb", "f9"…), the same names a script's steps use, and the game reads
//! only names: the map says which device gives which.
//!
//! The file is ours: `controls.cfg` beside `settings.cfg`, one `map.name=key,key` per action. With none, the first start
//! takes the original's own profile if there is one ([`Controls::from_profile`]).

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use bevy::prelude::*;

/// A device's button, or the wheel's turn, or a mouse axis (which only a default or a profile gives: no click defines it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    Key(KeyCode),
    Button(MouseButton),
    WheelUp,
    WheelDown,
    MouseAxis(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Map {
    Common,
    Infantry,
    Air,
    Land,
}

impl Map {
    /// In the order of the original's tabs.
    pub const ALL: [Map; 4] = [Map::Common, Map::Infantry, Map::Air, Map::Land];

    fn index(self) -> usize {
        Map::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    /// The tab's text in the lexicon.
    pub fn label(self) -> &'static str {
        match self {
            Map::Common => "CONTROLS_COMMON",
            Map::Infantry => "CONTROLS_INFANTRY",
            Map::Land => "CONTROLS_LANDSEA",
            Map::Air => "CONTROLS_AIR",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Map::Common => "common",
            Map::Infantry => "infantry",
            Map::Land => "land",
            Map::Air => "air",
        }
    }

    /// The name of its `.con` and, in `game.set<it>MouseSensitivity`, of its settings.
    fn con(self) -> (&'static str, &'static str) {
        match self {
            Map::Common => ("Common.con", "Common"),
            Map::Infantry => ("Infantry.con", "Inf"),
            Map::Land => ("Land.con", "LandSea"),
            Map::Air => ("Air.con", "Air"),
        }
    }

    /// Whether the page has a keyboard sensitivity and an inversion for it (the original's).
    pub fn has_keyboard(self) -> bool {
        matches!(self, Map::Air | Map::Land)
    }

    pub fn has_invert(self) -> bool {
        self != Map::Common
    }
}

/// An action of the menu: its text in the lexicon, the name it gives, its keys by default, and what the profiles call it
/// (`c_PIFire`; an axis's two ends, `c_PIThrottle+` for the first key of `addKeysToAxisMapping`).
struct Action(&'static str, &'static str, Option<Input>, Option<Input>, &'static str);

use KeyCode as K;
const fn k(c: KeyCode) -> Option<Input> {
    Some(Input::Key(c))
}
const LMB: Option<Input> = Some(Input::Button(MouseButton::Left));
const RMB: Option<Input> = Some(Input::Button(MouseButton::Right));
const AXIS_X: Option<Input> = Some(Input::MouseAxis(0));
const AXIS_Y: Option<Input> = Some(Input::MouseAxis(1));

// TODO: `c_PIToolTip` does nothing yet, and there is no map vote: their rows are there as in the original's page.
// The common map's mouse sensitivity (`setCommonMouseSensitivity`) has no effect here, on purpose: in the original it is the
// sensitivity of `defaultGameControlMap`'s mouse device, whose axes are `c_GIMouseLookX/Y`, the interface's own cursor; ours is the system's.
#[rustfmt::skip]
const COMMON: &[Action] = &[
    Action("CONTROLS_COMMON_ENTEREXIT", "e", k(K::KeyE), None, "c_PIUse"),
    Action("CONTROLS_COMMON_PARACHUTE", "9", k(K::Digit9), None, "c_PIMenuSelect9"),
    Action("CONTROLS_COMMON_CHAT", "k", k(K::KeyK), None, "c_PISayAll"),
    Action("CONTROLS_COMMON_TEAMCHAT", "l", k(K::KeyL), None, "c_PISayTeam"),
    Action("CONTROLS_COMMON_SHOW_SCOREBOARD", "tab", k(K::Tab), None, "c_PIShowScoreBoard"),
    Action("CONTROLS_COMMON_SHOW_SPAWNINTERFACE", "enter", k(K::Enter), None, "c_GIInGameMenu"),
    Action("CONTROLS_COMMON_SHOW_MAP", "m", k(K::KeyM), None, "c_PIMap"),
    Action("CONTROLS_COMMON_ZOOMMAP", "n", k(K::KeyN), None, "c_PIZoomMap"),
    Action("CONTROLS_COMMON_INSIDE", "f9", k(K::F9), None, "c_PICameraMode1"),
    Action("CONTROLS_COMMON_CHASE_REAR", "f10", k(K::F10), None, "c_PICameraMode2"),
    Action("CONTROLS_COMMON_CHASE_FRONT", "f11", k(K::F11), None, "c_PICameraMode3"),
    Action("CONTROLS_COMMON_FLY_BY", "f12", k(K::F12), None, "c_PICameraMode4"),
    Action("CONTROLS_COMMON_TOGGLE_CAMERA", "c", k(K::KeyC), None, "c_PIToggleCameraMode"),
    Action("CONTROLS_COMMON_PAUSE", "p", k(K::KeyP), None, "c_GITogglePause"),
    Action("CONTROLS_COMMON_SCREENSHOT", "print", k(K::PrintScreen), None, "c_PIScreenShot"),
    Action("CONTROLS_COMMON_TOOLTIP", "tooltip", k(K::KeyT), None, "c_PIToolTip"),
    Action("CONTROLS_COMMON_RADIO_1", "f1", k(K::F1), None, "c_PIRadio1"),
    Action("CONTROLS_COMMON_RADIO_2", "f2", k(K::F2), None, "c_PIRadio2"),
    Action("CONTROLS_COMMON_RADIO_3", "f3", k(K::F3), None, "c_PIRadio3"),
    Action("CONTROLS_COMMON_RADIO_4", "f4", k(K::F4), None, "c_PIRadio4"),
    Action("CONTROLS_COMMON_RADIO_5", "f5", k(K::F5), None, "c_PIRadio5"),
    Action("CONTROLS_COMMON_RADIO_6", "f6", k(K::F6), None, "c_PIRadio6"),
    Action("CONTROLS_COMMON_RADIO_7", "f7", k(K::F7), None, "c_PIRadio7"),
    Action("CONTROLS_COMMON_RADIO_8", "f8", k(K::F8), None, "c_PIRadio8"),
    Action("CONTROLS_COMMON_SHOWMAPVOTE", "mapvote", None, None, "c_PIMapVote"),
    Action("CONTROLS_COMMON_VOTEYES", "voteyes", None, None, "c_PIVoteYes"),
    Action("CONTROLS_COMMON_VOTENO", "voteno", None, None, "c_PIVoteNo"),
];

#[rustfmt::skip]
const INFANTRY: &[Action] = &[
    Action("CONTROLS_INFANTRY_FORWARD", "w", k(K::KeyW), None, "c_PIThrottle+"),
    Action("CONTROLS_INFANTRY_BACKWARD", "s", k(K::KeyS), None, "c_PIThrottle-"),
    Action("CONTROLS_INFANTRY_STRAFE_LEFT", "a", k(K::KeyA), None, "c_PIYaw-"),
    Action("CONTROLS_INFANTRY_STRAFE_RIGHT", "d", k(K::KeyD), None, "c_PIYaw+"),
    Action("CONTROLS_INFANTRY_JUMP", "space", k(K::Space), None, "c_PIAction"),
    // (Alt is ours)
    Action("CONTROLS_INFANTRY_WALK", "shift", k(K::ShiftLeft), k(K::AltLeft), "c_PIWalk"),
    Action("CONTROLS_INFANTRY_DROP", "g", k(K::KeyG), None, "c_PIDrop"),
    Action("CONTROLS_INFANTRY_FIRE", "lmb", LMB, None, "c_PIFire"),
    Action("CONTROLS_INFANTRY_ALT_FIRE", "rmb", RMB, None, "c_PIAltFire"),
    Action("CONTROLS_INFANTRY_RELOAD", "r", k(K::KeyR), None, "c_PIReload"),
    Action("CONTROLS_INFANTRY_NEXT_WEAPON", "wheelup", Some(Input::WheelUp), None, "c_PINextItem"),
    Action("CONTROLS_INFANTRY_PREV_WEAPON", "wheeldown", Some(Input::WheelDown), None, "c_PIPrevItem"),
    Action("CONTROLS_INFANTRY_WEAPON_1", "1", k(K::Digit1), None, "c_PIMenuSelect1"),
    Action("CONTROLS_INFANTRY_WEAPON_2", "2", k(K::Digit2), None, "c_PIMenuSelect2"),
    Action("CONTROLS_INFANTRY_WEAPON_3", "3", k(K::Digit3), None, "c_PIMenuSelect3"),
    Action("CONTROLS_INFANTRY_WEAPON_4", "4", k(K::Digit4), None, "c_PIMenuSelect4"),
    Action("CONTROLS_INFANTRY_WEAPON_5", "5", k(K::Digit5), None, "c_PIMenuSelect5"),
    Action("CONTROLS_INFANTRY_WEAPON_6", "6", k(K::Digit6), None, "c_PIMenuSelect6"),
    Action("CONTROLS_INFANTRY_CROUCH", "ctrl", k(K::ControlLeft), None, "c_PICrouch"),
    Action("CONTROLS_INFANTRY_PRONE", "z", k(K::KeyZ), None, "c_PILie"),
];

// (`c_PIPitch`'s plus is ↑, which dives in a plane and shuts the hatch or dives in a boat: the original's page says so,
// `DIVE / HATCH DOWN: UP ARROW`, `PITCH DOWN: UP ARROW`)
#[rustfmt::skip]
const LAND: &[Action] = &[
    Action("CONTROLS_LANDSEA_SPEEDUP", "w", k(K::KeyW), None, "c_PIThrottle+"),
    Action("CONTROLS_LANDSEA_SLOWDOWN", "s", k(K::KeyS), None, "c_PIThrottle-"),
    Action("CONTROLS_LANDSEA_TURNLEFT", "a", k(K::KeyA), None, "c_PIYaw-"),
    Action("CONTROLS_LANDSEA_TURNRIGHT", "d", k(K::KeyD), None, "c_PIYaw+"),
    Action("CONTROLS_LANDSEA_DOWN", "up", k(K::ArrowUp), None, "c_PIPitch+"),
    Action("CONTROLS_LANDSEA_UP", "down", k(K::ArrowDown), None, "c_PIPitch-"),
    Action("CONTROLS_LANDSEA_FIRE", "lmb", LMB, None, "c_PIFire"),
    Action("CONTROLS_LANDSEA_ALT_FIRE", "rmb", RMB, None, "c_PIAltFire"),
    Action("CONTROLS_LANDSEA_POS_1", "1", k(K::Digit1), None, "c_PIMenuSelect1"),
    Action("CONTROLS_LANDSEA_POS_2", "2", k(K::Digit2), None, "c_PIMenuSelect2"),
    Action("CONTROLS_LANDSEA_POS_3", "3", k(K::Digit3), None, "c_PIMenuSelect3"),
    Action("CONTROLS_LANDSEA_POS_4", "4", k(K::Digit4), None, "c_PIMenuSelect4"),
    Action("CONTROLS_LANDSEA_POS_5", "5", k(K::Digit5), None, "c_PIMenuSelect5"),
    Action("CONTROLS_LANDSEA_POS_6", "6", k(K::Digit6), None, "c_PIMenuSelect6"),
];

// (`Air.con` maps only 1–3; the lexicon has the texts of 4–6, spelt `CONTROL_`, and a bomber has the seats: they stay on)
#[rustfmt::skip]
const AIR: &[Action] = &[
    Action("CONTROLS_AIR_SPEEDUP", "w", k(K::KeyW), None, "c_PIThrottle+"),
    Action("CONTROLS_AIR_SLOWDOWN", "s", k(K::KeyS), None, "c_PIThrottle-"),
    Action("CONTROLS_AIR_STRAFE_LEFT", "a", k(K::KeyA), None, "c_PIYaw-"),
    Action("CONTROLS_AIR_STRAFE_RIGHT", "d", k(K::KeyD), None, "c_PIYaw+"),
    Action("CONTROLS_AIR_PITCH_UP", "down", AXIS_Y, k(K::ArrowDown), "c_PIPitch-"),
    Action("CONTROLS_AIR_PITCH_DOWN", "up", AXIS_Y, k(K::ArrowUp), "c_PIPitch+"),
    Action("CONTROLS_AIR_ROLL_LEFT", "left", AXIS_X, k(K::ArrowLeft), "c_PIRoll-"),
    Action("CONTROLS_AIR_ROLL_RIGHT", "right", AXIS_X, k(K::ArrowRight), "c_PIRoll+"),
    Action("CONTROLS_AIR_FIRE", "lmb", LMB, k(K::Space), "c_PIFire"),
    Action("CONTROLS_AIR_ALT_FIRE", "rmb", RMB, k(K::Numpad0), "c_PIAltFire"),
    Action("CONTROLS_AIR_TOGGLE_MOUSE", "shift", k(K::ShiftLeft), None, "c_PIMouseLook"),
    Action("CONTROLS_AIR_POS_1", "1", k(K::Digit1), None, "c_PIMenuSelect1"),
    Action("CONTROLS_AIR_POS_2", "2", k(K::Digit2), None, "c_PIMenuSelect2"),
    Action("CONTROLS_AIR_POS_3", "3", k(K::Digit3), None, "c_PIMenuSelect3"),
    Action("CONTROL_AIR_POS_4", "4", k(K::Digit4), None, "c_PIMenuSelect4"),
    Action("CONTROL_AIR_POS_5", "5", k(K::Digit5), None, "c_PIMenuSelect5"),
    Action("CONTROL_AIR_POS_6", "6", k(K::Digit6), None, "c_PIMenuSelect6"),
];

/// What 226's file gave another meaning to: the rows swapped or with the mouse's axis since, which an old file leaves as they come.
const CHANGED_IN_2: &[&str] = &["CONTROLS_AIR_PITCH_UP", "CONTROLS_AIR_PITCH_DOWN", "CONTROLS_AIR_ROLL_LEFT", "CONTROLS_AIR_ROLL_RIGHT", "CONTROLS_LANDSEA_UP", "CONTROLS_LANDSEA_DOWN"];

fn actions(m: Map) -> &'static [Action] {
    match m {
        Map::Common => COMMON,
        Map::Infantry => INFANTRY,
        Map::Land => LAND,
        Map::Air => AIR,
    }
}

/// What has no row in the menu and is always on: the original's outside camera, and our 7 and 8.
const FIXED: &[(KeyCode, &str)] = &[(K::Digit7, "7"), (K::Digit8, "8"), (K::Numpad2, "num2"), (K::Numpad4, "num4"), (K::Numpad6, "num6"), (K::Numpad8, "num8")];

/// What the wheel gives in a vehicle, with no row in the menu (`Land.con`; `wheel_keys` has its TODO).
const LAND_WHEEL: &[(Input, &str)] = &[(Input::WheelUp, "wheelup"), (Input::WheelDown, "wheeldown")];

/// The digits of a list on the screen (the radio's, the kits'), whatever the weapons' keys are.
const DIGITS: &[(KeyCode, &str)] = &[(K::Digit1, "1"), (K::Digit2, "2"), (K::Digit3, "3"), (K::Digit4, "4"), (K::Digit5, "5"), (K::Digit6, "6")];

/// The spawn screen's ← and →.
const ARROWS: &[(KeyCode, &str)] = &[(K::ArrowLeft, "left"), (K::ArrowRight, "right")];

/// What the player is at, for [`Controls::each`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Context {
    pub map: Map,
    /// a list is up that the digits pick from
    pub digits: bool,
    pub dead: bool,
}

/// The keys of each action, as `actions` lists them, and each map's mouse and keyboard.
#[derive(Clone, PartialEq, Debug)]
pub struct Controls {
    keys: [Vec<[Option<Input>; 2]>; 4],
    mouse: [f32; 4],
    keyboard: [f32; 4],
    invert: [bool; 4],
}

/// `Settings/Default/Controls`: `game.setInfMouseSensitivity 0.25`, `setAirMouseSensitivity 0.5`…
const MOUSE: [f32; 4] = [0.25, 0.25, 0.5, 0.25];
const KEYBOARD: f32 = 0.5;

/// The mouse's sensitivity whose gain is the one the game was tuned with (`MOUSE_UNITS`, `--sens 1`).
pub const MOUSE_NEUTRAL: f32 = 0.25;

impl Default for Controls {
    fn default() -> Self {
        Controls { keys: Map::ALL.map(|m| actions(m).iter().map(|a| [a.2, a.3]).collect()), mouse: MOUSE, keyboard: [KEYBOARD; 4], invert: Map::ALL.map(|m| m == Map::Air) }
    }
}

/// A row of the menu.
pub struct Row {
    pub label: &'static str,
    pub keys: [Option<Input>; 2],
}

/// Who lost a key to the one defined: its text in the lexicon.
pub type Conflict = Option<&'static str>;

impl Controls {
    /// A map as it comes.
    pub fn reset(&mut self, m: Map) {
        let d = Controls::default();
        let i = m.index();
        self.keys[i] = d.keys[i].clone();
        (self.mouse[i], self.keyboard[i], self.invert[i]) = (d.mouse[i], d.keyboard[i], d.invert[i]);
    }

    pub fn mouse(&self, m: Map) -> f32 {
        self.mouse[m.index()]
    }

    pub fn keyboard(&self, m: Map) -> f32 {
        self.keyboard[m.index()]
    }

    pub fn invert(&self, m: Map) -> bool {
        self.invert[m.index()]
    }

    pub fn set_mouse(&mut self, m: Map, v: f32) {
        self.mouse[m.index()] = v.clamp(0.0, 1.0);
    }

    pub fn set_keyboard(&mut self, m: Map, v: f32) {
        self.keyboard[m.index()] = v.clamp(0.0, 1.0);
    }

    pub fn set_invert(&mut self, m: Map, on: bool) {
        self.invert[m.index()] = on;
    }

    /// Whether the mouse's axis still gives this map something (a key put in its place takes it away).
    pub fn uses_axis(&self, m: Map, axis: u8) -> bool {
        self.keys[m.index()].iter().flatten().any(|i| *i == Some(Input::MouseAxis(axis)))
    }

    /// The old `--sens` and `--invert-y` (`settings.cfg`, before each map had its own), once.
    pub fn adopt_old_mouse(&mut self, sens: f32, invert: bool) {
        for m in [Map::Common, Map::Infantry, Map::Land, Map::Air] {
            let i = m.index();
            self.mouse[i] = (MOUSE[i] * sens).clamp(0.0, 1.0);
        }
        self.invert[Map::Infantry.index()] = invert;
        self.invert[Map::Land.index()] = invert;
    }

    pub fn rows(&self, m: Map) -> Vec<Row> {
        actions(m).iter().zip(&self.keys[m.index()]).map(|(a, k)| Row { label: a.0, keys: *k }).collect()
    }

    /// Whether this input can be defined here: Esc and Enter belong to the menus and the spawn screen, and the common
    /// map's actions are read as presses, which a wheel's turn is not.
    pub fn can_define(m: Map, i: Input) -> bool {
        match i {
            Input::Key(K::Escape | K::Enter | K::Backquote) | Input::MouseAxis(_) => false,
            Input::WheelUp | Input::WheelDown => m != Map::Common,
            Input::Key(k) => key_name(k).is_some(),
            Input::Button(b) => button_name(b).is_some(),
        }
    }

    /// Gives `input` to an action. An input does one thing in the maps that are on together (the common one and the one
    /// in use), as the original warns (`CONTROLS_CONFLICT_DETECTED`): the one that had it loses it, and is told.
    pub fn define(&mut self, m: Map, row: usize, slot: usize, input: Input) -> Conflict {
        let mut conflict = None;
        for other in Map::ALL {
            if other != m && other != Map::Common && m != Map::Common {
                continue;
            }
            for (j, keys) in self.keys[other.index()].iter_mut().enumerate() {
                let same_action = other == m && j == row;
                for (s, key) in keys.iter_mut().enumerate() {
                    if *key == Some(input) && !(same_action && s == slot) {
                        *key = None;
                        if !same_action {
                            conflict = Some(actions(other)[j].0);
                        }
                    }
                }
            }
        }
        if let Some(keys) = self.keys[m.index()].get_mut(row) {
            keys[slot] = Some(input);
        }
        conflict
    }

    /// Every input that is on, with the name it gives.
    pub fn each(&self, c: Context, mut f: impl FnMut(Input, &'static str)) {
        for m in [Map::Common, c.map] {
            for (a, keys) in actions(m).iter().zip(&self.keys[m.index()]) {
                for i in keys.iter().flatten().filter(|i| !matches!(i, Input::MouseAxis(_))) {
                    f(*i, a.1);
                }
            }
        }
        for (k, n) in FIXED {
            f(Input::Key(*k), n);
        }
        if c.map == Map::Land {
            for (i, n) in LAND_WHEEL {
                f(*i, n);
            }
        }
        if c.digits {
            for (k, n) in DIGITS {
                f(Input::Key(*k), n);
            }
        }
        if c.dead {
            for (k, n) in ARROWS {
                f(Input::Key(*k), n);
            }
        }
    }

    /// Whether an action of the common map has just been pressed.
    pub fn just(&self, keys: &ButtonInput<KeyCode>, buttons: &ButtonInput<MouseButton>, name: &str) -> bool {
        let Some(row) = COMMON.iter().position(|a| a.1 == name) else { return false };
        self.keys[Map::Common.index()][row].iter().flatten().any(|i| match i {
            Input::Key(k) => keys.just_pressed(*k),
            Input::Button(b) => buttons.just_pressed(*b),
            _ => false,
        })
    }

    pub fn write(&self) -> String {
        let mut out = String::from("# bytefield\nversion=2\n");
        for m in Map::ALL {
            out += &format!("{0}.mouse={1}\n{0}.keyboard={2}\n{0}.invert={3}\n", m.file_name(), self.mouse(m), self.keyboard(m), self.invert(m) as u8);
            for (a, keys) in actions(m).iter().zip(&self.keys[m.index()]) {
                let names: Vec<&str> = keys.iter().flatten().filter_map(|i| input_name(*i)).collect();
                out += &format!("{}.{}={}\n", m.file_name(), a.0, names.join(","));
            }
        }
        out
    }

    /// Reads what [`write`](Self::write) wrote; what is unknown or broken stays as by default.
    pub fn parse(text: &str) -> Controls {
        let mut c = Controls::default();
        let current = text.lines().any(|l| l.trim() == "version=2");
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let Some((map, label)) = k.trim().split_once('.') else { continue };
            let Some(m) = Map::ALL.into_iter().find(|m| m.file_name() == map) else { continue };
            let number = v.trim().parse::<f32>().ok().filter(|x| x.is_finite());
            match (label, number) {
                ("mouse", Some(x)) => c.set_mouse(m, x),
                ("keyboard", Some(x)) => c.set_keyboard(m, x),
                ("invert", Some(x)) => c.set_invert(m, x != 0.0),
                _ => {}
            }
            if !current && CHANGED_IN_2.contains(&label) {
                continue;
            }
            let Some(row) = actions(m).iter().position(|a| a.0 == label) else { continue };
            let mut keys = [None, None];
            let names: Vec<&str> = v.split(',').map(str::trim).filter(|n| !n.is_empty()).collect();
            let inputs: Vec<Option<Input>> = names.iter().map(|n| input_of(n)).collect();
            if inputs.iter().any(|i| i.is_none_or(|i| !Controls::can_define(m, i) && !fixed_default(m, row, i))) {
                continue;
            }
            for (slot, i) in inputs.into_iter().take(2).enumerate() {
                keys[slot] = i;
            }
            c.keys[m.index()][row] = keys;
        }
        c
    }

    /// A profile of the original (`Settings/Profiles/Custom/Controls`, whose four `.con` it rewrites when saving, or
    /// `Settings/Default/Controls`). What it does not map stays empty, as there; what only ours has comes back where free.
    pub fn from_profile(dir: &Path) -> Option<Controls> {
        let mut c = Controls::default();
        let mut read = 0;
        for m in Map::ALL {
            let Some(text) = read_ci(dir, m.con().0) else { continue };
            read += 1;
            c.keys[m.index()] = actions(m).iter().map(|_| [None, None]).collect();
            c.take_con(m, &text);
        }
        if read < Map::ALL.len() {
            return None;
        }
        // (ours, not the original's: `AIR`'s 4–6)
        for (row, a) in AIR.iter().enumerate().skip(14) {
            if c.keys[Map::Air.index()][row] == [None, None] && a.2.is_some_and(|i| !c.taken(Map::Air, i)) {
                c.keys[Map::Air.index()][row] = [a.2, None];
            }
        }
        Some(c)
    }

    fn taken(&self, m: Map, input: Input) -> bool {
        [m, Map::Common].iter().any(|m| self.keys[m.index()].iter().flatten().any(|k| *k == Some(input)))
    }

    /// One `.con`'s lines. A mapping's last word, `1`, puts it in the second column; a file names a map's settings by
    /// its own name, and the common one's lines are in `Common.con`.
    fn take_con(&mut self, m: Map, text: &str) {
        let device = |id: &str, button: bool| if button { profile_button(id) } else { profile_key(id) };
        for line in text.lines() {
            let w: Vec<&str> = line.split_whitespace().collect();
            let Some(&command) = w.first() else { continue };
            if let Some(setting) = command.strip_prefix("game.set") {
                let number = w.get(1).and_then(|v| v.parse::<f32>().ok()).filter(|x| x.is_finite());
                let name = m.con().1;
                match (setting.strip_prefix(name), number) {
                    (Some("MouseSensitivity"), Some(x)) => self.set_mouse(m, x),
                    (Some("KeyboardSensitivity"), Some(x)) => self.set_keyboard(m, x),
                    (Some("MouseInvert"), Some(x)) => self.set_invert(m, x != 0.0),
                    _ => {}
                }
                continue;
            }
            let Some(mapping) = command.strip_prefix("ControlMap.") else { continue };
            let slot = (w.last() == Some(&"1") && w.len() > 4) as usize;
            match mapping {
                "addKeyToTriggerMapping" | "addButtonToTriggerMapping" if w.len() >= 4 => {
                    if let Some(i) = device(w[3], mapping.starts_with("addButton")) {
                        self.put(m, w[1], slot, i);
                    }
                }
                "addKeysToAxisMapping" | "addButtonsToAxisMapping" if w.len() >= 5 => {
                    let button = mapping.starts_with("addButtons");
                    for (end, id) in [("+", w[3]), ("-", w[4])] {
                        if let Some(i) = device(id, button) {
                            self.put(m, &format!("{}{end}", w[1]), slot, i);
                        }
                    }
                }
                "addAxisToAxisMapping" if w.len() >= 4 && w[2] == "IDFMouse" => {
                    if let Some(n) = w[3].strip_prefix("IDAxis_").and_then(|n| n.parse().ok()) {
                        for end in ["+", "-"] {
                            self.put(m, &format!("{}{end}", w[1]), 0, Input::MouseAxis(n));
                        }
                    }
                }
                "addAxisToTriggerMapping" if w.len() >= 5 && w[3] == "IDFMouse" && w[4] == "IDAxis_2" => {
                    self.put(m, w[1], 0, Input::WheelUp);
                    self.put(m, w[2], 0, Input::WheelDown);
                }
                _ => {}
            }
        }
    }

    fn put(&mut self, m: Map, pi: &str, slot: usize, input: Input) {
        if let Some(row) = actions(m).iter().position(|a| a.4 == pi) {
            self.keys[m.index()][row][slot] = Some(input);
        }
    }

    pub fn save(&self) {
        if crate::settings::READ_ONLY.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let Some(p) = path() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&p, self.write()) {
            warn!("{}: {e}", p.display());
        }
    }
}

/// Whether a row's default is one that nobody can define, as its file then has it (Enter, a mouse's axis).
fn fixed_default(m: Map, row: usize, i: Input) -> bool {
    actions(m).get(row).is_some_and(|a| a.2 == Some(i) || a.3 == Some(i))
}

/// A file of `dir`, whatever its name's case (the game's files come from Windows).
fn read_ci(dir: &Path, name: &str) -> Option<String> {
    let entry = std::fs::read_dir(dir).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))?;
    std::fs::read_to_string(entry.path()).ok()
}

static INSTALL: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Where the game is, for its profile: before the first [`current`].
pub fn set_install(game: &Path) {
    if let Ok(mut now) = INSTALL.write() {
        *now = Some(game.to_path_buf());
    }
}

/// The original's own profile, if the install has one.
fn installed_profile() -> Option<Controls> {
    let game = INSTALL.read().ok()?.clone()?;
    let dir = bytefield_con::session::resolve_mod_path(&game, "Mods/bf1942/Settings/Profiles/Custom/Controls")?;
    let c = Controls::from_profile(&dir)?;
    info!("controls: the original's profile, {}", dir.display());
    Some(c)
}

/// `controls.cfg` beside the settings' file (their name with `.controls` for one named by `BYTEFIELD_CONFIG`).
fn path() -> Option<PathBuf> {
    let settings = crate::settings::path()?;
    if std::env::var_os("BYTEFIELD_CONFIG").is_some() {
        return Some(settings.with_extension("controls"));
    }
    Some(settings.with_file_name("controls.cfg"))
}

/// Whether the file has each map's mouse yet (before, one `sens` for all in `settings.cfg`).
pub fn has_own_mouse() -> bool {
    path().and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|t| t.contains(".mouse="))
}

static CURRENT: RwLock<Option<Controls>> = RwLock::new(None);

/// The keys in use, read from the file the first time.
pub fn current() -> Controls {
    if let Some(c) = CURRENT.read().ok().and_then(|c| c.clone()) {
        return c;
    }
    let saved = path().and_then(|p| std::fs::read_to_string(p).ok());
    let c = saved.map(|t| Controls::parse(&t)).or_else(installed_profile).unwrap_or_default();
    set(c.clone());
    c
}

pub fn set(c: Controls) {
    if let Ok(mut now) = CURRENT.write() {
        *now = Some(c);
    }
}

fn button_name(b: MouseButton) -> Option<&'static str> {
    Some(match b {
        MouseButton::Left => "Mouse1",
        MouseButton::Right => "Mouse2",
        MouseButton::Middle => "Mouse3",
        MouseButton::Back => "Mouse4",
        MouseButton::Forward => "Mouse5",
        _ => return None,
    })
}

/// A key's name in the original (`IDKey_LeftShift`), which is also the lexicon's key of the ones with a text.
fn key_id(k: KeyCode) -> Option<String> {
    let special = match k {
        K::ShiftLeft => "LeftShift",
        K::ShiftRight => "RightShift",
        K::ControlLeft => "LeftCtrl",
        K::ControlRight => "RightCtrl",
        K::AltLeft => "LeftAlt",
        K::AltRight => "RightAlt",
        K::ArrowUp => "ArrowUp",
        K::ArrowDown => "ArrowDown",
        K::ArrowLeft => "ArrowLeft",
        K::ArrowRight => "ArrowRight",
        K::NumpadAdd => "Add",
        K::NumpadSubtract => "Subtract",
        K::NumpadMultiply => "Multiply",
        K::NumpadDivide => "Divide",
        K::NumpadDecimal => "Decimal",
        K::NumpadEnter => "NumpadEnter",
        K::Enter => "Enter",
        K::CapsLock => "Capital",
        K::Minus => "Minus",
        K::Equal => "Equals",
        K::BracketLeft => "LeftBracket",
        K::BracketRight => "RightBracket",
        K::Backslash => "Backslash",
        K::Semicolon => "Semicolon",
        K::Quote => "Apostrophe",
        K::Comma => "Comma",
        K::Period => "Period",
        K::Slash => "Slash",
        K::IntlBackslash => "OEM_102",
        _ => "",
    };
    if !special.is_empty() {
        return Some(format!("IDKey_{special}"));
    }
    // (the rest have our name but for the numeric pad's: `Num4`, `IDKey_Numpad4`)
    let name = key_name(k)?;
    Some(match name.strip_prefix("Num") {
        Some(d) if d.len() == 1 => format!("IDKey_Numpad{d}"),
        _ => format!("IDKey_{name}"),
    })
}

fn profile_key(id: &str) -> Option<Input> {
    KEY_NAMES.iter().find(|(k, _)| key_id(*k).as_deref() == Some(id)).map(|(k, _)| Input::Key(*k))
}

fn profile_button(id: &str) -> Option<Input> {
    let n: usize = id.strip_prefix("IDButton_")?.parse().ok()?;
    BUTTONS.get(n).map(|b| Input::Button(*b))
}

/// An input's text in the menu: its entry in the lexicon, for what the lexicon has (`IDKey_LeftShift`, `Mouse_IDButton_0`,
/// `MWheelUp`, in each language), else its name, as a letter's or a number's.
pub fn lexicon_key(i: Input) -> Option<String> {
    Some(match i {
        Input::Button(b) => format!("Mouse_IDButton_{}", BUTTONS.iter().position(|x| *x == b)?),
        Input::WheelUp => "MWheelUp".to_string(),
        Input::WheelDown => "MWheelDown".to_string(),
        Input::MouseAxis(n) => format!("Mouse_IDAxis_{n}"),
        Input::Key(k) => key_id(k)?,
    })
}

const BUTTONS: [MouseButton; 5] = [MouseButton::Left, MouseButton::Right, MouseButton::Middle, MouseButton::Back, MouseButton::Forward];

/// The keys that can be defined, with their names (the file's and the menu's).
#[rustfmt::skip]
pub const KEY_NAMES: &[(KeyCode, &str)] = &[
    (K::KeyA, "A"), (K::KeyB, "B"), (K::KeyC, "C"), (K::KeyD, "D"), (K::KeyE, "E"), (K::KeyF, "F"), (K::KeyG, "G"), (K::KeyH, "H"), (K::KeyI, "I"),
    (K::KeyJ, "J"), (K::KeyK, "K"), (K::KeyL, "L"), (K::KeyM, "M"), (K::KeyN, "N"), (K::KeyO, "O"), (K::KeyP, "P"), (K::KeyQ, "Q"), (K::KeyR, "R"),
    (K::KeyS, "S"), (K::KeyT, "T"), (K::KeyU, "U"), (K::KeyV, "V"), (K::KeyW, "W"), (K::KeyX, "X"), (K::KeyY, "Y"), (K::KeyZ, "Z"),
    (K::Digit0, "0"), (K::Digit1, "1"), (K::Digit2, "2"), (K::Digit3, "3"), (K::Digit4, "4"), (K::Digit5, "5"), (K::Digit6, "6"), (K::Digit7, "7"), (K::Digit8, "8"), (K::Digit9, "9"),
    (K::F1, "F1"), (K::F2, "F2"), (K::F3, "F3"), (K::F4, "F4"), (K::F5, "F5"), (K::F6, "F6"), (K::F7, "F7"), (K::F8, "F8"), (K::F9, "F9"), (K::F10, "F10"), (K::F11, "F11"), (K::F12, "F12"),
    (K::Numpad0, "Num0"), (K::Numpad1, "Num1"), (K::Numpad2, "Num2"), (K::Numpad3, "Num3"), (K::Numpad4, "Num4"), (K::Numpad5, "Num5"), (K::Numpad6, "Num6"), (K::Numpad7, "Num7"), (K::Numpad8, "Num8"), (K::Numpad9, "Num9"),
    (K::NumpadAdd, "Num+"), (K::NumpadSubtract, "Num-"), (K::NumpadMultiply, "Num*"), (K::NumpadDivide, "Num/"), (K::NumpadDecimal, "Num."), (K::NumpadEnter, "NumEnter"),
    (K::Enter, "Enter"), (K::Space, "Space"), (K::Tab, "Tab"), (K::Backspace, "Backspace"), (K::CapsLock, "CapsLock"), (K::PrintScreen, "PrintScreen"), (K::ScrollLock, "ScrollLock"), (K::Pause, "Pause"),
    (K::ShiftLeft, "LShift"), (K::ShiftRight, "RShift"), (K::ControlLeft, "LCtrl"), (K::ControlRight, "RCtrl"), (K::AltLeft, "LAlt"), (K::AltRight, "RAlt"),
    (K::ArrowUp, "Up"), (K::ArrowDown, "Down"), (K::ArrowLeft, "Left"), (K::ArrowRight, "Right"),
    (K::Insert, "Insert"), (K::Delete, "Delete"), (K::Home, "Home"), (K::End, "End"), (K::PageUp, "PageUp"), (K::PageDown, "PageDown"),
    (K::Minus, "-"), (K::Equal, "="), (K::BracketLeft, "["), (K::BracketRight, "]"), (K::Backslash, "\\"), (K::Semicolon, ";"), (K::Quote, "'"), (K::Comma, ","), (K::Period, "."), (K::Slash, "/"), (K::IntlBackslash, "<"),
];

fn key_name(k: KeyCode) -> Option<&'static str> {
    KEY_NAMES.iter().find(|(c, _)| *c == k).map(|(_, n)| *n)
}

/// An input's name, in the file and in the menu.
pub fn input_name(i: Input) -> Option<&'static str> {
    match i {
        Input::Key(k) => key_name(k),
        Input::Button(b) => button_name(b),
        Input::WheelUp => Some("WheelUp"),
        Input::WheelDown => Some("WheelDown"),
        Input::MouseAxis(0) => Some("MouseAxis0"),
        Input::MouseAxis(1) => Some("MouseAxis1"),
        Input::MouseAxis(_) => None,
    }
}

fn input_of(name: &str) -> Option<Input> {
    match name {
        "WheelUp" => return Some(Input::WheelUp),
        "WheelDown" => return Some(Input::WheelDown),
        "MouseAxis0" => return Some(Input::MouseAxis(0)),
        "MouseAxis1" => return Some(Input::MouseAxis(1)),
        _ => {}
    }
    KEY_NAMES.iter().find(|(_, n)| *n == name).map(|(k, _)| Input::Key(*k)).or_else(|| BUTTONS.into_iter().find(|b| button_name(*b) == Some(name)).map(Input::Button))
}

/// The input pressed in this frame that could be defined, if any: what the menu waits for after a click on a row.
pub fn pressed_now(keys: &ButtonInput<KeyCode>, buttons: &ButtonInput<MouseButton>, wheel: f32) -> Option<Input> {
    KEY_NAMES.iter().find(|(k, _)| keys.just_pressed(*k)).map(|(k, _)| Input::Key(*k)).or_else(|| BUTTONS.into_iter().find(|b| buttons.just_pressed(*b)).map(Input::Button)).or(if wheel > 0.0 {
        Some(Input::WheelUp)
    } else if wheel < 0.0 {
        Some(Input::WheelDown)
    } else {
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn given(c: &Controls, ctx: Context, name: &str) -> Vec<Input> {
        let mut v = Vec::new();
        c.each(ctx, |i, n| {
            if n == name {
                v.push(i);
            }
        });
        v
    }

    const FOOT: Context = Context { map: Map::Infantry, digits: false, dead: false };
    const PLANE: Context = Context { map: Map::Air, digits: false, dead: false };

    #[test]
    fn a_key_moves_to_the_last_action_given_it() {
        let mut c = Controls::default();
        let reload = INFANTRY.iter().position(|a| a.1 == "r").unwrap();
        // (W is the forward's: it loses it, and says so)
        assert_eq!(c.define(Map::Infantry, reload, 0, Input::Key(K::KeyW)), Some("CONTROLS_INFANTRY_FORWARD"));
        assert_eq!(given(&c, FOOT, "w"), vec![]);
        assert_eq!(given(&c, FOOT, "r"), vec![Input::Key(K::KeyW)]);
        // (the other maps keep theirs: W still flies)
        assert_eq!(given(&c, PLANE, "w"), vec![Input::Key(K::KeyW)]);
    }

    #[test]
    fn the_common_map_is_on_top_of_all() {
        let mut c = Controls::default();
        let reload = INFANTRY.iter().position(|a| a.1 == "r").unwrap();
        assert_eq!(c.define(Map::Infantry, reload, 1, Input::Key(K::KeyE)), Some("CONTROLS_COMMON_ENTEREXIT"));
        let enter = COMMON.iter().position(|a| a.1 == "e").unwrap();
        assert_eq!(c.define(Map::Common, enter, 0, Input::Key(K::KeyW)), Some("CONTROLS_LANDSEA_SPEEDUP"));
        assert_eq!(given(&c, PLANE, "w"), vec![]);
        assert_eq!(given(&c, FOOT, "w"), vec![]);
    }

    #[test]
    fn its_own_key_again_is_no_conflict() {
        let mut c = Controls::default();
        assert_eq!(c.define(Map::Infantry, 0, 0, Input::Key(K::KeyW)), None);
        assert_eq!(c, Controls::default());
        // (the same key in its other slot moves)
        assert_eq!(c.define(Map::Infantry, 0, 1, Input::Key(K::KeyW)), None);
        assert_eq!(c.rows(Map::Infantry)[0].keys, [None, Some(Input::Key(K::KeyW))]);
    }

    #[test]
    fn the_file_there_and_back() {
        assert_eq!(Controls::parse(&Controls::default().write()), Controls::default());
        let mut c = Controls::default();
        c.define(Map::Air, 8, 1, Input::Button(MouseButton::Middle));
        c.set_mouse(Map::Land, 0.8);
        c.set_keyboard(Map::Air, 0.1);
        c.set_invert(Map::Infantry, true);
        c.define(Map::Infantry, 12, 0, Input::Key(K::KeyQ));
        c.define(Map::Land, 3, 1, Input::WheelUp);
        assert_eq!(Controls::parse(&c.write()), c);
    }

    #[test]
    fn a_broken_line_stays_as_by_default() {
        let c = Controls::parse("nope\ninfantry.CONTROLS_INFANTRY_FORWARD=NoSuchKey\ninfantry.NOPE=W\ncommon.CONTROLS_COMMON_CHAT=WheelUp\ninfantry.CONTROLS_INFANTRY_RELOAD=Escape\n");
        assert_eq!(c, Controls::default());
    }

    #[test]
    fn some_inputs_are_not_for_defining() {
        assert!(!Controls::can_define(Map::Infantry, Input::Key(K::Escape)));
        assert!(!Controls::can_define(Map::Infantry, Input::Key(K::Enter)));
        assert!(!Controls::can_define(Map::Common, Input::WheelUp));
        assert!(Controls::can_define(Map::Infantry, Input::WheelUp));
        assert!(Controls::can_define(Map::Air, Input::Button(MouseButton::Middle)));
    }

    #[test]
    fn the_lists_on_the_screen_keep_their_keys() {
        let mut c = Controls::default();
        c.define(Map::Infantry, INFANTRY.iter().position(|a| a.1 == "1").unwrap(), 0, Input::Key(K::KeyQ));
        assert_eq!(given(&c, FOOT, "1"), vec![Input::Key(K::KeyQ)]);
        assert_eq!(given(&c, Context { digits: true, ..FOOT }, "1"), vec![Input::Key(K::KeyQ), Input::Key(K::Digit1)]);
        assert_eq!(given(&c, Context { dead: true, ..FOOT }, "right"), vec![Input::Key(K::ArrowRight)]);
        assert_eq!(given(&c, FOOT, "right"), vec![]);
    }

    #[test]
    fn a_file_of_226_keeps_the_arrows_and_the_mouse_of_a_plane() {
        // (226 had `PITCH_UP` on ↑, and no mouse's axis in the rows)
        let c = Controls::parse("air.CONTROLS_AIR_PITCH_UP=Up\nair.CONTROLS_AIR_ROLL_LEFT=Left\ninfantry.CONTROLS_INFANTRY_RELOAD=Q\n");
        assert_eq!(given(&c, PLANE, "up"), vec![Input::Key(K::ArrowUp)]);
        assert!(c.uses_axis(Map::Air, 0) && c.uses_axis(Map::Air, 1));
        assert_eq!(given(&c, FOOT, "r"), vec![Input::Key(K::KeyQ)]);
    }

    #[test]
    fn each_map_has_its_mouse() {
        let c = Controls::default();
        assert_eq!((c.mouse(Map::Infantry), c.mouse(Map::Air), c.keyboard(Map::Land)), (0.25, 0.5, 0.5));
        assert!(c.invert(Map::Air) && !c.invert(Map::Land));
        let mut c = c;
        c.adopt_old_mouse(2.0, true);
        assert_eq!((c.mouse(Map::Infantry), c.mouse(Map::Air)), (0.5, 1.0));
        assert!(c.invert(Map::Infantry) && !c.invert(Map::Common));
        // (a key in the axis's place leaves the mouse out)
        let mut c = Controls::default();
        let row = AIR.iter().position(|a| a.0 == "CONTROLS_AIR_ROLL_LEFT").unwrap();
        c.define(Map::Air, row, 0, Input::Key(K::KeyQ));
        assert!(c.uses_axis(Map::Air, 0));
        c.define(Map::Air, row + 1, 0, Input::Key(K::KeyE));
        assert!(!c.uses_axis(Map::Air, 0));
    }

    #[test]
    fn a_profile_as_the_original_writes_it() {
        let dir = std::env::temp_dir().join(format!("bytefield-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |n: &str, t: &str| std::fs::write(dir.join(n), t).unwrap();
        write("Common.con", "ControlMap.create defaultGameControlMap\r\nControlMap.addKeyToTriggerMapping c_PIUse IDFKeyboard IDKey_F c_CMPushAndHold\r\nControlMap.addKeyToTriggerMapping c_PIUse IDFKeyboard IDKey_Numpad5 c_CMPushAndHold 1\r\ngame.setCommonMouseSensitivity 0.700000\r\n");
        write("Infantry.con", "ControlMap.addKeysToAxisMapping c_PIThrottle IDFKeyboard IDKey_I IDKey_K\r\nControlMap.addAxisToTriggerMapping c_PINextItem c_PIPrevItem IDFMouse IDAxis_2\r\ngame.setInfMouseSensitivity 0.600000\r\ngame.setInfMouseInvert 1\r\n");
        write("Land.con", "ControlMap.addButtonToTriggerMapping c_PIFire IDFMouse IDButton_2 c_CMPushAndHold\r\ngame.setLandSeaKeyboardSensitivity 0.200000\r\n");
        write("Air.con", "ControlMap.addAxisToAxisMapping c_PIRoll IDFMouse IDAxis_0 0\r\nControlMap.addKeysToAxisMapping c_PIRoll IDFKeyboard IDKey_ArrowRight IDKey_ArrowLeft 1\r\ngame.setAirMouseInvert 0\r\n");
        let c = Controls::from_profile(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(given(&c, FOOT, "e"), vec![Input::Key(K::KeyF), Input::Key(K::Numpad5)]);
        assert_eq!(given(&c, FOOT, "w"), vec![Input::Key(K::KeyI)]);
        assert_eq!(given(&c, FOOT, "s"), vec![Input::Key(K::KeyK)]);
        assert_eq!(given(&c, FOOT, "wheeldown"), vec![Input::WheelDown]);
        // (what it does not map is empty, as there)
        assert_eq!(given(&c, FOOT, "r"), vec![]);
        assert_eq!(given(&c, Context { map: Map::Land, ..FOOT }, "lmb"), vec![Input::Button(MouseButton::Middle)]);
        assert_eq!(given(&c, PLANE, "right"), vec![Input::Key(K::ArrowRight)]);
        assert!(c.uses_axis(Map::Air, 0) && !c.uses_axis(Map::Air, 1));
        assert_eq!((c.mouse(Map::Common), c.mouse(Map::Infantry), c.keyboard(Map::Land)), (0.7, 0.6, 0.2));
        assert!(c.invert(Map::Infantry) && !c.invert(Map::Air));
        // (ours stays)
        assert_eq!(given(&c, PLANE, "6"), vec![Input::Key(K::Digit6)]);
    }

    /// The original's default profile is, read, our defaults but for what is ours on purpose.
    #[test]
    fn the_default_profile_gives_the_defaults() {
        let game = bytefield_testdata::require!(Base);
        let dir = &game.join("Mods/bf1942/Settings/Default/Controls");
        let c = Controls::from_profile(dir).unwrap();
        let mut d = Controls::default();
        // (Alt walks: ours)
        let walk = INFANTRY.iter().position(|a| a.1 == "shift").unwrap();
        d.keys[Map::Infantry.index()][walk][1] = None;
        for m in Map::ALL {
            for (row, (a, b)) in c.keys[m.index()].iter().zip(&d.keys[m.index()]).enumerate() {
                assert_eq!(a, b, "{:?} {}", m, actions(m)[row].0);
            }
        }
        assert_eq!((c.mouse, c.keyboard, c.invert), (d.mouse, d.keyboard, d.invert));
    }

    #[test]
    fn every_key_has_the_originals_name_or_the_lexicons() {
        assert_eq!(lexicon_key(Input::Key(K::ShiftLeft)).as_deref(), Some("IDKey_LeftShift"));
        assert_eq!(lexicon_key(Input::Key(K::Numpad0)).as_deref(), Some("IDKey_Numpad0"));
        assert_eq!(lexicon_key(Input::Button(MouseButton::Right)).as_deref(), Some("Mouse_IDButton_1"));
        assert_eq!(lexicon_key(Input::WheelUp).as_deref(), Some("MWheelUp"));
        assert_eq!(lexicon_key(Input::MouseAxis(1)).as_deref(), Some("Mouse_IDAxis_1"));
        assert_eq!(profile_key("IDKey_Equals"), Some(Input::Key(K::Equal)));
        assert_eq!(profile_key("IDKey_F12"), Some(Input::Key(K::F12)));
        assert_eq!(profile_key("IDKey_Escape"), None);
    }

    /// The defaults against the original's files, when they are here: every key of `Settings/Default/Controls` that we use.
    #[test]
    fn the_defaults_are_the_originals() {
        let game = bytefield_testdata::require!(Base);
        let dir = &game.join("Mods/bf1942/Settings/Default/Controls");
        let name = |pi: &str| -> Option<String> {
            Some(
                match pi {
                    "c_PIAction" => "space",
                    "c_PIWalk" => "shift",
                    "c_PIMouseLook" => "shift",
                    "c_PIFire" => "lmb",
                    "c_PIAltFire" => "rmb",
                    "c_PIReload" => "r",
                    "c_PICrouch" => "ctrl",
                    "c_PILie" => "z",
                    "c_PIDrop" => "g",
                    "c_PIUse" => "e",
                    "c_PISayAll" => "k",
                    "c_PISayTeam" => "l",
                    "c_PIMap" => "m",
                    "c_PIZoomMap" => "n",
                    "c_PIShowScoreBoard" => "tab",
                    "c_PIToggleCameraMode" => "c",
                    "c_PIScreenShot" => "print",
                    "c_PICameraMode1" => "f9",
                    "c_PICameraMode2" => "f10",
                    "c_PICameraMode3" => "f11",
                    "c_PICameraMode4" => "f12",
                    "c_GITogglePause" => "p",
                    _ => {
                        if let Some(d) = pi.strip_prefix("c_PIMenuSelect") {
                            return Some(d.to_string());
                        }
                        if let Some(d) = pi.strip_prefix("c_PIRadio") {
                            return Some(format!("f{d}"));
                        }
                        return None;
                    }
                }
                .to_string(),
            )
        };
        let key = |id: &str| -> Option<Input> {
            let id = id.strip_prefix("IDKey_")?;
            let wanted = match id {
                "LeftShift" => "LShift".to_string(),
                "LeftCtrl" => "LCtrl".to_string(),
                other => other.strip_prefix("Arrow").map(String::from).or_else(|| other.strip_prefix("Numpad").map(|d| format!("Num{d}"))).unwrap_or_else(|| other.to_string()),
            };
            input_of(&wanted)
        };
        let mut checked = 0;
        let c = Controls::default();
        for (file, ctx) in [("Infantry.con", FOOT), ("Land.con", Context { map: Map::Land, ..FOOT }), ("Air.con", PLANE), ("Common.con", FOOT)] {
            let text = std::fs::read_to_string(dir.join(file)).unwrap();
            for line in text.lines() {
                let w: Vec<&str> = line.split_whitespace().collect();
                match w.first().copied() {
                    Some("ControlMap.addKeyToTriggerMapping") if w.len() >= 4 => {
                        let (Some(n), Some(i)) = (name(w[1]), key(w[3])) else { continue };
                        assert!(given(&c, ctx, &n).contains(&i), "{file}: {} {} is not {n}", w[1], w[3]);
                        checked += 1;
                    }
                    Some("ControlMap.addButtonToTriggerMapping") if w.len() >= 4 => {
                        let Some(n) = name(w[1]) else { continue };
                        let i = Input::Button(if w[3] == "IDButton_0" { MouseButton::Left } else { MouseButton::Right });
                        assert!(given(&c, ctx, &n).contains(&i), "{file}: {} {} is not {n}", w[1], w[3]);
                        checked += 1;
                    }
                    Some("ControlMap.addKeysToAxisMapping") if w.len() >= 5 => {
                        let names = match (w[1], file) {
                            ("c_PIThrottle", _) => ["w", "s"],
                            ("c_PIYaw", _) => ["d", "a"],
                            ("c_PIPitch", _) => ["up", "down"],
                            ("c_PIRoll", _) => ["right", "left"],
                            ("c_PICameraX", _) => ["num6", "num4"],
                            ("c_PICameraY", _) => ["num8", "num2"],
                            _ => continue,
                        };
                        for (n, id) in names.iter().zip([w[3], w[4]]) {
                            let i = key(id).unwrap_or_else(|| panic!("{id}"));
                            assert!(given(&c, ctx, n).contains(&i), "{file}: {} {id} is not {n}", w[1]);
                            checked += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        assert!(checked > 60, "only {checked} checked");
    }
}
