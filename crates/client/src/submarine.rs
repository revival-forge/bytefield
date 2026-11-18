//! The submarine's HUD: the periscope's mask and cross, the bearing ruler, the depth bar, the two depth tapes and the
//! oxygen. Measured on the original's Gato in El_Alamein flooded (`WATER=75`), in the original's
//! 800x600 pixels like `gauges.rs`. Only for the driver of a vehicle with `submarineData`, in first person.
//!
//! The original's layout is data (`menu/InGame` of `menu.rfa`) moved by variables its client fills each frame, and
//! [`variables`] is that code. Nothing on it is in metres: the tapes and the bar are **fractions of the water's depth
//! under the submarine** (`Submarine/MaximumDepth`), so the same dive reads 1.75 over 50 m of water and 4 over 30. The
//! left tape and the bar's mark are `Submarine/Depth` (the hull's depth less `setSubmarineHudDepthModifier`, which is
//! subtracted, not a scale: the Gato's origin floats 7.3 m under the water and its modifier is 5.9), the right tape is
//! `Submarine/WantedDepth` (how far the floater's input has gone, at most 3/4 of the water's depth: there the strip's
//! end reaches the window's), and the red mark on the bar is `Submarine/MaxDepth` (`submarineData`'s, where the
//! pressure starts to hurt), drawn only where the water is deep enough to get there.
//!
//! Port level 2: the layout to the pixel at 1024x768 and the variables from the client's code; level 3 for what the
//! notes below say is not known.

use bevy::prelude::*;

use crate::assets::Cache;
use crate::gauges::{K, picture, place};
use crate::play::Game;

#[derive(Component, Clone, Copy, PartialEq)]
enum S {
    /// everything else; shown or hidden together
    Piece,
    /// the depth's mark on the blue bar
    Slider,
    /// the red mark on the blue bar: the depth that hurts
    Limit,
    /// a tape's strip (0: the depth, 1: the depth asked for), moved inside its clip
    Strip(usize),
    /// the oxygen's red bars, cut from the top
    Oxygen,
}

pub struct SubmarinePlugin;

impl Plugin for SubmarinePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (setup.run_if(resource_added::<Game>), update.run_if(resource_exists::<Game>)).chain());
    }
}

/// The panels' top and the tapes' and bars' x (the bars' textures, 32 or 64 wide, are drawn 30 or 60 wide: the
/// frames' outlines are 37 px apart at 1024, not 40).
const TOP: f32 = 527.3;
const BAR_X: f32 = 344.9;
const TAPE_X: [f32; 2] = [367.2, 403.1];
const O2_X: f32 = 438.3;
/// The strip (32x256, drawn 30 wide): `0'` at row 32, in the window's middle with nothing to show, and its whole height
/// is the water's depth (its labels, each 14 rows, are not metres). The right tape stops at 3/4: the strip's end at the
/// window's (6.9 on its labels).
const STRIP_ZERO: f32 = 32.0;
const STRIP_ROWS: f32 = 256.0;
/// The window: the panel's inside (rows 3 to 61 of 64)
const WIN: [f32; 2] = [3.0, 61.0];
/// The bar's height, the water's depth: its slider's top and the red mark (11x3, 5 px into the bar's 32) go down it
const BAR_ROWS: f32 = 64.0;
const LIMIT: [f32; 3] = [348.0, 11.0, 3.0];
/// The cross and the ruler's middle (1.6 px left of and over the screen's middle)
const MID: [f32; 2] = [398.4, 298.4];
const RULER_Y: f32 = 462.3;
const LINE: f32 = 1.2;

