//! The soldiers' animation state machine, as far as it moves them.
//!
//! `Animations/AnimationStates.con` (run by the executable) builds one machine of states
//! (`AnimationStateMachine.createState` and the commands after it). A soldier has two instances
//! of it, the lower body and the upper body; only the lower body's matters to the physics:
//! - its state's flags give the pose (`c_AsmIsCrouching`, `c_AsmIsLying`: the soldier's pose lookup)
//!   and block jumping (crouching, lying);
//! - its state's `setSpeed forward turn strafe` multiply what the input asks for
//!   (`checkTransitions` hands them to `handlePlayerInput`);
//! - the input moves it from state to state (`addTransitionOne`), every frame, from
//!   `handlePlayerInput`; if no transition matches it goes to its `returnToState` (or stays);
//! - the animation's time moves it too (`addTransitionWhenDone` once the time leaves 0..1),
//!   in `handleFrameUpdate`: once per main-loop frame with the real time since the last one, so
//!   on the server it does not follow the 30 Hz of the physics (under gdb, far from it).
//!
//! Idles (after 4–7 s without input, at random) and the upper body are not modelled.

use std::collections::HashMap;

/// `c_Asm*` flags of a state (`setFlag`).
pub const HIDE_WEAPON: u32 = 0x2;
pub const LOCK_FREE_LOOK: u32 = 0x4;
pub const SWIMMING: u32 = 0x8;
pub const CLIMBING: u32 = 0x10;
pub const CROUCHING: u32 = 0x20;
pub const LYING: u32 = 0x40;
/// `c_SstJump`: a state with this sound trigger cannot jump again.
pub const SOUND_JUMP: i32 = 4;

/// `c_PI*`: the input axes by name.
const INPUTS: &[(&str, usize)] = &[
    ("c_piyaw", 0), ("c_pipitch", 1), ("c_piroll", 2), ("c_pithrottle", 3), ("c_pimouselookx", 4),
    ("c_pimouselooky", 5), ("c_picamerax", 6), ("c_picameray", 7), ("c_pifire", 8), ("c_piaction", 9),
    ("c_piuse", 10), ("c_pimouselook", 11), ("c_piwalk", 12), ("c_pirun", 13), ("c_pialtfire", 23),
    ("c_pireload", 24), ("c_pidrop", 25), ("c_pilie", 28), ("c_picrouch", 29),
];

fn constant(s: &str) -> Option<f64> {
    let l = s.to_ascii_lowercase();
    if let Some(&(_, i)) = INPUTS.iter().find(|(n, _)| *n == l) {
        return Some(i as f64);
    }
    Some(match l.as_str() {
        "c_asmhideweapon" => 2.0,
        "c_asmlockfreelook" => 4.0,
        "c_asmisswimming" => 8.0,
        "c_asmisclimbing" => 16.0,
        "c_asmiscrouching" => 32.0,
        "c_asmislying" => 64.0,
        "c_asmlooping" | "c_asmplaylooping" => 1.0,
        "c_asmplayonce" => 0.0,
        "c_sststand" => 1.0,
        "c_sstwalk" => 2.0,
        "c_sstrun" => 3.0,
        "c_sstjump" => 4.0,
        "c_ssttocrouch" => 6.0,
        "c_sstfromcrouch" => 7.0,
        "c_sstcrouchmove" => 8.0,
        "c_ssttocrawl" => 9.0,
        "c_sststandup" => 10.0,
        "c_sstcrawlmove" => 11.0,
        "c_ssttoswim" => 12.0,
        "c_sstswim" => 13.0,
        "c_sstswimstand" => 14.0,
        "c_sstfromswim" => 15.0,
        "c_sstopenparachute" => 16.0,
        "c_sstfallinghigh" => 17.0,
        "c_sstparachuteland" => 18.0,
        "c_ssthitdamage" => 19.0,
        "c_sstkilled" => 21.0,
        "c_ssthealedbymedic" => 22.0,
        "c_sstclimbladder" => 23.0,
        // "--1.0" (3pAnimationsTweaking.con, Ub_WalkBackwardMp18): the server reads −1
        _ => return s.strip_prefix('-').filter(|r| r.starts_with('-')).unwrap_or(s).parse().ok(),
    })
}

