//! The original's gauges with its pictures (`menu/Texture/…`): the kit's health bar (bottom left), the ammunition panel with the weapon's ammo icon and its rounds
//! (bottom right), and the bar of the kit's weapons, one slot per `itemIndex`, for a while after
//! the weapon in hand changes (bottom middle). In a vehicle, its own (`vehicle`). Their pictures come from the kit (`setHealthBarIcon`,
//! `setHealthBarFullIcon`, `addWeaponIcon`: the n-th is slot n + 1) and the weapon (`setAmmoIcon`).
//! Also the soldier's pose (`setSoldierStandingIcon`…, by the health bar), the tickets (top right,
//! each side's `setTicketIcon`) and the row of the radio's menus (top left, `Radio/Icon_f*`).
//!
//! Where: the original's 800×600 places (`menu/InGame`, `menu/RadioMenu`, and measured on
//! the original's capture at 1024×768, ×1.28), at that scale and from the screen's corners, as the
//! minimap.

use bevy::prelude::*;
use bevy::text::FontSize;
use bytefield_con::templates::Template;

use crate::assets::Cache;
use crate::play::Game;

/// Screen pixels per pixel of the original's 800×600 (its HUD at 1024×768).
pub(crate) const K: f32 = 1.28;
/// The pictures' side on the screen (they are 64 texels).
const SIDE: f32 = 64.0 * K;
/// The numbers' boxes' tops (the original's 800×600 pixels), measured against the original's screen.
const AMMO_TOP: f32 = 1.0;
const AMMO_LEFT: f32 = 1.2;
const TICKETS_TOP: f32 = 4.6;
const MAGS_TOP: f32 = 47.0;
const VEHICLE_ROUNDS_DY: f32 = 0.3;

/// The generic ammunition bar's height on the screen.
fn bar_h_px() -> f32 {
    43.0 * K
}

/// Where a weapon's magazine goes in the ammunition panel (left, top, width, height in the original's 800×600 pixels), at
/// the default `ammoBarPosX/Y` (5, −11): measured on the original's BAR. Level 3.
const MAG: (f32, f32, f32, f32) = (5.0, -4.6, 32.0, 64.0);
/// How long the weapon bar stays after a change (s): 20 frames at 20 a second in the original (DC_Final, key 2 with
/// the sniper's kit; `BAR=4`), ±0.05 s. Until then 2.5, unmeasured.
const BAR_TIME: f32 = 1.0;
const SLOTS: usize = 6;

#[derive(Component)]
struct HealthEmpty;
#[derive(Component)]
struct HealthClip;
#[derive(Component)]
struct HealthFull;
#[derive(Component)]
struct AmmoRoot;
#[derive(Component)]
struct AmmoClip;
#[derive(Component)]
struct AmmoIcon;
/// The side bar on foot: heat (true) or reload, and its clip.
#[derive(Component)]
struct FootSide(bool);
#[derive(Component)]
struct FootSideClip(bool);

/// Where an `ATIcon…`'s icon and its side bar go in the ammunition panel (the original's 800×600 pixels from the panel's corner),
/// measured on the original's grenade and medpack. Level 2.
const ICON: (f32, f32) = (2.0, 26.0);
const SIDE_BAR: (f32, f32) = (45.2, -0.7);

/// The ammunition panel by `setHudAmmoType`: `ATAmmoBar` the
/// magazine; `ATIcon…` the icon at its size instead, with a side bar of heat (`…HeatBar`, `…StrengthBar`) or reload
/// (`…ReloadBar`); `ATNone`, and no type at all (the binoculars), no panel.
#[derive(Clone, Copy, PartialEq, Debug)]
enum HudAmmo {
    None,
    Bar,
    Icon,
    /// the side bar: the ammunition left (`…HeatBar`, the medpack: full and without numbers in the original), the
    /// heat (`…StrengthBar`, the grenade's throw) or the reload (`…ReloadBar`)
    IconSide(Side),
}
#[derive(Clone, Copy, PartialEq, Debug)]
enum Side {
    Ammo,
    Heat,
    Reload,
}

fn hud_ammo(kind: &str) -> HudAmmo {
    match kind.to_ascii_lowercase().as_str() {
        "atammobar" => HudAmmo::Bar,
        "aticon" => HudAmmo::Icon,
        "aticonandheatbar" => HudAmmo::IconSide(Side::Ammo),
        "aticonandstrengthbar" => HudAmmo::IconSide(Side::Heat),
        "aticonandreloadbar" => HudAmmo::IconSide(Side::Reload),
        _ => HudAmmo::None,
    }
}

/// The ammunition's generic bar (`ammobar_*`), and a weapon's own magazine (`setAmmoBar`, `setAmmoBarFill`) in its place.
#[derive(Component)]
struct AmmoBar;
#[derive(Component)]
struct MagEmpty;
#[derive(Component)]
struct MagFull;
#[derive(Component)]
struct MagClip;
#[derive(Component)]
struct AmmoText;
#[derive(Component)]
struct AmmoMags;
#[derive(Component)]
struct Stance;
#[derive(Component)]
struct TicketFlag(usize);
#[derive(Component)]
struct TicketText(usize);
#[derive(Component)]
struct RadioRow;
#[derive(Component)]
struct WeaponBar;
#[derive(Component)]
struct Slot(usize);
#[derive(Component)]
struct SlotIcon(usize);

/// What the pictures were made for, and the weapon bar's clock.
#[derive(Default)]
struct Seen {
    kit: Option<String>,
    ammo_icon: Option<String>,
    /// the weapon whose magazine pictures are up
    mag: Option<String>,
    stance: Option<String>,
    flags: [Option<String>; 2],
    weapon: Option<i32>,
    /// (the simulation's clock, so a scripted capture sees what a player would)
    bar_until: f64,
}

pub struct GaugesPlugin;

impl Plugin for GaugesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, ((setup, vehicle_setup).run_if(resource_added::<Game>), (update, foot_side, top, vehicle, supply).run_if(resource_exists::<Game>)).chain());
    }
}

pub(crate) fn picture(g: &Game, cache: &mut Cache, images: &mut Assets<Image>, name: &str) -> Option<Handle<Image>> {
    let stem = name.trim().trim_matches('"');
    let stem = stem.rsplit_once('.').map_or(stem, |s| s.0);
    let h = cache.texture(&g.session.sim.world.vfs, &format!("menu/Texture/{stem}"), images)?;
    // the HUD's pictures do not tile: with the cache's repeating sampler the opaque left column of
    // `Heatbar_empty_32x64` bled into its transparent right one, a thin dark line beside the ammo box
    // (at x 982)
    if let Some(mut i) = images.get_mut(&h)
        && let bevy::image::ImageSampler::Descriptor(d) = &mut i.sampler
    {
        d.address_mode_u = bevy::image::ImageAddressMode::ClampToEdge;
        d.address_mode_v = bevy::image::ImageAddressMode::ClampToEdge;
    }
    Some(h)
}

