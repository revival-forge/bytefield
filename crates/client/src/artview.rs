//! The artillery's view from a scout's mark: what the original draws over it,
//! measured on its screen at 1024×768 and placed in
//! its 800×600 pixels, as `gauges.rs`. Level 3 in the bars' meaning; the pictures are the original's:
//! - the binoculars' `setScopeIcon` (`binocular.tga`) over the screen, black at the sides of a wide one;
//! - their `setSightIcon` (`scout_ring_128x128.tga`) at 128, dark, and twice as large in white, round the centre,
//!   with a thin black cross;
//! - the bar of the gun's turn above (256 wide, «0» over its middle) and of its rise at the left (256 high, «0» at
//!   its foot), each with a yellow mark (`DirBarXScale`, `DirBarYScaleAbove`/`Below` of the seat);
//! - «Scout <name>: <seconds the mark has left>» in a dark box under the ring.
//!
//! - the player's minimap mark (the ring and its vehicle's icon) always at the white ring's right.

use bevy::prelude::*;
use bytefield_con::templates::Template;

use crate::bmfont::BmText;
use crate::play::Game;

const K: f32 = 1.28;
/// The bars' length (800×600 pixels), their ticks (8 parts, halved), and where they are.
const BAR: f32 = 256.0;
const PARTS: usize = 16;
const TURN_Y: f32 = 148.0;
const RISE_X: f32 = 62.5;
const RISE_TOP: f32 = 172.0;
const YELLOW: Color = Color::srgb(1.0, 0.85, 0.0);
const LINE: Color = Color::srgba(0.0, 0.0, 0.0, 0.85);

#[derive(Component)]
struct Root;
#[derive(Component)]
struct TurnMark;
#[derive(Component)]
struct RiseMark;
#[derive(Component)]
struct Label;
#[derive(Component)]
struct Left;

pub struct ArtViewPlugin;

impl Plugin for ArtViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (setup.run_if(resource_added::<Game>), update.run_if(resource_exists::<Game>)).chain());
    }
}

/// A picture of `menu/Texture`, as it is or with its colour made white (keeping its alpha).
fn picture(g: &Game, name: &str, white: bool, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let b = g.session.sim.world.vfs.read(&format!("menu/texture/{name}")).ok()?;
    let mut img = bytefield_formats::tga::decode(&b).ok()?;
    if white {
        for p in img.rgba.chunks_exact_mut(4) {
            // (the ring is a dark line on clear texels: its darkness becomes the white's alpha)
            let dark = 255 - p[0].max(p[1]).max(p[2]);
            // (fainter than the dark one: some 0.6)
            let a = (p[3] as u32 * dark as u32 * 6 / 2550) as u8;
            p.copy_from_slice(&[255, 255, 255, a]);
        }
    }
    Some(images.add(crate::assets::rgba_image(&img)))
}

/// A node centred on the original's (x, y) at the screen's middle, `w`×`h` of its pixels.
fn centred(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Percent(50.0),
        top: Val::Percent(50.0),
        margin: UiRect { left: Val::Px((x - 400.0 - w / 2.0) * K), top: Val::Px((y - 300.0 - h / 2.0) * K), ..default() },
        width: Val::Px(w * K),
        height: Val::Px(h * K),
        ..default()
    }
}

fn rect(x: f32, y: f32, w: f32, h: f32, c: Color) -> (Node, BackgroundColor) {
    (centred(x, y, w, h), BackgroundColor(c))
}