/// What a transition's bound of ±1 becomes.
pub const OPEN: f32 = 999999.0;

/// `addTransitionOne input lo hi target` (+ `addToLatestTransition input lo hi`): every input
/// given must be in [lo, hi] (an input not mapped reads 0).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Transition {
    pub inputs: Vec<(usize, f32, f32)>,
    pub target: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct State {
    pub name: String,
    /// `setSoundTrigger` (−1: none)
    pub sound: i32,
    pub flags: u32,
    /// `setSpeed forward turn strafe`
    pub speed: [f32; 3],
    /// `setMorphFactor` (default 5)
    pub morph: f32,
    pub transitions: Vec<Transition>,
    pub return_to: Option<String>,
    /// `addTransitionWhenDone` (`_POSE_`: the instance's pose state)
    pub when_done: Option<String>,
    /// `addAnimation file speed looping`, at most two (the third person's, then the first
    /// person's); one whose file does not load is not added, and a first one with "1p" in its
    /// path goes second, after an empty one ("": no animation, its time runs all the same).
    /// `set3pAnimationSpeed` sets the first one's speed.
    pub anims: Vec<(String, f32, bool)>,
    pub idles: Vec<String>,
    /// `addTransitionItem` (+9): with no `addTransitionWhenDone`, the state goes at once to the
    /// one named as it plus the item's name (or to the pose state)
    pub item: bool,
    /// `addRandomTransition`: … or to one of these, at random
    pub random: Vec<String>,
    /// `setOtherState kind state`: the state another animated thing goes to with this one
    /// (`c_AsmWeaponState`: the weapon in the hand, on its own skeleton; `c_AsmFaceState`: the
    /// face), by the kind's name (lower case)
    pub other: Vec<(String, String)>,
}

impl State {
    fn new(name: &str) -> State {
        State {
            name: name.to_string(),
            sound: -1,
            flags: 0,
            speed: [1.0; 3],
            morph: 5.0,
            transitions: Vec::new(),
            return_to: None,
            when_done: None,
            anims: Vec::new(),
            idles: Vec::new(),
            item: false,
            random: Vec::new(),
            other: Vec::new(),
        }
    }

    /// The animation an instance plays (`updateState(dt, 0, …)` on the server: the first):
    /// its speed and whether it loops.
    pub fn anim(&self) -> Option<(f32, bool)> {
        self.anims.first().map(|a| (a.1, a.2))
    }
}

/// The animation state's add animation: at most two; not added if the file does
/// not load (an empty name never does); the first one, if its path has "1p", goes second after
/// an empty copy of itself.
fn add_animation(st: &mut State, loaded: &mut Vec<Option<Anim>>, file: &str, speed: f32, looping: bool, load: &mut impl FnMut(&str) -> Option<Anim>) {
    if st.anims.len() >= 2 || file.is_empty() {
        return;
    }
    let Some(a) = load(file) else { return };
    if st.anims.is_empty() && file.contains("1p") {
        st.anims.push((String::new(), speed, looping));
        loaded.push(None);
    }
    st.anims.push((file.to_string(), speed, looping));
    loaded.push(Some(a));
}

/// `replaceSubString(s, find, with)`: every `find` (case matters) becomes `with`; false if none
/// was there or they are the same.
fn replace_all(s: &mut String, find: &str, with: &str) -> bool {
    if s.is_empty() || find.is_empty() || with.is_empty() || find == with || !s.contains(find) {
        return false;
    }
    *s = s.replace(find, with);
    true
}

/// The states of `AnimationStateMachine`, in creation order (their index is their id).
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Machine {
    pub states: Vec<State>,
    #[serde(skip)]
    index: HashMap<String, usize>,
    /// each state's animations, read (the animation manager's animation lookup: one per file); None
    /// for an empty one
    #[serde(skip)]
    pub animations: Vec<Vec<Option<std::sync::Arc<bytefield_formats::anim::BoneAnimation>>>>,
}