/// A vehicle's picture or a weapon panel's icon named but with no file: grey 75/255 and opaque in the original, as a mesh's
/// (`assets.rs`), not the white of an empty image. Measured on the 155 howitzer of DC (`Vehicle/Icon_pak40.tga` and
/// `Ammo/Icon_cannon_155.tga`, in no archive): 76/72/74 over sand and over sky. Level 3:
/// only these two seen.
// TODO: the other pictures of the HUD with no file (the foot's `ammoIcon`, the health bars) still hide or go white: unmeasured.
fn picture_or_grey(g: &Game, cache: &mut Cache, images: &mut Assets<Image>, name: &str) -> Handle<Image> {
    picture(g, cache, images, name).unwrap_or_else(|| {
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let size = Extent3d { width: 1, height: 1, depth_or_array_layers: 1 };
        images.add(Image::new_fill(size, TextureDimension::D2, &[75, 75, 75, 255], TextureFormat::Rgba8UnormSrgb, bevy::asset::RenderAssetUsages::default()))
    })
}

fn setup(mut commands: Commands, g: Res<Game>, mut cache: ResMut<Cache>, mut images: ResMut<Assets<Image>>) {
    let mut pic = |n: &str| picture(&g, &mut cache, &mut images, n).map(ImageNode::new).unwrap_or_default();
    // health: the empty bar, and the full one over it cut from the top
    commands
        .spawn((Node { position_type: PositionType::Absolute, left: Val::Px(48.0 * K), bottom: Val::Px(11.0 * K), width: Val::Px(SIDE), height: Val::Px(SIDE), ..default() }, ImageNode::default(), HealthEmpty))
        .with_children(|b| {
            b.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Px(SIDE), height: Val::Px(SIDE), overflow: Overflow::clip(), ..default() }, HealthClip))
                .with_children(|c| {
                    c.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Px(SIDE), height: Val::Px(SIDE), ..default() }, ImageNode::default(), HealthFull));
                });
        });
    // the pose, right of the health bar
    commands.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(90.0 * K), bottom: Val::Px(12.0 * K), width: Val::Px(SIDE), height: Val::Px(SIDE), ..default() }, ImageNode::default(), Visibility::Hidden, Stance));
    // ammunition: the panel; in its tall slot the bar (or the weapon's ammo icon), the rounds at
    // its top right, the magazines left in its foot
    let (bar_w, bar_h) = (27.0 * K, bar_h_px());
    commands
        .spawn((Node { position_type: PositionType::Absolute, right: Val::Px(45.0 * K), bottom: Val::Px(9.0 * K), width: Val::Px(SIDE), height: Val::Px(SIDE), ..default() }, pic("Ingame/ammobar_soldier_panel_64x64"), AmmoRoot))
        .with_children(|b| {
            // a weapon's magazine: its 32×64 pictures as they are, where the original has its BAR's; level 3
            b.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(MAG.0 * K), top: Val::Px(MAG.1 * K), width: Val::Px(MAG.2 * K), height: Val::Px(MAG.3 * K), ..default() }, ImageNode::default(), Visibility::Hidden, MagEmpty))
                .with_children(|e| {
                    e.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Px(MAG.2 * K), height: Val::Px(MAG.3 * K), overflow: Overflow::clip(), ..default() }, MagClip))
                        .with_children(|c| {
                            c.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Px(MAG.2 * K), height: Val::Px(MAG.3 * K), ..default() }, ImageNode::default(), MagFull));
                        });
                });
            b.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(5.0 * K), top: Val::Px(14.0 * K), width: Val::Px(bar_w), height: Val::Px(bar_h), ..default() }, pic("Ingame/ammobar_empty_32x64"), AmmoBar))
                .with_children(|e| {
                    e.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Px(bar_w), height: Val::Px(bar_h), overflow: Overflow::clip(), ..default() }, AmmoClip))
                        .with_children(|c| {
                            c.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Px(bar_w), height: Val::Px(bar_h), ..default() }, pic("Ingame/ammobar_full_32x64")));
                        });
                });
            // (`setAmmoIcon` of an `ATIcon…`: its 64×32 at its own size, the grenade's top at 26 and left at 2 of the panel in the
            // original; it was once 40×20 at 20)
            b.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(ICON.0 * K), top: Val::Px(ICON.1 * K), width: Val::Px(64.0 * K), height: Val::Px(32.0 * K), ..default() }, ImageNode::default(), Visibility::Hidden, AmmoIcon));
            // the side bar of an `ATIconAndHeatBar`/`…StrengthBar` (heat) or `…ReloadBar` (reload), right of the box, its 32×64 at its own size
            for heat in [true, false] {
                let (e, f) = if heat { ("Ingame/Heatbar_empty_32x64", "Ingame/Heatbar_full_32x64") } else { ("Ingame/ReloadTimebar_empty_32x64", "Ingame/ReloadTimebar_full_32x64") };
                let at = Node { position_type: PositionType::Absolute, left: Val::Px(SIDE_BAR.0 * K), top: Val::Px(SIDE_BAR.1 * K), width: Val::Px(32.0 * K), height: Val::Px(64.0 * K), ..default() };
                let (w, h) = (at.width, at.height);
                b.spawn((at, pic(e), Visibility::Hidden, FootSide(heat))).with_children(|e| {
                    e.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: w, height: h, overflow: Overflow::clip(), ..default() }, FootSideClip(heat)))
                        .with_children(|c| {
                            c.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: w, height: h, ..default() }, pic(f)));
                        });
                });
            }
            // (the original's: Trebuchet MS 11 with a dark rim, `bmfont`)
            b.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(AMMO_LEFT * K), top: Val::Px(AMMO_TOP * K), width: Val::Px(40.0 * K), ..default() },
                crate::bmfont::BmText::new("Trebuchet MS11", Color::WHITE, K, 40.0 * K, Justify::Right, true),
                AmmoText,
            ));
            b.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(44.0 * K), top: Val::Px(MAGS_TOP * K), width: Val::Px(16.0 * K), ..default() },
                crate::bmfont::BmText::new("standard6", Color::BLACK, K, 16.0 * K, Justify::Center, false),
                AmmoMags,
            ));
        });
    // the tickets: the bar, then each side's flag and count (the Allies left, the Axis right)
    commands.spawn((Node { position_type: PositionType::Absolute, right: Val::Px(-76.0 * K), top: Val::Px(4.0 * K), width: Val::Px(256.0 * K), height: Val::Px(32.0 * K), ..default() }, pic("icon_ticketbar")));
    for (k, (flag, count)) in [(159.0, 117.0), (51.0, 10.0)].into_iter().enumerate() {
        let team = 2 - k;
        commands.spawn((Node { position_type: PositionType::Absolute, right: Val::Px(flag * K), top: Val::Px(9.0 * K), width: Val::Px(16.0 * K), height: Val::Px(16.0 * K), ..default() }, ImageNode::default(), TicketFlag(team)));
        commands.spawn((
            Node { position_type: PositionType::Absolute, right: Val::Px(count * K), top: Val::Px(TICKETS_TOP * K), width: Val::Px(60.0 * K), ..default() },
            crate::bmfont::BmText::new("Trebuchet MS14", crate::minimap::team_color(team as u32), K, 60.0 * K, Justify::Right, true),
            TicketText(team),
        ));
    }
    // the radio's menus, F1–F8 (`RadioMenu`: two rows of four, 65 apart, at 30 and 330)
    commands
        .spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Px(800.0 * K), height: Val::Px(40.0 * K), ..default() }, Visibility::Hidden, RadioRow))
        .with_children(|r| {
            for f in 0..8 {
                let x = if f < 4 { 30.0 + 65.0 * f as f32 } else { 330.0 + 65.0 * (f - 4) as f32 };
                r.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(x * K), top: Val::Px(3.0 * K), width: Val::Px(64.0 * K), height: Val::Px(32.0 * K), ..default() }, pic(&format!("Radio/Icon_f{}", f + 1))))
                    .with_children(|i| {
                        i.spawn((Text::new(format!("F{}", f + 1)), TextFont { font_size: FontSize::Px(9.0 * K), ..default() }, TextColor(Color::WHITE), Node { position_type: PositionType::Absolute, left: Val::Px(4.0 * K), top: Val::Px(6.0 * K), ..default() }));
                    });
            }
        });
    // the weapon bar: its picture (six cells of 64 texels in the first 384 of 512), a slot per item index
    let (slot, h) = (SIDE, SIDE);
    let w = slot * 8.0;
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), margin: UiRect::left(Val::Px(-slot * 3.0)), bottom: Val::Px(52.0), width: Val::Px(w), height: Val::Px(h), ..default() },
            pic("ingame_weaponbar_512x64"),
            Visibility::Hidden,
            WeaponBar,
        ))
        .with_children(|b| {
            for k in 0..SLOTS {
                b.spawn((
                    Node { position_type: PositionType::Absolute, left: Val::Px(k as f32 * slot), top: Val::Px(0.0), width: Val::Px(slot), height: Val::Px(h), justify_content: JustifyContent::Center, ..default() },
                    BackgroundColor(Color::NONE),
                    Slot(k),
                ))
                .with_children(|s| {
                    s.spawn((Node { width: Val::Px(h), height: Val::Px(h), ..default() }, ImageNode::default(), SlotIcon(k)));
                });
            }
        });
}

