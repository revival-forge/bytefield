//! The minimap as the original's:
//! top right, a window on the player with the level's picture (`textures/ingamemap.dds`, its
//! combat area) 20 % see-through (`game.setMinimapTransparency 20`; the original draws
//! no picture, but it has the setting and the tools call that file the minimap), north up
//! (`game.setStaticMinimap 1`); three zooms with N (`c_PIZoomMap`), and M (`c_PIMap`) for the
//! whole map (its combat area, 655 px, from the same corner). On it:
//! - the player, `Minimap/minimap_icon_ring_32x32` turned to where it faces;
//! - its team on foot (`minimap_icon_soldier_16x16`) and its team's vehicles (each its
//!   template's `setMinimapIcon`, `setMinimapIconSize`), in the team's colour, turned;
//! - the flags: the owner's (its soldiers' `setControlPointIcon`, or `conp_neutral`), a base's
//! crossed out (`baseflag_` and the same); not drawn out of the window;
//! - the square it is in (8 × 8 over the terrain: columns A–H, rows 1–8 from the north);
//! - below, a bar with a mark per flag that changes hands (`Ingame/cpbar/cpbar_*_cp_16x8`);
//! - where the seat has `sonarPos` (the destroyers' driver), the sonar.
//!
//! Sizes are the original's at 1024×768, in pixels.

use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::UiTransform;

use crate::assets::Cache;
use crate::play::Game;

/// The window's inner side (px).
const SIDE: f32 = 210.0;
/// Its corner: from the right and the top (under the tickets).
const RIGHT: f32 = 12.0;
const TOP: f32 = 45.0;
/// The pixels the whole combat area spans at each zoom (N goes to the next); the first is the one it starts in. Not
/// metres: the original scales the map to its area, whatever its size or its picture's (registered against
/// `ingamemap`: 783 on El_Alamein's 2048 m, 778 on DC_Al_Nas' 416 m and
/// DC_Basrahs_Edge's 512 m with a 1024² picture; the other two on DC_Basrahs_Edge; the farthest ±10).
const ZOOMS: [f32; 3] = [780.0, 1797.0, 338.0];
/// The icons' sides (flags, the team's, the player's) are the original's in 800×600, drawn ×1.28 as the rest of its HUD
/// at 1024×768.
const ICON: f32 = 1.28;
/// The whole map (M): the combat area at this side (655 px on El_Alamein and on DC_Basrahs_Edge), from the same corner.
const BIG_SIDE: f32 = 655.0;
/// The picture's opacity (`game.setMinimapTransparency 20`: 20 % see-through).
const OPACITY: f32 = 0.8;
/// How many icons it keeps.
const ICONS: usize = 96;
/// The squares of the grid over the terrain.
const GRID: f32 = 8.0;
/// `Game.setSonarRotationSpeed 0.025` (`Menu.con`): how far the sonar's line turns each frame (radians; the
/// client's minimap takes it off its angle every frame it draws, and its setter keeps
/// it over π as how much a contact fades each frame: half a turn).
const SONAR_SPEED: f32 = 0.025;
/// The frames a second that turn stands for here (ours are not the original's frames).
// TODO: measure how fast it turns in the original (a Fletcher's driver on Midway): 60 is a guess
const SONAR_FPS: f32 = 60.0;
/// `Submarine/sonar.tga` (128 × 128, its line from the centre) and the contacts' `Minimap/map_dot` (16 px), drawn
/// in 0.75 grey.
// (its side ×`ICON` on screen: 83 px of sweep in the original's F-15C; the dots' side not measured)
const SONAR_SIDE: f32 = 128.0;
const SONAR_DOT: f32 = 16.0;

/// The area that frames the respawn screen's map: the combat area, or the whole terrain.
pub(crate) fn area(g: &Game) -> [f32; 4] {
    let l = &g.session.sim.world.level;
    l.combat_area.map_or([0.0, 0.0, l.terrain.world_size as f32, l.terrain.world_size as f32], |a| a.map(|v| v as f32))
}

/// Where (x, z) is on the map, as fractions from its top left; None outside.
pub(crate) fn on_map(a: [f32; 4], x: f32, z: f32) -> Option<(f32, f32)> {
    let (u, v) = ((x - a[0]) / a[2], 1.0 - (z - a[1]) / a[3]);
    ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some((u, v))
}