fn setup(mut commands: Commands, g: Res<Game>, mut cache: ResMut<Cache>, mut images: ResMut<Assets<Image>>) {
    // (stretched: by default an image keeps its proportions inside its node, and these are drawn 30 wide from 32)
    let mut pic = |n: &str| picture(&g, &mut cache, &mut images, n).map(|h| ImageNode { image: h, image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() }).unwrap_or_default();
    let hidden = Visibility::Hidden;
    // the mask, the whole screen, under the rest of the HUD
    let full = Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands.spawn((full, pic("Submarine/periscope.tga"), GlobalZIndex(-1), hidden, S::Piece));
    // the cross: `periscope_crosshair.tga` (its lines at texel 127.5 of 256: 1.6 px left of and over the middle), but 1 px
    // wide as the original draws it (stretched and filtered, the texture's are 2 to 4)
    let black = BackgroundColor(Color::BLACK);
    let line = |x: f32, y: f32, w: f32, h: f32| Node { position_type: PositionType::Absolute, left: Val::Px(x * K), top: Val::Px(y * K), width: Val::Px(w * K), height: Val::Px(h * K), ..default() };
    let mut h = line(0.0, MID[1], 0.0, 1.0 / K);
    h.width = Val::Percent(100.0);
    let mut v = line(MID[0], 0.0, 1.0 / K, 0.0);
    v.height = Val::Percent(100.0);
    commands.spawn((h, black, GlobalZIndex(-1), hidden, S::Piece));
    commands.spawn((v, black, GlobalZIndex(-1), hidden, S::Piece));
    // the ruler: 256 wide, a tick each 16 (the ends, 20 tall; each 32, 15; between, 8) and the yellow mark in the middle
    // TODO: it did not move in any capture (7 s with A and D: the Gato hardly turned). The original's client fills
    // `Submarine/DirectionAngle` with the view's bearing off the hull's (an atan2 of the view's axes on the hull's forward
    // and right) less `setSubmarineHudDirModifier` (Gato 0.01, Sub7C π/1000); which node of `menu/InGame` it moves (the
    // ticks or the mark) and by how much is to be read there.
    commands.spawn((line(MID[0] - 128.0, RULER_Y - LINE / 2.0, 256.0, LINE), black, hidden, S::Piece));
    for i in -8..=8i32 {
        let t = match i.abs() {
            0 => continue,
            8 => 20.0,
            a if a % 2 == 0 => 15.0,
            _ => 7.8,
        };
        commands.spawn((line(MID[0] + 16.0 * i as f32 - LINE / 2.0, RULER_Y - t / 2.0, LINE, t), black, hidden, S::Piece));
    }
    commands.spawn((line(MID[0] - 1.5, 452.3, 3.9, 22.6), black, hidden, S::Piece)).with_children(|m| {
        let fill = Node { position_type: PositionType::Absolute, left: Val::Px(0.8 * K), right: Val::Px(0.8 * K), top: Val::Px(0.8 * K), bottom: Val::Px(0.8 * K), ..default() };
        m.spawn((fill, BackgroundColor(Color::srgb_u8(123, 107, 4))));
    });
    // the depth's bar, its slider and the red mark. The bar is a fill of `panel_djup` (brown) and `panel_djup2` (blue) by
    // `Submarine/WaterDepth` over `Submarine/MaximumDepth`, the same number as a float and cut to an integer: always full.
    commands.spawn((place(BAR_X, TOP, 30.0, 64.0), pic("Submarine/panel_djup2_32x64"), hidden, S::Piece));
    commands.spawn((place(LIMIT[0], TOP, LIMIT[1], LIMIT[2]), BackgroundColor(Color::srgb(1.0, 0.0, 0.0)), GlobalZIndex(1), hidden, S::Limit));
    commands.spawn((place(BAR_X - 3.5, TOP, 32.0, 4.0), pic("Submarine/panel_djup3_32x4"), hidden, S::Slider));
    // the tapes: the strip in a clip of the window, the frame (dark, half see-through) over it
    // TODO: each tape is three strips moved together: `sub_deapth_numbers` (brown) under `…_green` and `…_red`, the green
    // one cut by `Submarine/PeriscopeDepth` (nobody sets it: the file's 10) and the red by `Submarine/MaxDepth`, each with
    // an integer of its own (green 4 and -55, red 116 and 57, left and right tape). How `BfScrollPictureNode` cuts them
    // is not read: the line between green and brown sits in the same row of the window for the same dive whatever the
    // water's depth (all green at rest; 15.7 rows under the middle after 4 s diving, 7.7 over it after 7 s, the same on
    // both tapes once still), so it goes with the depth in metres and not with the strip. Green all over here.
    for (i, x) in TAPE_X.into_iter().enumerate() {
        let mut clip = place(x, TOP + WIN[0], 30.0, WIN[1] - WIN[0]);
        clip.overflow = Overflow::clip();
        commands.spawn((clip, hidden, S::Piece)).with_children(|c| {
            let strip = Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Px(30.0 * K), height: Val::Px(256.0 * K), ..default() };
            c.spawn((strip, pic("Submarine/sub_deapth_numbers_green_32x256"), S::Strip(i)));
        });
        commands.spawn((place(x, TOP, 30.0, 64.0), pic("Submarine/sub_deapth_panel_32x64"), hidden, S::Piece));
    }
    // the oxygen: grey when full; the red bars over it as it goes (a guess: never seen used up)
    commands.spawn((place(O2_X, TOP, 60.0, 64.0), pic("Submarine/panel_syre_64x64"), hidden, S::Piece)).with_children(|b| {
        let clip = Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Px(60.0 * K), height: Val::Px(0.0), overflow: Overflow::clip(), ..default() };
        b.spawn((clip, S::Oxygen)).with_children(|c| {
            let full = Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Px(60.0 * K), height: Val::Px(64.0 * K), ..default() };
            c.spawn((full, pic("Submarine/panel_syre2_64x64")));
        });
    });
}