/// The kit template of the player's kit (its team's, by `game.setKit` index).
fn kit<'a>(g: &'a Game) -> Option<&'a Template> {
    let sim = &g.session.sim;
    let p = sim.players.get(g.player)?;
    let c = sim.modes.conquest.as_ref()?;
    let name = c.teams.kits.get(p.team.checked_sub(1)? as usize)?.get(p.kit)?.as_ref()?;
    sim.world.db.object_template(name)
}

fn arg(t: &Template, key: &str) -> Option<String> {
    t.get(key).and_then(|a| a.first()).map(|s| s.trim_matches('"').to_string())
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
/// The ammunition panel's two ways of showing a magazine: the generic bar or the weapon's own.
#[derive(bevy::ecs::system::SystemParam)]
struct Mags<'w, 's> {
    bar: Query<'w, 's, &'static mut Visibility, (With<AmmoBar>, Without<MagEmpty>, Without<AmmoRoot>, Without<WeaponBar>, Without<HealthEmpty>, Without<AmmoIcon>, Without<SlotIcon>)>,
    empty: Query<'w, 's, (&'static mut ImageNode, &'static mut Visibility, &'static mut Node), (With<MagEmpty>, Without<MagClip>, Without<AmmoBar>, Without<MagFull>, Without<AmmoRoot>, Without<WeaponBar>, Without<HealthEmpty>, Without<HealthFull>, Without<AmmoIcon>, Without<SlotIcon>)>,
    full: Query<'w, 's, &'static mut ImageNode, (With<MagFull>, Without<MagEmpty>, Without<HealthEmpty>, Without<HealthFull>, Without<AmmoIcon>, Without<SlotIcon>)>,
    clip: Query<'w, 's, &'static mut Node, (With<MagClip>, Without<MagEmpty>, Without<HealthClip>, Without<AmmoClip>)>,
}

fn update(
    g: Res<Game>,
    mut cache: ResMut<Cache>,
    mut images: ResMut<Assets<Image>>,
    mut seen: Local<Seen>,
    mut health: Query<(&mut ImageNode, &mut Visibility), (With<HealthEmpty>, Without<HealthFull>, Without<AmmoIcon>, Without<SlotIcon>)>,
    mut full: Query<&mut ImageNode, (With<HealthFull>, Without<AmmoIcon>, Without<SlotIcon>)>,
    mut clips: ParamSet<(Query<&mut Node, (With<HealthClip>, Without<MagEmpty>, Without<MagClip>)>, Query<&mut Node, (With<AmmoClip>, Without<MagEmpty>, Without<MagClip>)>)>,
    mut ammo: Query<&mut Visibility, (With<AmmoRoot>, Without<HealthEmpty>, Without<WeaponBar>)>,
    mut ammo_icon: Query<(&mut ImageNode, &mut Visibility), (With<AmmoIcon>, Without<SlotIcon>, Without<HealthEmpty>, Without<AmmoRoot>, Without<WeaponBar>)>,
    mut ammo_text: Query<&mut crate::bmfont::BmText, (With<AmmoText>, Without<AmmoMags>)>,
    mut ammo_mags: Query<&mut crate::bmfont::BmText, (With<AmmoMags>, Without<AmmoText>)>,
    mut bar: Query<&mut Visibility, (With<WeaponBar>, Without<HealthEmpty>, Without<AmmoRoot>)>,
    mut slots: Query<(&mut BackgroundColor, &Slot)>,
    mut slot_icons: Query<(&mut ImageNode, &mut Visibility, &SlotIcon), (Without<HealthEmpty>, Without<AmmoRoot>, Without<WeaponBar>, Without<AmmoIcon>)>,
    mut mag_ui: Mags,
) {
    if g.is_added() {
        *seen = Seen::default();
    }
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let soldier = p.soldier.filter(|_| p.alive).map(|i| &sim.vehicles[i]);
    let kit = kit(&g);
    // a new kit: its pictures
    let kit_name = kit.map(|k| k.name.clone());
    if kit_name != seen.kit {
        seen.kit = kit_name;
        let get = |key: &str, cache: &mut Cache, images: &mut Assets<Image>| kit.and_then(|k| arg(k, key)).and_then(|n| picture(&g, cache, images, &n));
        if let Ok((mut i, _)) = health.single_mut() {
            i.image = get("healthBarIcon", &mut cache, &mut images).unwrap_or_default();
        }
        if let Ok(mut i) = full.single_mut() {
            i.image = get("healthBarFullIcon", &mut cache, &mut images).unwrap_or_default();
        }
        let icons: Vec<String> = kit.map_or(Vec::new(), |k| k.all("addWeaponIcon").iter().filter_map(|a| a.first()).map(|s| s.trim_matches('"').to_string()).collect());
        for (mut i, mut v, s) in slot_icons.iter_mut() {
            let pic = icons.get(s.0).and_then(|n| picture(&g, &mut cache, &mut images, n));
            *v = if pic.is_some() { Visibility::Inherited } else { Visibility::Hidden };
            i.image = pic.unwrap_or_default();
        }
    }
    // health: the soldier's, in a vehicle too (the original keeps it, beside the vehicle's panel)
    let hp = soldier.and_then(|v| v.armor.as_ref()).map(|a| (a.hp / a.max_hp).clamp(0.0, 1.0));
    if let Ok((_, mut v)) = health.single_mut() {
        let want = if hp.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if let Some(f) = hp
        && let Ok(mut n) = clips.p0().single_mut()
    {
        let h = Val::Px(SIDE * f);
        if n.height != h {
            n.height = h;
        }
    }
    // the weapon in hand (on foot)
    let weapon = soldier.filter(|s| s.entry.inside.is_none()).and_then(|s| s.soldier()).and_then(|m| m.weapon.as_ref());
    let hud = hud_ammo(&weapon.and_then(|w| sim.world.db.object_template(&w.template.name)).and_then(|x| arg(x, "setHudAmmoType")).unwrap_or_default());
    let icon_type = matches!(hud, HudAmmo::Icon | HudAmmo::IconSide(_));
    if let Ok(mut v) = ammo.single_mut() {
        let want = if weapon.is_some() && hud != HudAmmo::None { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if let Some(w) = weapon {
        let t = &w.template;
        let mag = w.mags.first().copied().unwrap_or(0);
        let mags = w.mags.iter().skip(1).filter(|&&m| m > 0).count();
        let f = if t.mag_size > 0 { (mag as f32 / t.mag_size as f32).clamp(0.0, 1.0) } else { 1.0 };
        // the weapon's own magazine (`setHudAmmoType ATAmmoBar` with `setAmmoBar`/`setAmmoBarFill`): its full picture
        // shown from the foot up to `setAmmoBarSize` of its 64 rows
        let x = sim.world.db.object_template(&t.name);
        let own = x.and_then(|x| Some((arg(x, "setAmmoBar")?, arg(x, "setAmmoBarFill")?, x.get("setAmmoBarSize").and_then(|a| a.first()).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(64.0))));
        if seen.mag.as_deref() != Some(t.name.as_str()) {
            seen.mag = Some(t.name.clone());
            let pics = own.as_ref().and_then(|(e, f, _)| Some((picture(&g, &mut cache, &mut images, e)?, picture(&g, &mut cache, &mut images, f)?)));
            // `ammoBarPosX/Y` (only so spelt: the original does not know the data's usual `setAmomBarPos*`, so most weapons
            // keep the default, which those lines say again: 5, −11)
            let pos = |k: &str, d: f32| x.and_then(|x| x.get(k)).and_then(|a| a.first()).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d);
            let (px, py) = (pos("setAmmoBarPosX", 5.0), pos("setAmmoBarPosY", -11.0));
            if let Ok((mut i, mut v, mut n)) = mag_ui.empty.single_mut() {
                *v = if pics.is_some() && !icon_type { Visibility::Inherited } else { Visibility::Hidden };
                i.image = pics.as_ref().map(|p| p.0.clone()).unwrap_or_default();
                n.left = Val::Px((MAG.0 + px - 5.0) * K);
                n.top = Val::Px((MAG.1 + py + 11.0) * K);
            }
            if let Ok(mut i) = mag_ui.full.single_mut() {
                i.image = pics.as_ref().map(|p| p.1.clone()).unwrap_or_default();
            }
            if let Ok(mut v) = mag_ui.bar.single_mut() {
                *v = if pics.is_some() || icon_type { Visibility::Hidden } else { Visibility::Inherited };
            }
        }
        if let Some((_, _, size)) = own
            && let Ok(mut n) = mag_ui.clip.single_mut()
        {
            let h = Val::Px(MAG.3 * K * f * (size / 64.0).clamp(0.0, 1.0));
            if n.height != h {
                n.height = h;
            }
        }
        if let Ok(mut n) = clips.p1().single_mut() {
            // (the bar's own height: with the panel's, it looked full down to two thirds)
            let h = Val::Px(bar_h_px() * f);
            if n.height != h {
                n.height = h;
            }
        }
        if let Ok(mut text) = ammo_text.single_mut() {
            // (the medpack's 1800 is not shown)
            let s = if matches!(hud, HudAmmo::IconSide(Side::Ammo | Side::Reload)) { String::new() } else { format!("{mag}") };
            if text.text != s {
                text.text = s;
            }
        }
        if let Ok(mut text) = ammo_mags.single_mut() {
            // (none with `ammoType 2`: the grenades, mines, C4; the mortar's (1) shows its 0, the SMAW's (3) its 4)
            let thrown = sim.world.db.object_template(&t.name).and_then(|x| arg(x, "ammoType")).is_some_and(|v| v.trim() == "2");
            // (nor with the medpack's and the repair pack's side bars, which show no numbers at all)
            let s = if t.num_of_mag < 0 || thrown || matches!(hud, HudAmmo::IconSide(Side::Ammo | Side::Reload)) { String::new() } else { format!("{mags}") };
            if text.text != s {
                text.text = s;
            }
        }
        // its ammo icon (`setAmmoIcon`: grenades, kits…) in the bar's place, with an `ATIcon…`; the rest keep the bar
        // (the original picks a `magbar_*` per weapon)
        let icon = if icon_type { sim.world.db.object_template(&t.name).and_then(|x| arg(x, "ammoIcon")).unwrap_or_default() } else { String::new() };
        if seen.ammo_icon.as_deref() != Some(icon.as_str()) {
            if let Ok((mut i, mut v)) = ammo_icon.single_mut() {
                let pic = (!icon.is_empty()).then(|| picture(&g, &mut cache, &mut images, &icon)).flatten();
                *v = if pic.is_some() { Visibility::Inherited } else { Visibility::Hidden };
                i.image = pic.unwrap_or_default();
            }
            seen.ammo_icon = Some(icon);
        }
    }
    // the weapon bar, for a while after a change
    let slot = weapon.map(|w| w.template.item_index);
    if slot != seen.weapon {
        if slot.is_some() && seen.weapon.is_some() {
            seen.bar_until = sim.clock.time + BAR_TIME as f64;
        }
        seen.weapon = slot;
    }
    if let Ok(mut v) = bar.single_mut() {
        let want = if sim.clock.time < seen.bar_until { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    for (mut bg, s) in slots.iter_mut() {
        let want = if slot == Some(s.0 as i32 + 1) { Color::srgba(1.0, 0.85, 0.4, 0.35) } else { Color::NONE };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

/// The soldier's pose picture, the tickets and the radio's row.
#[allow(clippy::type_complexity)]
fn top(
    g: Res<Game>,
    mut cache: ResMut<Cache>,
    mut images: ResMut<Assets<Image>>,
    mut seen: Local<Seen>,
    mut stance: Query<(&mut ImageNode, &mut Visibility), (With<Stance>, Without<TicketFlag>, Without<RadioRow>)>,
    mut flags: Query<(&mut ImageNode, &TicketFlag), Without<Stance>>,
    mut counts: Query<(&mut crate::bmfont::BmText, &TicketText)>,
    mut radio: Query<&mut Visibility, (With<RadioRow>, Without<Stance>)>,
) {
    if g.is_added() {
        *seen = Seen::default();
    }
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let template = |name: &str| sim.world.db.object_template(name);
    // the pose: standing, crouching or lying (`getPose`), in a vehicle too
    let soldier = p.soldier.filter(|_| p.alive).map(|i| &sim.vehicles[i]);
    let icon = soldier.and_then(|s| {
        let key = match s.soldier()?.pose() {
            0 => "soldierStandingIcon",
            1 => "soldierCrouchIcon",
            _ => "soldierProneIcon",
        };
        template(&s.template).and_then(|t| arg(t, key))
    });
    if icon != seen.stance {
        if let Ok((mut i, mut v)) = stance.single_mut() {
            let pic = icon.as_deref().and_then(|n| picture(&g, &mut cache, &mut images, n));
            *v = if pic.is_some() { Visibility::Inherited } else { Visibility::Hidden };
            i.image = pic.unwrap_or_default();
        }
        seen.stance = icon;
    }
    // the tickets: each side's soldier's `setTicketIcon`
    let Some(c) = sim.modes.conquest.as_ref() else { return };
    for team in 1..=2usize {
        let icon = c.teams.skins[team - 1].as_deref().and_then(template).and_then(|t| arg(t, "ticketIcon"));
        if icon != seen.flags[team - 1] {
            for (mut i, f) in flags.iter_mut() {
                if f.0 == team {
                    i.image = icon.as_deref().and_then(|n| picture(&g, &mut cache, &mut images, n)).unwrap_or_default();
                }
            }
            seen.flags[team - 1] = icon;
        }
    }
    // (in CTF, the captures)
    for (mut text, t) in counts.iter_mut() {
        let s = match sim.modes.ctf.as_ref() {
            Some(f) => format!("{}", f.captures[t.0 - 1]),
            None => format!("{}", c.tickets[t.0 - 1].max(0)),
        };
        if text.text != s {
            text.text = s;
        }
    }
    if let Ok(mut v) = radio.single_mut() {
        // (not over the screen when dead: the original's has none)
        let want = if g.radio_icons && !g.dead() { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

/// The gauges in a vehicle: bottom left
/// the vehicle's picture (`setVehicleIcon`, 128 texels at (200, 462)) with a mark on each seat
/// taken (`setVehicleIconPos` of the seat's PlayerControlObject) and its health bar at (174, 525);
/// bottom right a panel per weapon of the seat (`setNumberOfWeaponIcons`: one at x 690, two at
/// 590 and 700, y 534), with its ammo icon (`setPrimaryAmmoIcon`/`setSecondaryAmmoIcon`) and the
/// bars its `setPrimaryAmmoBar`/`setSecondaryAmmoBar` asks for; bottom middle the turret's
/// direction (`setHasTurretIcon`: the hull turning under the gun, at (400, 540)).
/// Only in the first person: from outside the original has none.
#[derive(Component, Clone, Copy, PartialEq)]
enum V {
    Picture,
    Health,
    HealthClip,
    /// the engine's heat bar (`setHeatBarType`), left of the health bar
    Heat,
    HeatClip,
    Dot(usize),
    /// the panel `g` (0: the only one, 1: the primary of two, 2: the secondary), and its pieces
    Panel(usize),
    Icon(usize),
    Ammo(usize),
    AmmoClip(usize),
    Rounds(usize),
    /// a heat (true) or reload bar, near the panel (alone) or far (beside the ammo bar)
    Extra(usize, bool, bool),
    ExtraClip(usize, bool, bool),
    Turret,
    Hull,
}

/// The panels' origins (x; y is 534) and how many seats can have a mark. The primary of two at 580, not the 590 read before:
/// measured on the original's T34.
const PANEL_X: [f32; 3] = [690.0, 580.0, 700.0];
const DOTS: usize = 6;
/// A seat's mark (`icon_vehicledot_*`), in the original's pixels.
const DOT: f32 = 10.0;

/// A node at the original's (x, y, w, h), from the screen's nearest side.
pub(crate) fn place(x: f32, y: f32, w: f32, h: f32) -> Node {
    let mut n = Node { position_type: PositionType::Absolute, bottom: Val::Px((600.0 - y - h) * K), width: Val::Px(w * K), height: Val::Px(h * K), ..default() };
    if x + w / 2.0 < 400.0 {
        n.left = Val::Px(x * K);
    } else {
        n.right = Val::Px((800.0 - x - w) * K);
    }
    n
}

/// An empty bar with the full one over it, cut from the top by its clip.
fn fill_bar(commands: &mut Commands, at: Node, empty: ImageNode, full: ImageNode, piece: V, clip: V) {
    let (w, h) = (at.width, at.height);
    commands.spawn((at, empty, Visibility::Hidden, piece)).with_children(|b| {
        b.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: w, height: h, overflow: Overflow::clip(), ..default() }, clip)).with_children(|c| {
            c.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), bottom: Val::Px(0.0), width: w, height: h, ..default() }, full));
        });
    });
}

fn vehicle_setup(mut commands: Commands, g: Res<Game>, mut cache: ResMut<Cache>, mut images: ResMut<Assets<Image>>) {
    let mut pic = |n: &str| picture(&g, &mut cache, &mut images, n).map(ImageNode::new).unwrap_or_default();
    commands.spawn((place(200.0, 462.0, 128.0, 128.0), ImageNode::default(), Visibility::Hidden, V::Picture));
    for i in 0..DOTS {
        commands.spawn((place(0.0, 0.0, DOT, DOT), ImageNode::default(), Visibility::Hidden, V::Dot(i)));
    }
    let (e, f) = (pic("Ingame/Vehicle_healthbar_empty_32x64"), pic("Ingame/Vehicle_healthbar_full_32x64"));
    fill_bar(&mut commands, place(174.0, 525.0, 32.0, 64.0), e, f, V::Health, V::HealthClip);
    // the engine's heat bar: XPack2's Natter (`setHeatBarType B`, `InverseHeatBar 1`) in the original:
    // `ReloadTimebar` (yellow, not the red `Heatbar`), 21×52 at (199, 701) of 1024, its bottom on the health bar's
    // and just left of it. Level 2: one capture, full with the engine cold; how it empties and R/G's pictures, unseen.
    // TODO: R and G's pictures (no level uses them; `Heatbar` for both is a guess), and the bar emptying in flight
    let (e, f) = (pic("Ingame/ReloadTimebar_empty_32x64"), pic("Ingame/ReloadTimebar_full_32x64"));
    fill_bar(&mut commands, place(156.0, 547.0, 32.0, 64.0), e, f, V::Heat, V::HeatClip);
    for (gi, x) in PANEL_X.into_iter().enumerate() {
        let y = 534.0;
        commands.spawn((place(x + 9.0, y - 9.0, 64.0, 64.0), pic("Ingame/Ammobar_vehicle_panel_64x64"), Visibility::Hidden, V::Panel(gi)));
        commands.spawn((place(x + 20.0, y - 20.0, 64.0, 64.0), ImageNode::default(), Visibility::Hidden, V::Icon(gi)));
        let (e, f) = (pic("Ingame/Ammobar_empty_32x64"), pic("Ingame/Ammobar_full_32x64"));
        fill_bar(&mut commands, place(x - 12.0, y - 9.0, 32.0, 64.0), e, f, V::Ammo(gi), V::AmmoClip(gi));
        commands.spawn((
            place(x + 13.0, y - 8.0 + VEHICLE_ROUNDS_DY, 40.0, 20.0),
            crate::bmfont::BmText::new("Trebuchet MS11", Color::WHITE, K, 40.0 * K, Justify::Left, true),
            Visibility::Hidden,
            V::Rounds(gi),
        ));
        for heat in [true, false] {
            for near in [true, false] {
                let bx = if near { x - 10.0 } else { x - 31.0 };
                let (e, f) = if heat { (pic("Ingame/Heatbar_empty_32x64"), pic("Ingame/Heatbar_full_32x64")) } else { (pic("Ingame/ReloadTimebar_empty_32x64"), pic("Ingame/ReloadTimebar_full_32x64")) };
                fill_bar(&mut commands, place(bx, y + 13.0, 32.0, 64.0), e, f, V::Extra(gi, heat, near), V::ExtraClip(gi, heat, near));
            }
        }
    }
    // the supply icons, down the right edge (measured: Humvee seat 2 and M3A1, heal at 302 and ammo at 442; repair at 372, M3A1 by a repairpoint)
    for (i, (n, y)) in [("icon_heal", 302.0), ("icon_repair", 372.0), ("icon_reload", 442.0)].into_iter().enumerate() {
        commands.spawn((place(720.0, y, 64.0, 64.0), pic(n), Visibility::Hidden, SupplyIcon(i)));
    }
    // the turret: the back, the hull turning on it (centred), the gun up
    let center = |x: f32, w: f32| Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), margin: UiRect::left(Val::Px((x - 400.0) * K)), width: Val::Px(w * K), ..default() };
    let mut back = center(400.0, 64.0);
    (back.height, back.bottom) = (Val::Px(64.0 * K), Val::Px((600.0 - 540.0 - 64.0) * K));
    commands.spawn((back, pic("Ingame/Icon_tank_turn_back_64x64"), Visibility::Hidden, V::Turret)).with_children(|t| {
        t.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(10.0 * K), top: Val::Px(10.0 * K), width: Val::Px(32.0 * K), height: Val::Px(32.0 * K), ..default() }, pic("Ingame/Icon_tank_turn_body_32x32"), bevy::ui::UiTransform::IDENTITY, V::Hull));
        t.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(18.0 * K), top: Val::Px(0.0), width: Val::Px(16.0 * K), height: Val::Px(32.0 * K), ..default() }, pic("Ingame/Icon_tank_turn_pipe_16x32")));
    });
}