pub(crate) fn team_color(t: u32) -> Color {
    match t {
        1 => Color::srgb(0.9, 0.25, 0.2),
        2 => Color::srgb(0.3, 0.5, 1.0),
        _ => Color::srgb(0.85, 0.85, 0.85),
    }
}

#[derive(Component)]
struct Mini;
#[derive(Component)]
struct Window;
#[derive(Component)]
struct Icon(usize);
#[derive(Component)]
struct Picture;
#[derive(Component)]
struct Square;
#[derive(Component)]
struct Bar;
#[derive(Component)]
struct BarMark(usize);

/// The sonar's screen (the client's minimap, with the player's object's `getSonarPos`): a line
/// turning clockwise round the vehicle; each object its sonar senses (`Sim::sonar_contacts`, not itself nor one
/// inside another) shows as a dot when the line passes over it, and fades out in half a turn. The dot follows its
/// object while it lasts.
#[derive(Resource, Default)]
struct Sweep {
    /// how far it has turned (radians, clockwise from north)
    angle: f32,
    /// the dots: the object and how much of it is left (1 to 0)
    dots: Vec<(usize, f32)>,
}

impl Sweep {
    /// One frame of `step` radians, with the bearings (clockwise from north) of what the sonar senses: what the line
    /// passes now (within `step` of it, as the original's) gets a dot unless it has one.
    fn turn(&mut self, step: f32, sensed: &[(usize, f32)]) {
        let tau = std::f32::consts::TAU;
        self.angle += step;
        if self.angle > tau {
            self.angle = 0.0;
        }
        let fade = step / std::f32::consts::PI;
        self.dots.retain_mut(|d| {
            d.1 -= fade;
            d.1 > 0.0
        });
        for &(j, b) in sensed {
            let off = (b - self.angle).rem_euclid(tau);
            let off = off.min(tau - off);
            if off < step && !self.dots.iter().any(|d| d.0 == j) {
                self.dots.push((j, 1.0));
            }
        }
    }
}

/// The zoom (an index in [`ZOOMS`]) and whether the whole map is up.
#[derive(Resource, Default)]
struct View {
    zoom: usize,
    big: bool,
}

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<View>().init_resource::<Sweep>().add_systems(Update, (setup.run_if(resource_added::<Game>), update.run_if(resource_exists::<Game>)).chain());
    }
}

fn pic(g: &Game, cache: &mut Cache, images: &mut Assets<Image>, name: &str) -> Option<Handle<Image>> {
    let stem = name.trim().trim_matches('"');
    let stem = stem.rsplit_once('.').map_or(stem, |s| s.0);
    cache.texture(&g.session.sim.world.vfs, &format!("menu/Texture/{stem}"), images)
}

fn setup(mut commands: Commands, g: Res<Game>, mut cache: ResMut<Cache>, mut images: ResMut<Assets<Image>>) {
    let dir = g.session.sim.world.level.terrain.heightmap.rsplit_once('/').map_or("", |d| d.0).to_string();
    let picture = g.session.sim.world.vfs.read(&format!("{dir}/textures/ingamemap.dds")).ok().and_then(|b| bytefield_formats::dds::decode(&b).ok()).map(|img| images.add(crate::assets::rgba_image(&img)));
    if picture.is_none() {
        warn!("no ingamemap for the minimap");
    }
    let frame = pic(&g, &mut cache, &mut images, "Minimap/icon_mapbar_small").map(|h| ImageNode { image: h, image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() }).unwrap_or_default();
    commands
        .spawn((Node { position_type: PositionType::Absolute, right: Val::Px(RIGHT), top: Val::Px(TOP), width: Val::Px(SIDE), height: Val::Px(SIDE), ..default() }, Mini))
        .with_children(|m| {
            m.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), overflow: Overflow::clip(), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.2 * OPACITY)),
                Window,
            ))
            .with_children(|w| {
                if let Some(h) = picture {
                    w.spawn((Node { position_type: PositionType::Absolute, ..default() }, ImageNode { image: h, color: Color::srgba(1.0, 1.0, 1.0, OPACITY), image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() }, Picture));
                }
                for k in 0..ICONS {
                    w.spawn((Node { position_type: PositionType::Absolute, ..default() }, ImageNode::default(), UiTransform::IDENTITY, Visibility::Hidden, Icon(k)));
                }
            });
            m.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(-5.0), top: Val::Px(-5.0), width: Val::Px(SIDE + 10.0), height: Val::Px(SIDE + 10.0), ..default() }, frame));
            m.spawn((
                Text::new(""),
                TextFont { font_size: FontSize::Px(18.0), ..default() },
                TextColor(Color::WHITE),
                Node { position_type: PositionType::Absolute, left: Val::Px(6.0), bottom: Val::Px(4.0), ..default() },
                Square,
            ));
        });
    // the flags' bar, under the window. Its marks as the original's on DC_Al_Nas: 16 px of mark
    // seen at x 812, 841, 870 (a 29 px pitch) and rows 271-275, the bar's outline from y 266. The 16x8 picture's mark is 13x5
    // texels, so it is stretched to 20x8 (without `Stretch` it was drawn at its own 16 px: 13 px of mark)
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, right: Val::Px(RIGHT - 3.0), top: Val::Px(TOP + SIDE + 12.0), width: Val::Px(SIDE + 6.0), height: Val::Px(12.0), padding: UiRect { left: Val::Px(13.0), right: Val::Px(8.0), top: Val::Px(4.0), bottom: Val::ZERO }, column_gap: Val::Px(9.0), ..default() },
            BackgroundColor(Color::srgba(0.15, 0.15, 0.13, 0.85)),
            Outline::new(Val::Px(1.0), Val::ZERO, Color::srgb(0.45, 0.45, 0.4)),
            Bar,
        ))
        .with_children(|b| {
            for k in 0..16 {
                b.spawn((Node { width: Val::Px(20.0), height: Val::Px(8.0), flex_shrink: 0.0, ..default() }, ImageNode { image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() }, Visibility::Hidden, BarMark(k)));
            }
        });
}