/// The variables of the original's HUD (`Submarine/…` in `menu/InGame`), as its client fills them each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Variables {
    /// `Depth`: the hull's depth less the template's modifier, never under 0
    depth: f32,
    /// `WantedDepth`: the floater's input, at most 3/4 of `maximum`
    wanted: f32,
    /// `MaxDepth`: where the pressure hurts, less the modifier; out of reach (9999) where the water is shallower
    max_depth: f32,
    /// `MaximumDepth`: the water's depth under the submarine, cut to an integer; what everything is drawn over
    maximum: f32,
}

/// `underwater` is the depth of the hull's origin, `wanted` what the floater is asked for, `water` the water's depth at
/// the submarine (its level less the ground's height), `max_depth` the template's (`submarineData`; -1 is the water's)
/// and `modifier` its `setSubmarineHudDepthModifier`.
fn variables(underwater: f32, wanted: f32, water: f32, max_depth: f32, modifier: f32) -> Variables {
    let maximum = water.trunc();
    let max_depth = if max_depth == -1.0 { maximum } else { max_depth };
    let max_depth = if maximum < max_depth { 9999.0 } else { max_depth };
    Variables { depth: (underwater - modifier).max(0.0), wanted: wanted.min(0.75 * maximum), max_depth: max_depth - modifier, maximum }
}

/// What the HUD shows, each over the water's depth: the tapes, the bar's slider and red mark (if it is on the bar),
/// and the oxygen used (0..1).
struct Reading {
    tapes: [f32; 2],
    slider: f32,
    limit: Option<f32>,
    used: f32,
}

fn reading(g: &Game) -> Option<Reading> {
    if g.third_person {
        return None;
    }
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let s = &sim.vehicles[p.soldier.filter(|_| p.alive)?];
    let (k, seat) = (s.entry.inside?, s.entry.position);
    let v = &sim.vehicles[k];
    let sub = v.entry.submarine?;
    if seat != 0 {
        return None;
    }
    let t = sim.world.db.object_template(&v.template)?;
    // DC gives `submarineData` to what is on land too (T72, M1A1, M2A3, the Pickup's family: its way of drowning them)
    if !t.get("setvehicletype").and_then(|a| a.first()).is_some_and(|k| k.trim().eq_ignore_ascii_case("vtsubmarine")) {
        return None;
    }
    let modifier = t.get("submarineHudDepthModifier").and_then(|a| a.first()?.trim().parse::<f32>().ok()).unwrap_or(0.0);
    // the floater's bundle: how far its input has gone (the depth it asks for, in metres)
    // TODO: diving for 4 s over 50 m of water the original asks for 8.8 m at the key's release and both tapes stop at 9.6 m
    // half a second later (the hull's origin at 15.5 m); here the floater's input goes 2 m/s (8 m) and the hull follows
    // more slowly (10.4 m at the release, against 12.7). The floater's rate and the hull's lag are the sim's.
    let angle = v.craft.parts.iter().filter(|p| matches!(p.kind, bytefield_sim::craft::Kind::Floating(_))).find_map(|p| p.bundle.as_ref().map(|b| b.angle[1].abs())).unwrap_or(0.0);
    let pos = v.craft.body.pos;
    let ground = sim.world.height_at(pos[0] as f64, pos[2] as f64).unwrap_or(f64::NAN) as f32;
    let water = v.craft.water_level.map(|w| w - ground).filter(|d| d.is_finite()).unwrap_or(0.0);
    let h = variables(v.craft.body.underwater, angle, water, sub.max_depth, modifier);
    if g.tick % 30 == 0 && std::env::var_os("BYTEFIELD_SUB_DEBUG").is_some() {
        info!("sub: tick {} depth {:.2} m ballast {:.2} oxygen {:.3} y {:.2} ground {:.2} hud {:?}", g.tick, v.craft.body.underwater, angle, sub.oxygen, pos[1], ground, h);
    }
    // (no water under it: nothing to divide by; everything stays at the top)
    let over = |x: f32| x / h.maximum.max(1.0);
    Some(Reading {
        tapes: [over(h.depth), over(h.wanted)],
        slider: over(h.depth).min(1.0),
        limit: Some(over(h.max_depth)).filter(|f| *f <= 1.0),
        used: (1.0 - sub.oxygen).clamp(0.0, 1.0),
    })
}