/// What a panel shows: its icon, its bars (`AB…`) and its weapon's state.
struct Shown {
    icon: Option<String>,
    ammo_bar: bool,
    /// heat (true) or reload, near or far
    extra: Option<(bool, bool)>,
    ammo: f32,
    rounds: i32,
    heat: f32,
    reload: f32,
}

/// The bars of an `AB…` (`setPrimaryAmmoBar`): the ammo bar, and a heat or reload bar.
fn bars(kind: &str) -> (bool, Option<(bool, bool)>) {
    match kind.to_ascii_lowercase().as_str() {
        "abammobar" => (true, None),
        "abammobarheatbar" => (true, Some((true, false))),
        "abammobarreloadbar" => (true, Some((false, false))),
        "abheatbaronly" => (false, Some((true, true))),
        "abreloadbaronly" => (false, Some((false, true))),
        _ => (false, None),
    }
}

/// The side bar on foot ([`HudAmmo::IconSide`]): which one, and how full.
/// TODO: how full, measured only at rest (the medpack and the repair pack full, the grenade empty); throwing, healing and repairing, unmeasured.
fn foot_side(g: Res<Game>, mut bars: Query<(&FootSide, &mut Visibility)>, mut clips: Query<(&FootSideClip, &mut Node)>) {
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let w = p.soldier.filter(|_| p.alive).map(|i| &sim.vehicles[i]).filter(|s| s.entry.inside.is_none()).and_then(|s| s.soldier()).and_then(|m| m.weapon.as_ref());
    let side = w.and_then(|w| match hud_ammo(&sim.world.db.object_template(&w.template.name).and_then(|x| arg(x, "setHudAmmoType")).unwrap_or_default()) {
        HudAmmo::IconSide(s) => Some((w, s)),
        _ => None,
    });
    let heat = side.map(|(_, s)| s != Side::Reload);
    for (b, mut v) in bars.iter_mut() {
        let want = if heat == Some(b.0) { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    let Some((w, s)) = side else { return };
    let t = &w.template;
    let f = match s {
        Side::Ammo => {
            let mag = w.mags.first().copied().unwrap_or(0);
            if t.mag_size > 0 { mag as f32 / t.mag_size as f32 } else { 1.0 }
        }
        Side::Heat => w.heat,
        Side::Reload => {
            if w.reload > 0.0 && t.reload_time > 0.0 { 1.0 - w.reload / t.reload_time } else { 1.0 }
        }
    }
    .clamp(0.0, 1.0);
    for (c, mut n) in clips.iter_mut() {
        let h = Val::Px(64.0 * K * f);
        if heat == Some(c.0) && n.height != h {
            n.height = h;
        }
    }
}

/// One of the supply icons on the right (`ShowHealIcon`, `ShowRepairIcon`, ammo): 0, 1, 2.
#[derive(Component, Clone, Copy)]
struct SupplyIcon(usize);

/// On foot and in a vehicle, what a depot in reach can give: heal, repair, ammo.
/// On a server too: the copy has the level's depots and its vehicles'.
fn supply(g: Res<Game>, mut icons: Query<(&SupplyIcon, &mut Visibility)>) {
    let s = g.session.sim.supply_icons(g.player);
    let on = [s.heal, s.repair, s.ammo];
    for (i, mut v) in icons.iter_mut() {
        let want = if on[i.0] { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

#[allow(clippy::type_complexity)]
fn vehicle(
    g: Res<Game>,
    mut cache: ResMut<Cache>,
    mut images: ResMut<Assets<Image>>,
    mut seen: Local<Option<(String, usize)>>,
    mut pieces: Query<(&V, &mut Node, &mut Visibility, Option<&mut ImageNode>, Option<&mut bevy::ui::UiTransform>), Without<crate::bmfont::BmText>>,
    mut texts: Query<(&V, &mut crate::bmfont::BmText, &mut Visibility)>,
) {
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let inside = p.soldier.filter(|_| p.alive).map(|i| &sim.vehicles[i]).and_then(|s| Some((s.entry.inside?, s.entry.position)));
    let template = |name: &str| sim.world.db.object_template(name);
    let Some((k, seat)) = inside else {
        if seen.take().is_some() {
            for (_, _, mut v, ..) in pieces.iter_mut() {
                *v = Visibility::Hidden;
            }
            for (_, _, mut v) in texts.iter_mut() {
                *v = Visibility::Hidden;
            }
        }
        return;
    };
    let v = &sim.vehicles[k];
    // TODO: GC's X-wing seat 2 (`Astromech_Droid_Control`: `AddPcoPosId 0`, `SetPcoId 1`, and no Seat template) shows the pilot's two
    // panels in the original (torpedoes "2" and lasers) and its first person is the far view; ours shows the droid's one empty panel
    // and looks from inside the hull. A HUD read from the PCO whose id is the seat's `AddPcoPosId` would do it, but vanilla's
    // destroyers' rear gunners declare the same pair (`hatsuzukirearpco`, `fletcher_back_canons_pco`): unchecked. How: compare
    // against the original game's HUD in an X-wing's seat 2 and in a Fletcher's rear seat (on land too); the original's choice
    // of PCO for the HUD would tell.
    let pco = v.entry.seats.get(seat).and_then(|s| template(&s.pco));
    let key = |k: &str| pco.and_then(|t| arg(t, k));
    // the seat's weapons: the primary panel is its first weapon in the template's tree, the secondary the first on another key. GC's
    // X-wing hangs its torpedo launcher (alt-fire, magSize 2) before the lasers and declares `setPrimaryAmmoIcon icon_rocket`: the
    // original's primary panel counts the torpedoes ("2"). Over every seat with two weapons of vanilla, XPack1 and 2 and both DCs
    // (450) the first in the tree is the fire key's and the first on another key the alt-fire's, as before; in GC the 16 that differ
    // (X-, Y-, A-, B-wing, TIEs, Slave1, AT-STs) all declare the first one's icon as primary. Level 2: from the data and one capture.
    let mine: Vec<&bytefield_sim::firearms::FireArms> = v.weapons.iter().filter(|(part, _)| v.craft.parts[*part].seat == seat).map(|(_, w)| w).collect();
    let first = mine.first().copied();
    let first_other = first.and_then(|p| mine.iter().find(|w| w.template.input_fire != p.template.input_fire).copied());
    // (one panel, and the turret's angle: the fire key's weapon, else the alt-fire's)
    let primary = mine.iter().find(|w| w.template.input_fire != 23).copied();
    let secondary = mine.iter().find(|w| w.template.input_fire == 23).copied();
    // no `set…AmmoBar`, no bar nor rounds: the icon alone (the MH-6's and AC-130's pilots; vanilla
    // always declares one); and a panel with no weapon behind it, its icon alone (the AC-130's pilot: the target's icon, no weapon on fire)
    let shown = |w: Option<&bytefield_sim::firearms::FireArms>, which: &str| {
        let (ammo_bar, extra) = bars(&key(&format!("{which}AmmoBar")).unwrap_or_default());
        let icon = key(&format!("{which}AmmoIcon"));
        let Some(w) = w else { return Shown { icon, ammo_bar: false, extra: None, ammo: 1.0, rounds: 0, heat: 0.0, reload: 1.0 } };
        let t = &w.template;
        let mag = w.mags.first().copied().unwrap_or(0);
        Shown {
            icon,
            ammo_bar,
            extra,
            ammo: if t.mag_size > 0 { (mag as f32 / t.mag_size as f32).clamp(0.0, 1.0) } else { 1.0 },
            rounds: mag,
            heat: w.heat.clamp(0.0, 1.0),
            reload: if w.reload > 0.0 && t.reload_time > 0.0 { 1.0 - (w.reload / t.reload_time).clamp(0.0, 1.0) } else { 1.0 },
        }
    };
    let two = key("numberOfWeaponIcons").and_then(|n| n.parse::<i32>().ok()).unwrap_or(1) >= 2 && (primary.is_some() || secondary.is_some());
    let mut panels: [Option<Shown>; 3] = [None, None, None];
    if two {
        // (a seat with weapons on one key only keeps them by key: the alt-fire's on the secondary panel)
        let (p1, p2) = if first_other.is_some() { (first, first_other) } else { (primary, secondary) };
        panels[1] = Some(shown(p1, "primary"));
        panels[2] = Some(shown(p2, "secondary"));
    } else {
        // no weapon on the seat, its declared icon alone (the UH-60's pilot: `icon_hover`)
        panels[0] = primary.map(|w| shown(Some(w), "primary")).or_else(|| secondary.map(|w| shown(Some(w), "secondary"))).or_else(|| Some(shown(None, "primary")));
    }
    // a seat that declares no ammo icon has no panel, whatever weapon it holds (the Humvee's and Technical's driver)
    for p in panels.iter_mut().filter(|p| p.as_ref().is_some_and(|s| s.icon.is_none())) {
        *p = None;
    }
    // the turret's angle: the primary gun's forward in the hull's axes
    // a seat with no gun of its own turns with the vehicle's first (the MLRS's driver and its rocket ramp)
    let turret = (!g.third_person && key("hasTurretIcon").is_some_and(|t| t == "1")).then(|| primary.and_then(|w| v.weapons.iter().find(|(_, x)| std::ptr::eq(x, w)).map(|(part, _)| *part)).or_else(|| v.weapons.first().map(|(part, _)| *part))).flatten().map(|part| {
        let f = v.craft.frames_in_root()[part].rows[2];
        f[0].atan2(f[2])
    });
    // the vehicle's picture and the seats' marks, by the seat's template
    let changed = seen.as_ref() != Some(&(v.template.clone(), seat));
    if changed {
        *seen = Some((v.template.clone(), seat));
    }
    let hp = v.armor.as_ref().map(|a| (a.hp / a.max_hp).clamp(0.0, 1.0));
    // the seat's engine with a heat bar: how full (`InverseHeatBar`: full when cold)
    let heat = v.craft.parts.iter().filter(|p| p.seat == seat).find_map(|p| {
        let bytefield_sim::craft::Kind::Engine(e) = &p.kind else { return None };
        let t = template(&p.name)?;
        arg(t, "heatBarType")?;
        let h = e.heat.clamp(0.0, 1.0);
        Some(if arg(t, "inverseHeatBar").is_some_and(|i| i == "1") { 1.0 - h } else { h })
    });
    let icon_pos = |i: usize| -> Option<(f32, f32)> {
        let s = v.entry.seats.get(i)?;
        let p = arg(template(&s.pco)?, "vehicleIconPos")?;
        let (x, y) = p.split_once('/')?;
        Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
    };
    let picture_name = template(&v.template).and_then(|t| arg(t, "vehicleIcon"));
    let vis = |on: bool| if on { Visibility::Inherited } else { Visibility::Hidden };
    let clip = |n: &mut Node, f: f32| {
        let h = Val::Px(64.0 * K * f);
        if n.height != h {
            n.height = h;
        }
    };
    for (piece, mut node, mut visibility, image, rot) in pieces.iter_mut() {
        let want = match *piece {
            V::Picture => {
                if changed && let Some(mut i) = image {
                    i.image = picture_name.as_deref().map(|n| picture_or_grey(&g, &mut cache, &mut images, n)).unwrap_or_default();
                }
                picture_name.is_some()
            }
            V::Health => hp.is_some(),
            V::HealthClip => {
                clip(&mut node, hp.unwrap_or(0.0));
                true
            }
            V::Heat => heat.is_some(),
            V::HeatClip => {
                clip(&mut node, heat.unwrap_or(0.0));
                true
            }
            V::Dot(i) => match icon_pos(i) {
                Some((x, y)) => {
                    let (l, b) = (Val::Px((200.0 + x - DOT / 2.0) * K), Val::Px((600.0 - 462.0 - y - DOT / 2.0) * K));
                    if node.left != l || node.bottom != b {
                        (node.left, node.bottom) = (l, b);
                    }
                    // yours, a friend's or free (`icon_vehicledot_enemy` needs a vehicle shared with the enemy: never)
                    let name = match v.entry.occupants.get(i).is_some_and(|o| o.is_some()) {
                        true if i == seat => "icon_vehicledot_local",
                        true => "icon_vehicledot_friend",
                        false => "icon_vehicledot_empty",
                    };
                    if let (Some(mut im), Some(h)) = (image, picture(&g, &mut cache, &mut images, name))
                        && im.image != h
                    {
                        im.image = h;
                    }
                    picture_name.is_some()
                }
                None => false,
            },
            V::Panel(gi) => panels[gi].is_some(),
            V::Icon(gi) => {
                if changed && let Some(mut i) = image {
                    i.image = panels[gi].as_ref().and_then(|s| s.icon.as_deref()).map(|n| picture_or_grey(&g, &mut cache, &mut images, n)).unwrap_or_default();
                }
                panels[gi].as_ref().is_some_and(|s| s.icon.is_some())
            }
            V::Ammo(gi) => panels[gi].as_ref().is_some_and(|s| s.ammo_bar),
            V::AmmoClip(gi) => {
                clip(&mut node, panels[gi].as_ref().map_or(0.0, |s| s.ammo));
                true
            }
            V::Extra(gi, heat, near) => panels[gi].as_ref().is_some_and(|s| s.extra == Some((heat, near))),
            V::ExtraClip(gi, heat, _) => {
                clip(&mut node, panels[gi].as_ref().map_or(0.0, |s| if heat { s.heat } else { s.reload }));
                true
            }
            V::Turret => turret.is_some(),
            V::Hull => {
                if let (Some(a), Some(mut t)) = (turret, rot) {
                    // the hull turns the other way from the gun (x is the hull's right; on the
                    // screen a positive turn is clockwise)
                    let r = Rot2::radians(-a);
                    if t.rotation != r {
                        t.rotation = r;
                    }
                }
                true
            }
            V::Rounds(_) => false,
        };
        let want = vis(want);
        if *visibility != want {
            *visibility = want;
        }
    }
    for (piece, mut text, mut visibility) in texts.iter_mut() {
        let V::Rounds(gi) = *piece else { continue };
        let s = panels[gi].as_ref().filter(|s| s.ammo_bar).map(|s| format!("{}", s.rounds));
        let want = vis(s.is_some());
        if *visibility != want {
            *visibility = want;
        }
        if let Some(s) = s
            && text.text != s
        {
            text.text = s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel by `setHudAmmoType`, as the original draws DC_Final's kits: no type is no panel (the binoculars).
    #[test]
    fn hud_ammo_types() {
        assert_eq!(hud_ammo("ATAmmoBar"), HudAmmo::Bar);
        assert_eq!(hud_ammo("ATIcon"), HudAmmo::Icon);
        assert_eq!(hud_ammo("ATIconAndStrengthBar"), HudAmmo::IconSide(Side::Heat));
        assert_eq!(hud_ammo("ATIconAndHeatBar"), HudAmmo::IconSide(Side::Ammo));
        assert_eq!(hud_ammo("ATIconAndReloadBar"), HudAmmo::IconSide(Side::Reload));
        assert_eq!(hud_ammo("ATNone"), HudAmmo::None);
        assert_eq!(hud_ammo(""), HudAmmo::None);
    }
}