/// A template's last value for `key`, unquoted.
fn last_arg(g: &Game, template: &str, key: &str) -> Option<String> {
    let t = g.session.sim.world.db.object_template(template)?;
    t.get(key).and_then(|a| a.last()).map(|s| s.trim_matches('"').to_string())
}

/// The picture of a flag held by a team: its soldiers' `setControlPointIcon` (`conp_ger`); a
/// base's, the same crossed out (`baseflag_conp_ger`); nobody's, `conp_neutral`.
pub fn flag(g: &Game, team: u32, base: bool) -> String {
    let skin = team.checked_sub(1).and_then(|t| g.session.sim.modes.conquest.as_ref()?.teams.skins.get(t as usize).cloned().flatten());
    match skin.and_then(|s| last_arg(g, &s, "setControlPointIcon")) {
        Some(icon) if base => format!("baseflag_{icon}"),
        Some(icon) => icon,
        None => "conp_neutral".into(),
    }
}

/// The square of the grid at (x, z): `F5`. The grid is over the map's area, as the one printed
/// on its picture (Berlin's combat area is smaller than its terrain).
pub fn square(a: [f32; 4], x: f32, z: f32) -> String {
    let col = ((x - a[0]) / a[2] * GRID).floor().clamp(0.0, GRID - 1.0) as u8;
    let row = ((a[1] + a[3] - z) / a[3] * GRID).floor().clamp(0.0, GRID - 1.0) as u8;
    format!("{}{}", (b'A' + col) as char, row + 1)
}

/// The angle (radians, clockwise from north) a forward vector faces on the map.
pub(crate) fn heading(f: [f32; 3]) -> f32 {
    f[0].atan2(f[2])
}

/// One icon to draw: its picture, where (world x, z), its side, its tint and its turn.
pub(crate) struct Mark {
    pub image: Option<Handle<Image>>,
    pub x: f32,
    pub z: f32,
    pub side: f32,
    pub tint: Color,
    pub turn: f32,
}