fn update(g: Res<Game>, mut pieces: Query<(&S, &mut Node, Option<&mut Visibility>)>) {
    let r = reading(&g);
    for (s, mut node, vis) in pieces.iter_mut() {
        let shown = r.as_ref().is_some_and(|r| *s != S::Limit || r.limit.is_some());
        let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if let Some(mut vis) = vis
            && *vis != want
        {
            *vis = want;
        }
        let Some(r) = r.as_ref() else { continue };
        let set = |n: &mut Val, to: Val| {
            if *n != to {
                *n = to;
            }
        };
        match *s {
            S::Piece => {}
            S::Slider => set(&mut node.bottom, Val::Px((600.0 - 4.0 - (TOP + BAR_ROWS * r.slider)) * K)),
            S::Limit => set(&mut node.bottom, Val::Px((600.0 - LIMIT[2] - (TOP + BAR_ROWS * r.limit.unwrap_or(0.0))) * K)),
            S::Strip(i) => {
                let half = (WIN[1] - WIN[0]) / 2.0;
                let row = (STRIP_ZERO + STRIP_ROWS * r.tapes[i]).clamp(half, 256.0 - half);
                set(&mut node.top, Val::Px((half - row) * K));
            }
            S::Oxygen => set(&mut node.height, Val::Px(64.0 * K * r.used)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers of the original's Gato (modifier 5.9, hurt under 40 m, its origin 7.3 m under the water at rest) against
    /// what its HUD showed flooding El_Alamein to several depths.
    #[test]
    fn the_originals_readings() {
        let rows = |v: f32, h: Variables| STRIP_ROWS * v / h.maximum;
        // at rest over 50 m of water (50.9: cut to 50) the left tape reads 0.25 on its labels, 7 rows; the right one, 0
        let h = variables(7.3, 0.0, 50.9, 40.0, 5.9);
        assert_eq!(h.maximum, 50.0);
        assert!((rows(h.depth, h) - 7.0).abs() < 0.5, "{h:?}");
        assert_eq!(h.wanted, 0.0);
        // the red mark, 43.6 rows down the bar: its middle at y 573 of 600
        assert!((TOP + BAR_ROWS * h.max_depth / h.maximum + LIMIT[2] / 2.0 - 573.0).abs() < 1.0, "{h:?}");
        // over 70 m the mark is higher; over 30 and 15 m nobody gets to 40: no mark
        assert!(variables(7.3, 0.0, 70.0, 40.0, 5.9).max_depth / 70.0 < h.max_depth / 50.0);
        assert!(variables(7.3, 0.0, 30.0, 40.0, 5.9).max_depth > 30.0);
        // the right tape stops at 3/4 of the water's depth: 192 rows, 6.9 on its labels (each 28 rows)
        let h = variables(12.0, 40.0, 15.0, 40.0, 5.9);
        assert_eq!(rows(h.wanted, h), 192.0);
        // the hull's origin over the modifier (in the air, or a boat with `submarineData`): 0, not negative
        assert_eq!(variables(2.0, 0.0, 50.0, 40.0, 5.9).depth, 0.0);
        // no max depth in the template (-1): the water's
        assert_eq!(variables(0.0, 0.0, 50.0, -1.0, 0.0).max_depth, 50.0);
    }
}