type Anim = std::sync::Arc<bytefield_formats::anim::BoneAnimation>;

impl Machine {
    /// From the `AnimationStateMachine.*` commands of the loaded scripts, in order; `read` gives
    /// an animation file's bytes (an animation that does not read is not added, as on the server).
    pub fn from_commands<'a>(calls: impl IntoIterator<Item = (&'a str, &'a [String])>, read: impl Fn(&str) -> Option<Vec<u8>>) -> Machine {
        let mut m = Machine::default();
        let mut cache: HashMap<String, Option<Anim>> = HashMap::new();
        let mut load = |f: &str| -> Option<Anim> {
            cache
                .entry(f.to_ascii_lowercase())
                .or_insert_with(|| read(f).and_then(|b| bytefield_formats::anim::BoneAnimation::parse(&b).ok()).map(std::sync::Arc::new))
                .clone()
        };
        let mut cur: Option<usize> = None;
        for (cmd, args) in calls {
            let c = cmd.to_ascii_lowercase();
            let Some(method) = c.strip_prefix("animationstatemachine.") else { continue };
            let num = |i: usize| args.get(i).and_then(|a| constant(a)).map(|v| v as f32);
            let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
            match method {
                "createstate" => {
                    let Some(name) = args.first() else { continue };
                    cur = Some(m.create(name));
                    continue;
                }
                "setactivestate" => {
                    cur = args.first().and_then(|n| m.find(n));
                    continue;
                }
                // `setAnimationSpeed(state, 3p, 1p)`: the first animation's speed, the second's
                "set3panimationspeed" | "set1panimationspeed" => {
                    let k = (method == "set1panimationspeed") as usize;
                    if let (Some(i), Some(v)) = (args.first().and_then(|n| m.find(n)), num(1))
                        && let Some(a) = m.states[i].anims.get_mut(k)
                    {
                        a.1 = v;
                    }
                    continue;
                }
                // copyState new find 3p speed3p 1p speed1p; copyState2 new find (the same names,
                // the animations' own speeds)
                "copystate" => {
                    if let (Some(src), Some(s3), Some(s1)) = (cur, num(3), num(5)) {
                        m.copy_state(src, arg(0), arg(1), arg(2), s3, arg(4), s1, &mut load);
                    }
                    continue;
                }
                "copystate2" => {
                    if let Some(src) = cur {
                        let sp = |k: usize| m.states[src].anims.get(k).map_or(0.0, |a| a.1);
                        let (s3, s1) = (sp(0), sp(1));
                        m.copy_state(src, arg(0), arg(1), arg(0), s3, arg(0), s1, &mut load);
                    }
                    continue;
                }
                _ => {}
            }
            let Some(ci) = cur else { continue };
            let st = &mut m.states[ci];
            match method {
                "setsoundtrigger" => st.sound = num(0).map_or(-1, |v| v as i32),
                "setflag" => st.flags |= num(0).map_or(0, |v| v as u32),
                "setspeed" => {
                    if let (Some(a), Some(b), Some(c)) = (num(0), num(1), num(2)) {
                        st.speed = [a, b, c];
                    }
                }
                "setmorphfactor" => {
                    if let Some(v) = num(0) {
                        st.morph = v;
                    }
                }
                "addanimation" => {
                    if let (Some(f), Some(sp)) = (args.first(), num(1)) {
                        add_animation(st, &mut m.animations[ci], f, sp, num(2).unwrap_or(0.0) != 0.0, &mut load);
                    }
                }
                "addtransitionone" => {
                    // a bound of exactly ±1 is open: the engine stores ±999999 (read from the server)
                    if let (Some(i), Some(lo), Some(hi), Some(t)) = (num(0), num(1), num(2), args.get(3)) {
                        let lo = if lo == -1.0 { -OPEN } else { lo };
                        let hi = if hi == 1.0 { OPEN } else { hi };
                        st.transitions.push(Transition { inputs: vec![(i as usize, lo, hi)], target: t.clone() });
                    }
                }
                "addtolatesttransition" => {
                    if let (Some(i), Some(lo), Some(hi), Some(t)) = (num(0), num(1), num(2), st.transitions.last_mut()) {
                        t.inputs.truncate(1);
                        t.inputs.push((i as usize, lo, hi));
                    }
                }
                "returntostate" => st.return_to = args.first().cloned(),
                "addtransitionwhendone" => st.when_done = args.first().cloned(),
                "addtransitionitem" => st.item = true,
                "addrandomtransition" => st.random.extend(args.first().cloned()),
                "addidle" => st.idles.extend(args.first().cloned()),
                "setotherstate" => {
                    if let (Some(k), Some(t)) = (args.first(), args.get(1)) {
                        st.other.push((k.to_ascii_lowercase(), t.clone()));
                    }
                }
                "clearallidles" => st.idles.clear(),
                _ => {}
            }
        }
        m
    }

    /// `createState`: a new state, or the one of that name.
    fn create(&mut self, name: &str) -> usize {
        let key = name.to_ascii_lowercase();
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        self.states.push(State::new(name));
        self.animations.push(Vec::new());
        self.index.insert(key, self.states.len() - 1);
        self.states.len() - 1
    }

    /// The animation state machine's copy state of the active state: a
    /// state named as it with `find` replaced by `new` (none if the name has no `find` or the
    /// state exists), its data copied with `find` → `new` in the transitions' targets, idles,
    /// `returnToState` and `addTransitionWhenDone`; its animations with `find` → `3p` at speed
    /// `s3` (the first) and → `1p` at `s1` (the second; a lone animation of a state not named
    /// `Ub_*` is taken as the first person's). If none of them loads, the state goes again.
    #[allow(clippy::too_many_arguments)]
    fn copy_state(&mut self, src: usize, new: &str, find: &str, p3: &str, s3: f32, p1: &str, s1: f32, load: &mut impl FnMut(&str) -> Option<Anim>) {
        if new == find {
            return;
        }
        let mut name = self.states[src].name.clone();
        if !replace_all(&mut name, find, new) || self.find(&name).is_some() {
            return;
        }
        let mut st = self.states[src].clone();
        st.name = name.clone();
        for t in &mut st.transitions {
            replace_all(&mut t.target, find, new);
        }
        for s in st.idles.iter_mut().chain(st.return_to.iter_mut()).chain(st.when_done.iter_mut()).chain(st.other.iter_mut().map(|o| &mut o.1)) {
            replace_all(s, find, new);
        }
        let lone_1p = st.anims.len() == 1 && !name.to_ascii_lowercase().starts_with("ub_");
        let mut loaded = Vec::new();
        for (k, (file, _, looping)) in std::mem::take(&mut st.anims).into_iter().enumerate().take(2) {
            let (with, speed) = if lone_1p || k == 1 { (p1, s1) } else { (p3, s3) };
            let mut f = file;
            replace_all(&mut f, find, with);
            add_animation(&mut st, &mut loaded, &f, speed, looping, load);
        }
        if st.anims.is_empty() && !self.states[src].anims.is_empty() {
            return;
        }
        let i = self.create(&name);
        self.states[i] = st;
        self.animations[i] = loaded;
    }

    /// The animation of a state an instance plays (the first), if read.
    pub fn animation(&self, i: usize) -> Option<&bytefield_formats::anim::BoneAnimation> {
        self.animations.get(i).and_then(|a| a.first()).and_then(|a| a.as_deref())
    }

    /// The weapon's state for a state (`setOtherState c_AsmWeaponState`), its animation (the
    /// first), speed and whether it loops; and the state's own first speed.
    pub fn weapon_animation(&self, i: usize) -> Option<(&bytefield_formats::anim::BoneAnimation, f32, bool, f32)> {
        let (_, name) = self.states[i].other.iter().find(|o| o.0 == "c_asmweaponstate")?;
        let j = self.find(name)?;
        let a = self.animation(j)?;
        let (_, speed, looping) = self.states[j].anims.first()?;
        let own = self.states[i].anims.first().map_or(1.0, |a| a.1);
        Some((a, *speed, *looping, own))
    }

    /// The first person's animation of a state (its second), if read: the animation, its
    /// speed and whether it loops.
    pub fn animation_1p(&self, i: usize) -> Option<(&bytefield_formats::anim::BoneAnimation, f32, bool)> {
        let a = self.animations.get(i)?.get(1)?.as_deref()?;
        let (_, speed, looping) = self.states[i].anims.get(1)?;
        Some((a, *speed, *looping))
    }

    /// `findState` (no case).
    pub fn find(&self, name: &str) -> Option<usize> {
        self.index.get(&name.to_ascii_lowercase()).copied()
    }

    /// The animation state's transitions check: the first
    /// transition whose inputs all match, else the `returnToState` (or the state itself); a
    /// state that returns to itself, past its idle timer, goes to one of its idles (those that
    /// exist) instead, at random (`pick`: that one).
    fn check(&self, i: usize, input: &Input, timer: f32, pose: Option<usize>, pick: &mut dyn FnMut(usize) -> usize) -> usize {
        let st = &self.states[i];
        for t in &st.transitions {
            if t.inputs.iter().all(|&(k, lo, hi)| {
                let v = input.get(k);
                lo <= v && v <= hi
            }) && let Some(j) = self.find(&t.target)
            {
                return j;
            }
        }
        // an item state (`addTransitionItem`) does not return: it waits for its `updateState`
        // (`_POSE_`: the instance's pose state)
        let back = if st.item {
            i
        } else {
            match st.return_to.as_deref() {
                Some(n) if n.eq_ignore_ascii_case("_POSE_") => pose.unwrap_or(i),
                Some(n) => self.find(n).unwrap_or(i),
                None => i,
            }
        };
        if timer < 0.0 && back == i {
            let idles: Vec<usize> = st.idles.iter().filter_map(|n| self.find(n)).collect();
            if !idles.is_empty() {
                return idles[pick(idles.len())];
            }
        }
        back
    }

    /// The idles of a state that exist, by id.
    pub fn idles(&self, i: usize) -> Vec<usize> {
        self.states[i].idles.iter().filter_map(|n| self.find(n)).collect()
    }
}