fn setup(mut commands: Commands, g: Res<Game>, mut images: ResMut<Assets<Image>>, mut cache: ResMut<crate::assets::Cache>) {
    let (scope, sight) = binoculars(&g);
    let mask = picture(&g, &scope, false, &mut images);
    let ring = picture(&g, &sight, false, &mut images);
    let white = picture(&g, &sight, true, &mut images);
    let disc = cache.texture(&g.session.sim.world.vfs, "menu/Texture/Minimap/minimap_icon_ring_32x32", &mut images);
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands.spawn((full.clone(), ZIndex(-1), Visibility::Hidden, Root)).with_children(|r| {
        // the mask: as wide as a 4:3 screen as high as ours, black at the sides (as the scope's)
        r.spawn(full.clone()).with_children(|m| {
            let black = || (Node { flex_grow: 1.0, height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::BLACK));
            m.spawn(black());
            if let Some(h) = mask {
                m.spawn((ImageNode { image: h, image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() }, Node { height: Val::Percent(100.0), aspect_ratio: Some(4.0 / 3.0), ..default() }));
            }
            m.spawn(black());
        });
        if let Some(h) = white {
            r.spawn((centred(400.0, 300.0, 256.0, 256.0), ImageNode::new(h)));
        }
        if let Some(h) = ring {
            r.spawn((centred(400.0, 300.0, 128.0, 128.0), ImageNode::new(h)));
        }
        // the player's mark at the white ring's right, wherever the gun is (the gun 6 or 40 m away, the view
        // turned or not): the minimap's ring (its arrow down) with the vehicle's icon (its gun up)
        // TODO: what turns them a little (some 12° in two captures, none in a third); where the original means it to be
        if let Some(h) = disc {
            r.spawn((centred(528.0, 300.0, 32.0, 32.0), ImageNode { image: h, color: Color::srgb(0.6, 0.6, 0.6), ..default() }, bevy::ui::UiTransform { rotation: Rot2::radians(std::f32::consts::PI), ..default() }));
        }
        r.spawn((centred(528.0, 300.0, 16.0, 16.0), ImageNode::default(), Visibility::Hidden, SeatIcon));
        // the cross: 117 to each side
        r.spawn(rect(400.0, 300.0, 234.0, 1.0, LINE));
        r.spawn(rect(400.0, 300.0, 1.0, 234.0, LINE));
        // the turn's bar: the line, its ticks (the ends and the eighths longer), «0», the yellow left end and mark
        r.spawn(rect(400.0, TURN_Y, BAR, 1.0, LINE));
        for k in 0..=PARTS {
            let h = if k == 0 || k == PARTS { 20.0 } else if k % 2 == 0 { 14.0 } else { 7.0 };
            r.spawn(rect(400.0 - BAR / 2.0 + BAR * k as f32 / PARTS as f32, TURN_Y, 1.0, h, LINE));
        }
        r.spawn(rect(400.0 - BAR / 2.0, TURN_Y, 3.0, 22.0, YELLOW));
        r.spawn((rect(400.0, TURN_Y, 3.0, 22.0, YELLOW), TurnMark));
        r.spawn((centred(400.0, TURN_Y - 18.0, 20.0, 10.0), BmText::new("Trebuchet MS11", Color::WHITE, 1.0, 20.0 * K, Justify::Center, true))).insert(Zero);
        // the rise's bar: the line, its ticks, «0» at its foot and the mark
        r.spawn(rect(RISE_X, RISE_TOP + BAR / 2.0, 1.0, BAR, LINE));
        for k in 0..=PARTS {
            let w = if k == 0 || k == PARTS { 20.0 } else if k % 2 == 0 { 14.0 } else { 7.0 };
            r.spawn(rect(RISE_X, RISE_TOP + BAR * k as f32 / PARTS as f32, w, 1.0, LINE));
        }
        r.spawn((rect(RISE_X, RISE_TOP + BAR, 22.0, 3.0, YELLOW), RiseMark));
        r.spawn((centred(RISE_X - 22.0, RISE_TOP + BAR, 12.0, 10.0), BmText::new("Trebuchet MS11", Color::WHITE, 1.0, 12.0 * K, Justify::Right, true))).insert(Zero);
        // the scout and the time left
        // (a rounded box with a light rim; the name greyer than the seconds, and smaller: its font unscaled)
        let mut b = centred(403.0, 476.5, 256.0, 21.0);
        (b.border, b.border_radius) = (UiRect::all(Val::Px(1.0)), BorderRadius::all(Val::Px(3.0 * K)));
        r.spawn((b, BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.6)), BorderColor::all(Color::srgba(0.7, 0.7, 0.7, 0.5)))).with_children(|b| {
            b.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(4.0 * K), top: Val::Px(5.0 * K), width: Val::Px(180.0 * K), ..default() }, BmText::new("Trebuchet MS11", Color::srgb(0.8, 0.82, 0.74), 1.0, 180.0 * K, Justify::Left, true), Label));
            b.spawn((Node { position_type: PositionType::Absolute, right: Val::Px(6.0 * K), top: Val::Px(3.0 * K), width: Val::Px(60.0 * K), ..default() }, BmText::new("Trebuchet MS11", Color::WHITE, K, 60.0 * K, Justify::Right, true), Left));
        });
    });
}

#[derive(Component)]
struct Zero;
#[derive(Component)]
struct SeatIcon;

/// The binoculars' pictures (`setScopeIcon`, `setSightIcon`), those of vanilla's if a mod's have none.
fn binoculars(g: &Game) -> (String, String) {
    let db = &g.session.sim.world.db;
    let get = |k: &str| db.object_template("Binoculars").and_then(|t| t.get(k)?.first().map(|v| v.trim().trim_matches('"').to_string()));
    (get("setscopeicon").unwrap_or_else(|| "binocular.tga".into()), get("setsighticon").unwrap_or_else(|| "scout_ring_128x128.tga".into()))
}

fn num(t: &Template, k: &str) -> Option<f32> {
    t.get(k)?.first()?.trim().parse().ok()
}