/// What the team has on the map, the player apart: its vehicles (each its template's icon), then its soldiers on foot.
pub(crate) fn team_marks(g: &Game, team: u32, pics: &mut impl FnMut(&str) -> Option<Handle<Image>>) -> Vec<Mark> {
    let sim = &g.session.sim;
    let mut list = Vec::new();
    let soldier_icon = pics("Minimap/minimap_icon_soldier_16x16");
    // a vehicle's team: who rides it, or the team its spawners give its template
    let mut riders = std::collections::HashMap::new();
    for p in sim.players.iter().filter(|p| p.alive) {
        if let Some(k) = p.soldier.and_then(|s| sim.vehicles[s].entry.inside) {
            riders.insert(k, p.team);
        }
    }
    let spawned = |t: &str| sim.world.level.spawners.iter().flat_map(|s| &s.templates).find(|(_, n)| n.eq_ignore_ascii_case(t)).map(|(team, _)| *team);
    for (_, v) in sim.vehicles.iter().enumerate().filter(|&(k, v)| !v.removed && v.in_world() && !v.has_soldier_contact() && riders.get(&k).copied().or_else(|| spawned(&v.template)) == Some(team)) {
        let Some(icon) = last_arg(g, &v.template, "setMinimapIcon") else { continue };
        let size = last_arg(g, &v.template, "setMinimapIconSize").and_then(|s| s.parse::<f32>().ok()).unwrap_or(16.0);
        list.push(Mark { image: pics(&icon), x: v.craft.body.pos[0], z: v.craft.body.pos[2], side: size, tint: team_color(team), turn: heading(v.craft.body.rows[2]) });
    }
    for (k, p) in sim.players.iter().enumerate() {
        if k == g.player || !p.alive || p.team != team {
            continue;
        }
        let Some(v) = p.soldier.map(|s| &sim.vehicles[s]).filter(|v| v.entry.inside.is_none()) else { continue };
        list.push(Mark { image: soldier_icon.clone(), x: v.craft.body.pos[0], z: v.craft.body.pos[2], side: 10.0, tint: team_color(team), turn: heading(v.craft.body.rows[2]) });
    }
    list
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update(
    g: Res<Game>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    ui: Res<crate::respawn::RespawnUi>,
    mut view: ResMut<View>,
    mut cache: ResMut<Cache>,
    mut images: ResMut<Assets<Image>>,
    mut mini: Query<(&mut Node, &mut Visibility), (With<Mini>, Without<Icon>, Without<Bar>)>,
    mut bar: Query<&mut Visibility, (With<Bar>, Without<Mini>, Without<Icon>, Without<BarMark>)>,
    mut icons: Query<(&Icon, &mut Node, &mut ImageNode, &mut UiTransform, &mut Visibility), (Without<Mini>, Without<BarMark>)>,
    mut marks: Query<(&BarMark, &mut ImageNode, &mut Visibility), (Without<Icon>, Without<Mini>, Without<Bar>)>,
    mut square_text: Query<&mut Text, With<Square>>,
    mut picture: Query<&mut Node, (With<Picture>, Without<Icon>, Without<Mini>, Without<BarMark>, Without<Bar>)>,
    mut sweep: ResMut<Sweep>,
    time: Res<Time>,
    mut cone: Local<Option<(usize, usize, f32, bool)>>,
) {
    let controls = crate::controls::current();
    if controls.just(&keys, &buttons, "n") {
        view.zoom = (view.zoom + 1) % ZOOMS.len();
    }
    if controls.just(&keys, &buttons, "m") {
        view.big = !view.big;
    }
    let shown = if g.dead() && !ui.hidden { Visibility::Hidden } else { Visibility::Inherited };
    let sim = &g.session.sim;
    let a = area(&g);
    let me = &sim.players[g.player];
    // where it is centred: the player (driving: its vehicle), or its body when dead
    let mine = me.soldier.map(|s| {
        let v = &sim.vehicles[s];
        v.entry.inside.map_or(v, |j| &sim.vehicles[j])
    });
    let middle = [a[0] + a[2] / 2.0, a[1] + a[3] / 2.0];
    let centre = mine.map_or(middle, |v| [v.craft.body.pos[0], v.craft.body.pos[2]]);
    let (side, scale, centre) = if view.big { (BIG_SIDE, BIG_SIDE / a[2], middle) } else { (SIDE, ZOOMS[view.zoom] / a[2], centre) };
    for (mut node, mut vis) in mini.iter_mut() {
        node.width = Val::Px(side);
        node.height = Val::Px(side);
        *vis = shown;
    }
    for mut vis in bar.iter_mut() {
        *vis = if view.big { Visibility::Hidden } else { shown };
    }
    if shown == Visibility::Hidden {
        return;
    }
    for mut node in picture.iter_mut() {
        node.left = Val::Px(side / 2.0 + (a[0] - centre[0]) * scale);
        node.top = Val::Px(side / 2.0 - (a[1] + a[3] - centre[1]) * scale);
        node.width = Val::Px(a[2] * scale);
        node.height = Val::Px(a[3] * scale);
    }
    let mut pics = |n: &str| pic(&g, &mut cache, &mut images, n);
    let mut list: Vec<Mark> = Vec::new();
    // the flags (under the rest)
    // TODO: the original's order is not the file's (DC_Al_Nas: blue, grey, grey; ours, in `Object.create` order, grey, blue, grey)
    // Sorted by team (2, 1, 0) or the player's team first would both fit: a capture with a red flag tells them apart
    let mut bar_marks = Vec::new();
    if let Some(c) = sim.modes.conquest.as_ref() {
        for p in &c.points {
            let base = p.template.unable_to_change_team;
            let name = flag(&g, p.team, base);
            list.push(Mark { image: pics(&name), x: p.pos[0], z: p.pos[2], side: ICON * if base { 30.0 } else { 20.0 }, tint: Color::WHITE, turn: 0.0 });
            if !base {
                bar_marks.push(match p.team {
                    1 => "Ingame/cpbar/cpbar_red_cp_16x8",
                    2 => "Ingame/cpbar/cpbar_blue_cp_16x8",
                    _ => "Ingame/cpbar/cpbar_gray_cp_16x8",
                });
            }
        }
    }
    // its team's vehicles, and its team on foot
    list.extend(team_marks(&g, me.team, &mut pics).into_iter().map(|m| Mark { side: m.side * ICON, ..m }));
    let soldier_icon = pics("Minimap/minimap_icon_soldier_16x16");
    list.truncate(ICONS - 3);
    // in an artillery seat, the gun's cone (`artillery_minimap_camview`: its tip on the vehicle; it opens
    // the way the gun points), as the original's in the seat and from a scout's mark. Its side measured there.
    // Not until the gun has turned since the seat was taken: missing with the gun still in the mark's view
    // and in the seat, there after turning it (level 2, from the pictures).
    match me.soldier.filter(|&i| me.alive && sim.art_seat(i)).and_then(|i| Some((sim.vehicles[i].entry.inside?, sim.vehicles[i].entry.position))) {
        Some((k, seat)) => {
            let (rows, at) = crate::play::seat_camera(&sim.vehicles[k], seat);
            let turn = heading(rows[2]);
            let c = cone.get_or_insert((k, seat, turn, false));
            if (c.0, c.1) != (k, seat) {
                *c = (k, seat, turn, false);
            }
            c.3 |= (turn - c.2).abs() > 1e-3;
            if c.3 {
                // (its tip is the picture's centre: the cone fills its left half)
                list.push(Mark { image: pics("Minimap/artillery_minimap_camview_128x128"), x: at[0], z: at[2], side: 160.0, tint: Color::WHITE, turn: turn + std::f32::consts::FRAC_PI_2 });
            }
        }
        None => *cone = None,
    }
    // the sonar, round its vehicle (`sonarPos`)
    // (in the F-15C the ground's scale is the original's, 1.48 against 1.44 px a texel with `minimap-scale.py`,
    // and its sweep reaches 83 px from the centre against our 65 at 128: the picture is drawn ×`ICON`, like the icons)
    match me.soldier.filter(|&i| me.alive && sim.sonar_seat(i)).and_then(|i| sim.vehicles[i].entry.inside) {
        Some(k) => {
            let at = sim.vehicles[k].craft.body.pos;
            let sensed: Vec<(usize, f32)> = sim
                .sonar_contacts(k)
                .into_iter()
                .filter(|&(j, _)| j != k)
                .map(|(j, _)| {
                    let p = sim.vehicles[j].craft.body.pos;
                    (j, heading([p[0] - at[0], 0.0, p[2] - at[2]]).rem_euclid(std::f32::consts::TAU))
                })
                .collect();
            sweep.turn(SONAR_SPEED * SONAR_FPS * time.delta_secs(), &sensed);
            let grey = |a: f32| Color::srgba(0.75, 0.75, 0.75, a);
            // (the picture's line points west, its trail south: turned a quarter more than the angle)
            list.push(Mark { image: pics("Submarine/sonar"), x: at[0], z: at[2], side: SONAR_SIDE * ICON, tint: grey(1.0), turn: sweep.angle + std::f32::consts::FRAC_PI_2 });
            let dot = pics("Minimap/map_dot");
            for &(j, left) in &sweep.dots {
                let Some(v) = sim.vehicles.get(j).filter(|v| v.in_world()) else { continue };
                list.push(Mark { image: dot.clone(), x: v.craft.body.pos[0], z: v.craft.body.pos[2], side: SONAR_DOT, tint: grey(left), turn: 0.0 });
            }
        }
        None => *sweep = Sweep::default(),
    }
    list.truncate(ICONS - 2);
    // the player, on top
    if let Some(v) = mine.filter(|_| me.alive) {
        // its own icon in its team's colour inside the ring (the ring alone, grey, is lost on a
        // dark picture): on foot the soldier's, driving its vehicle's
        let (icon, size) = if v.has_soldier_contact() {
            (soldier_icon.clone(), 20.0)
        } else {
            let size = last_arg(&g, &v.template, "setMinimapIconSize").and_then(|s| s.parse::<f32>().ok()).unwrap_or(16.0);
            (last_arg(&g, &v.template, "setMinimapIcon").and_then(|n| pics(&n)), size * 1.25)
        };
        // (its colour is the original's: AC-130 75,117,230 against our 73,120,243, Stryker 76,121,234 and 68,115,231)
        // (the ring under it, as the original's)
        // TODO: its size: the AC-130's comes out ~1.2× the original's inside the same ring, the Stryker's about the same
        // (`*-02-F9.png` against `*-1-ours-f9.png`)
        list.push(Mark { image: pics("Minimap/minimap_icon_ring_32x32"), x: v.craft.body.pos[0], z: v.craft.body.pos[2], side: 32.0 * ICON, tint: Color::WHITE, turn: heading(v.craft.body.rows[2]) });
        list.push(Mark { image: icon, x: v.craft.body.pos[0], z: v.craft.body.pos[2], side: size * ICON, tint: team_color(me.team), turn: heading(v.craft.body.rows[2]) });
    }
    for (icon, mut node, mut im, mut t, mut vis) in icons.iter_mut() {
        let Some(m) = list.get(icon.0) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let (px, py) = (side / 2.0 + (m.x - centre[0]) * scale, side / 2.0 - (m.z - centre[1]) * scale);
        let half = m.side / 2.0;
        // (flags too: out of the window they are not drawn: the original keeps none on its edge:
        // it has been seen keeping them there)
        if !(-half..side + half).contains(&px) || !(-half..side + half).contains(&py) {
            *vis = Visibility::Hidden;
            continue;
        }
        node.left = Val::Px(px - half);
        node.top = Val::Px(py - half);
        node.width = Val::Px(m.side);
        node.height = Val::Px(m.side);
        if let Some(h) = &m.image {
            if im.image != *h {
                im.image = h.clone();
            }
        }
        im.color = m.tint;
        t.rotation = Rot2::radians(m.turn);
        *vis = if m.image.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (bm, mut im, mut vis) in marks.iter_mut() {
        match bar_marks.get(bm.0).and_then(|n| pics(n)) {
            Some(h) => {
                im.image = h;
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for mut text in square_text.iter_mut() {
        text.0 = if view.big { String::new() } else { square(a, centre[0], centre[1]) };
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_squares_are_the_originals() {
        // the player at the allies' base of El Alamein was in F5
        let all = [0.0, 0.0, 2048.0, 2048.0];
        assert_eq!(super::square(all, 1516.0, 858.0), "F5");
        assert_eq!(super::square(all, 0.0, 2048.0), "A1");
        assert_eq!(super::square(all, 2047.0, 0.0), "H8");
        // over a smaller area, its own squares
        assert_eq!(super::square([1024.0, 512.0, 512.0, 1024.0], 1300.0, 1000.0), "E5");
    }

    #[test]
    fn the_sonar_shows_what_its_line_passes_and_fades_it_in_half_a_turn() {
        let mut s = super::Sweep::default();
        let step = 0.025;
        // something east (π/2): no dot until the line gets there, about 63 frames
        let mut frames = 0;
        while s.dots.is_empty() {
            s.turn(step, &[(7, std::f32::consts::FRAC_PI_2)]);
            frames += 1;
            assert!(frames < 100);
        }
        assert!((61..=64).contains(&frames), "{frames}");
        // it fades in half a turn (π / 0.025 ≈ 126 frames), though the line passes it no more
        let mut more = 0;
        while !s.dots.is_empty() {
            s.turn(step, &[]);
            more += 1;
        }
        assert!((124..=127).contains(&more), "{more}");
        // a whole turn goes back to 0
        let mut s = super::Sweep { angle: 6.28, dots: Vec::new() };
        s.turn(step, &[]);
        assert_eq!(s.angle, 0.0);
    }
}