/// `c_PIFire`.
const FIRE_INPUT: usize = 8;

/// A `PlayerInput`: 55 axes and the mask of the mapped ones (an unmapped axis reads 0).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Input {
    pub axes: Vec<f32>,
    pub mask: u64,
}

impl Default for Input {
    fn default() -> Input {
        Input { axes: vec![0.0; 55], mask: 0 }
    }
}

impl Input {
    pub fn get(&self, i: usize) -> f32 {
        if i < 55 && self.mask >> i & 1 != 0 { self.axes[i] } else { 0.0 }
    }
    pub fn mapped(&self, i: usize) -> bool {
        i < 55 && self.mask >> i & 1 != 0
    }
    pub fn set(&mut self, i: usize, v: f32) {
        if i < 55 {
            self.axes[i] = v;
        }
    }
}

/// An `AnimationStateMachineInstance` (the soldier's lower and upper body).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Instance {
    /// current state and the one the animation was set up for
    pub state: Option<usize>,
    pub prev: Option<usize>,
    /// the animation playing: speed, looping
    pub anim: Option<(f32, bool)>,
    /// its time: 0..1, done outside it
    pub time: f32,
    /// the morph from the previous animation: 0..1
    pub morph: f32,
    /// the pose's state, where `_POSE_` and an item state with no item go
    pub pose: Option<usize>,
    /// the time a new looping animation starts at, if above 0 (the upper body gets the
    /// lower body's time every frame)
    pub start: f32,
    /// the time left before an idle: `rand % 4 + 4` s when a state is set up, down
    /// with the frame updates' dt; 1000 while a new state waits to be set up
    pub idle: f32,
    /// the server's `rand` has no seed we know: our own, for the idle timer, the idle and
    /// `addRandomTransition`; `pick`, if given, is the next idle chosen (a reference run's)
    pub rng: u64,
    pub pick: Option<usize>,
    /// +4: the fire input was held at the last check
    pub firing: bool,
}