/// The gun's turn and rise (degrees) in the vehicle's frame: its seat's camera forward against the body's axes.
fn gun_angles(g: &Game) -> Option<(f32, f32, String)> {
    let sim = &g.session.sim;
    let i = sim.players[g.player].soldier?;
    let e = &sim.vehicles[i].entry;
    let v = &sim.vehicles[e.inside?];
    let (rows, _) = crate::play::seat_camera(v, e.position);
    let f = rows[2];
    let b = v.craft.body.rows;
    let d = |a: [f32; 3]| f[0] * a[0] + f[1] * a[1] + f[2] * a[2];
    let (x, y, z) = (d(b[0]), d(b[1]), d(b[2]));
    Some((x.atan2(z).to_degrees(), y.atan2(x.hypot(z)).to_degrees(), v.entry.seats.get(e.position)?.pco.clone()))
}

#[allow(clippy::type_complexity)]
fn update(
    g: Res<Game>,
    mut root: Query<&mut Visibility, With<Root>>,
    mut turn: Query<&mut Node, (With<TurnMark>, Without<RiseMark>)>,
    mut rise: Query<&mut Node, (With<RiseMark>, Without<TurnMark>)>,
    mut texts: Query<(&mut BmText, Option<&Label>, Option<&Left>, Option<&Zero>)>,
    mut lex: Local<Option<bytefield_formats::lexicon::Lexicon>>,
    mut icon: Query<(&mut ImageNode, &mut Visibility), (With<SeatIcon>, Without<Root>)>,
    mut cache: ResMut<crate::assets::Cache>,
    mut images: ResMut<Assets<Image>>,
) {
    if g.is_added() {
        *lex = None;
    }
    let sim = &g.session.sim;
    let mark = g.scout_view.and_then(|k| sim.projectiles.get(k)).filter(|p| p.alive());
    let shown = if mark.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in root.iter_mut() {
        if *v != shown {
            *v = shown;
        }
    }
    let Some(p) = mark else { return };
    // the vehicle's minimap icon
    let vehicle = sim.players[g.player].soldier.and_then(|i| sim.vehicles[i].entry.inside);
    let name = vehicle.and_then(|k| sim.world.db.object_template(&sim.vehicles[k].template)?.get("setminimapicon")?.last().map(|n| n.trim().trim_matches('"').to_string()));
    let handle = name.and_then(|n| cache.texture(&sim.world.vfs, &format!("menu/Texture/{}", n.rsplit_once('.').map_or(n.as_str(), |x| x.0)), &mut images));
    if let Ok((mut im, mut v)) = icon.single_mut() {
        match handle {
            Some(h) => {
                if im.image != h {
                    im.image = h;
                }
                *v = Visibility::Inherited;
            }
            None => *v = Visibility::Hidden,
        }
    }
    let lex = lex.get_or_insert_with(|| crate::lang::load(&sim.world.vfs));
    // TODO: what the bars measure is a guess (level 3): the turn from the hull, ±`DirBarXScale` over the half bar, and
    // the rise over the horizon, from −`DirBarYScaleBelow` at the foot to `DirBarYScaleAbove` at the top; the original's
    // client (not the original game) has it, and what `DirBarYScaleMin`/`Max`, `DirBarRotate` (180 on four vanilla guns) and the left yellow end
    // are for.
    if let Some((yaw, pitch, pco)) = gun_angles(&g) {
        let t = sim.world.db.object_template(&pco);
        let scale = t.and_then(|t| num(t, "dirbarxscale")).filter(|&s| s > 0.0).unwrap_or(25.0);
        let above = t.and_then(|t| num(t, "dirbaryscaleabove")).unwrap_or(40.0);
        let below = t.and_then(|t| num(t, "dirbaryscalebelow")).unwrap_or(0.0);
        let x = 400.0 + (yaw / scale).clamp(-1.0, 1.0) * BAR / 2.0;
        let span = (above + below).max(1.0);
        let y = RISE_TOP + BAR - ((pitch + below) / span).clamp(0.0, 1.0) * BAR;
        if let Ok(mut n) = turn.single_mut() {
            n.margin.left = Val::Px((x - 400.0 - 1.5) * K);
        }
        if let Ok(mut n) = rise.single_mut() {
            n.margin.top = Val::Px((y - 300.0 - 1.5) * K);
        }
    }
    let who = p.scout.as_ref().and_then(|m| sim.players.iter().position(|q| q.soldier == Some(m.player)));
    // (with no name, as the HUD's feed: «PLAYER k», «BOT k»)
    let name = who.map_or(String::new(), |k| {
        let q = &sim.players[k];
        if q.name.is_empty() { format!("{} {}", crate::lang::tr(lex, if q.ai { "BOT" } else { "PLAYER" }), k) } else { q.name.clone() }
    });
    let label = format!("{} {}:", crate::lang::tr(lex, "SCOUT"), name);
    let left = format!("{}", (p.ttl - p.age as f32).max(0.0).ceil() as i32);
    for (mut t, l, r, z) in texts.iter_mut() {
        let want = if l.is_some() {
            &label
        } else if r.is_some() {
            &left
        } else if z.is_some() {
            "0"
        } else {
            continue;
        };
        if t.text != want {
            t.text = want.to_string();
        }
    }
}