impl Instance {
    pub fn new(state: Option<usize>) -> Instance {
        Instance { state, prev: None, anim: None, time: 0.0, morph: 0.0, pose: None, start: 0.0, idle: 1000.0, rng: 1, pick: None, firing: false }
    }

    pub fn rand(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.rng >> 33) as u32
    }

    /// `setCurrentState`: the animation is set up at the next `update`. (It keeps the one it
    /// comes from, but setting the new one up clears that: the morph blends from the
    /// pose the skeleton has.)
    pub fn set(&mut self, i: usize) {
        if self.state != Some(i) {
            self.state = Some(i);
            self.anim = None;
            self.idle = 1000.0;
        }
    }

    pub fn flags(&self, m: &Machine) -> u32 {
        self.state.map_or(0, |i| m.states[i].flags)
    }

    pub fn sound(&self, m: &Machine) -> i32 {
        self.state.map_or(0, |i| m.states[i].sound.max(0))
    }

    /// The soldier's pose lookup: 0 stand, 1 crouch, 2 lie.
    pub fn pose(&self, m: &Machine) -> usize {
        let f = self.flags(m);
        if f & CROUCHING != 0 { 1 } else if f & LYING != 0 { 2 } else { 0 }
    }

    /// `checkTransitions(input, &forward, &turn, &strafe)`: follow the input's transitions as far
    /// as they go; the multipliers are those of the state it ends in.
    pub fn check(&mut self, m: &Machine, input: &Input) -> [f32; 3] {
        let Some(mut i) = self.state else { return [0.0; 3] };
        for _ in 0..m.states.len() {
            if self.prev != Some(i) {
                self.idle = 1000.0;
            }
            // a state that plays once and is left by firing starts its animation again at each
            // new press
            if let Some((speed, false)) = self.anim
                && m.states[i].transitions.iter().any(|t| t.inputs.iter().any(|&(k, _, _)| k == FIRE_INPUT))
            {
                let f = input.get(FIRE_INPUT);
                if !self.firing && f > 0.0 {
                    self.time = if speed < 0.0 { 1.0 } else { 0.0 };
                }
                self.firing = f > 0.0;
            }
            let (timer, forced) = (self.idle, self.pick);
            let mut rng = self.rng;
            let j = m.check(i, input, timer, self.pose, &mut |n| match forced {
                Some(k) => m.idles(i).iter().position(|&x| x == k).unwrap_or(0),
                None => {
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    (rng >> 33) as usize % n
                }
            });
            self.rng = rng;
            if m.idles(i).contains(&j) {
                self.pick = None;
            }
            if j == i {
                break;
            }
            self.set(j);
            i = j;
        }
        m.states[i].speed
    }

    /// `updateState(dt, 0, item)` (from `handleFrameUpdate`): a new state sets its animation up
    /// (time 0, or 1 playing backwards; a looping one keeps the time it had, or takes `start`;
    /// none: time 100) and its idle timer; otherwise the time runs at the animation's speed and the morph at the
    /// state's. Then the animation state's update: with an `addTransitionWhenDone`, out of
    /// 0..1 the state goes there (`_POSE_`: the pose state); without, an item state goes to its
    /// name plus the item's (none such: the pose state) and one with random transitions to one
    /// of them. A state reached that is not the pose state is set up at once.
    pub fn update(&mut self, m: &Machine, dt: f32, item: &str) {
        let Some(i) = self.state else { return };
        let st = &m.states[i];
        // (a state with no animation is set up again every time: its idle timer never runs out)
        if self.prev == Some(i)
            && let Some((speed, _)) = self.anim
        {
            self.time += dt * speed;
            self.morph += dt * st.morph;
            if self.morph >= 1.0 {
                self.morph = 1.0;
            }
        } else {
            self.prev = Some(i);
            self.idle = (self.rand() & 3) as f32 + 4.0;
            self.anim = st.anim();
            match self.anim {
                Some((speed, looping)) => {
                    self.morph = if st.morph > 1000.0 { 1.0 } else { 0.0 };
                    if !looping {
                        self.time = if speed < 0.0 { 1.0 } else { 0.0 };
                    } else if self.start > 0.0 {
                        self.time = self.start;
                    }
                }
                None => self.time = 100.0,
            }
        }
        self.idle -= dt;
        // None: no change; Some(None): the pose state
        let next: Option<Option<usize>> = match st.when_done.as_deref() {
            Some(w) if w.eq_ignore_ascii_case("_POSE_") || m.find(w).is_some() => {
                (!(0.0..=1.0).contains(&self.time)).then(|| if w.eq_ignore_ascii_case("_POSE_") { None } else { m.find(w) })
            }
            _ if st.item => Some(m.find(&format!("{}{}", st.name, item))),
            _ if !st.random.is_empty() => {
                let k = self.rand() as usize % st.random.len();
                m.find(&st.random[k]).map(Some)
            }
            _ => None,
        };
        let Some(j) = next.map(|j| j.or(self.pose)) else { return };
        if let Some(j) = j
            && j != i
        {
            self.set(j);
            if Some(j) != self.pose {
                self.update(m, dt, item);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `.baf` of no bones and one frame.
    const TINY: [u8; 11] = [3, 0, 0, 0, 0, 0, 1, 0, 0, 0, 15];

    fn machine(lines: &[&str]) -> Machine {
        let calls: Vec<(String, Vec<String>)> = lines
            .iter()
            .map(|l| {
                let mut w = l.split_whitespace().map(String::from);
                (w.next().unwrap(), w.collect())
            })
            .collect();
        Machine::from_commands(calls.iter().map(|(c, a)| (c.as_str(), a.as_slice())), |_| Some(TINY.to_vec()))
    }

    #[test]
    fn input_moves_and_time_finishes() {
        let m = machine(&[
            "AnimationStateMachine.createState Lb_Stand",
            "AnimationStateMachine.addAnimation a.baf 0.8 1",
            "AnimationStateMachine.addTransitionOne c_PIThrottle 0.5 1 Lb_Run",
            "AnimationStateMachine.createState Lb_Run",
            "AnimationStateMachine.addAnimation b.baf 1.6 1",
            "AnimationStateMachine.addTransitionOne c_PIAction 0.5 1 Lb_Jump",
            "AnimationStateMachine.addTransitionOne c_PIThrottle 0.5 1 Lb_Run",
            "AnimationStateMachine.returnToState Lb_Stand",
            "AnimationStateMachine.createState Lb_Jump",
            "AnimationStateMachine.addAnimation c.baf 1.0 0",
            "AnimationStateMachine.addTransitionWhenDone Lb_Run",
            "AnimationStateMachine.setSpeed 0 1 1",
            "AnimationStateMachine.setSoundTrigger c_SstJump",
            "AnimationStateMachine.set3pAnimationSpeed Lb_Jump 2",
        ]);
        let mut a = Instance::new(m.find("lb_stand"));
        let mut inp = Input::default();
        inp.mask = 1 << 3 | 1 << 9;
        inp.set(3, 1.0);
        assert_eq!(a.check(&m, &inp), [1.0; 3]);
        assert_eq!(a.state, m.find("Lb_Run"));
        inp.set(9, 1.0);
        assert_eq!(a.check(&m, &inp), [0.0, 1.0, 1.0]);
        assert_eq!(a.sound(&m), SOUND_JUMP);
        a.update(&m, 0.1, "");
        assert_eq!(a.time, 0.0);
        for _ in 0..6 {
            a.update(&m, 0.1, "");
        }
        assert_eq!(a.state, m.find("Lb_Run"), "{a:?}");
    }

    #[test]
    fn weapon_states_are_copied_and_items_lead_to_them() {
        let m = Machine::from_commands(
            [
                "AnimationStateMachine.createState Ub_StandAim",
                "AnimationStateMachine.addTransitionItem",
                "AnimationStateMachine.createState Ub_Fire",
                "AnimationStateMachine.addTransitionItem",
                "AnimationStateMachine.createState Ub_StandAimThompson",
                "AnimationStateMachine.addAnimation 3p/Thompson/StandThompson.baf 0.8 1",
                "AnimationStateMachine.addAnimation 1p/Thompson/StandThompson.baf 0.1 1",
                "AnimationStateMachine.addTransitionOne c_PIFire 0.5 1 Ub_Fire",
                "AnimationStateMachine.returnToState Ub_StandAimThompson",
                "AnimationStateMachine.copyState2 Sg44 Thompson",
                "AnimationStateMachine.copyState K98 Thompson No4 1.0 K98 1.0",
                "AnimationStateMachine.copyState2 Mp40 Thompson",
                "AnimationStateMachine.createState Ub_FireThompson",
                "AnimationStateMachine.addAnimation 3p/Thompson/FireThompson.baf 1.0 0",
                "AnimationStateMachine.addTransitionWhenDone _POSE_",
                "AnimationStateMachine.set3pAnimationSpeed Ub_StandAimSg44 0.6",
            ]
            .iter()
            .map(|l| {
                let mut w = l.split_whitespace().map(String::from);
                (w.next().unwrap(), w.collect::<Vec<_>>())
            })
            .collect::<Vec<_>>()
            .iter()
            .map(|(c, a)| (c.as_str(), a.as_slice())),
            // no Mp40 files: its copy goes
            |f| (!f.contains("Mp40")).then(|| TINY.to_vec()),
        );
        let sg = &m.states[m.find("Ub_StandAimSg44").unwrap()];
        assert_eq!(sg.anims[0], ("3p/Sg44/StandSg44.baf".to_string(), 0.6, true));
        assert_eq!(sg.anims[1].0, "1p/Sg44/StandSg44.baf");
        assert_eq!(sg.return_to.as_deref(), Some("Ub_StandAimSg44"));
        let k98 = &m.states[m.find("Ub_StandAimK98").unwrap()];
        assert_eq!((k98.anims[0].0.as_str(), k98.anims[0].1), ("3p/No4/StandNo4.baf", 1.0));
        assert_eq!(k98.anims[1].0, "1p/K98/StandK98.baf");
        assert!(m.find("Ub_StandAimMp40").is_none());
        // the item state goes to the weapon's at once; firing, back to the pose when done
        let mut a = Instance::new(m.find("Ub_StandAim"));
        a.pose = m.find("Ub_StandAim");
        a.update(&m, 0.1, "Thompson");
        assert_eq!(a.state, m.find("Ub_StandAimThompson"));
        assert_eq!((a.anim, a.time), (Some((0.8, true)), 100.0));
        let mut inp = Input::default();
        inp.mask = 1 << 8;
        inp.set(8, 1.0);
        a.check(&m, &inp);
        a.update(&m, 0.1, "Thompson");
        assert_eq!(a.state, m.find("Ub_FireThompson"));
        for _ in 0..10 {  // 10 × 0.1 in f32 is past 1
            a.update(&m, 0.1, "Thompson");
        }
        assert_eq!(a.state, m.find("Ub_StandAim"));
        a.update(&m, 0.1, "Thompson");
        assert_eq!(a.state, m.find("Ub_StandAimThompson"));
        // no item: the pose state stays
        let mut b = Instance::new(m.find("Ub_StandAim"));
        b.pose = m.find("Ub_StandAim");
        b.update(&m, 0.1, "");
        assert_eq!((b.state, b.time), (m.find("Ub_StandAim"), 100.0));
    }
}
