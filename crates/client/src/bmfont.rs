//! The original's bitmap fonts (`font.rfa`: `Font/<name>.dif` and its grey `.tga`) for the HUD's numbers, which the
//! original draws in Trebuchet MS with a dark rim (tickets, ammunition). A `.dif` is a header (version, name, the
//! picture's size, the line's height), then `glyphs` and per glyph its code, the space before, its width, the space
//! after, how far it rises over the base line and its rectangle in the picture; then `kerning`: pairs and how much
//! nearer. Level 2 (from the files and captures).

use bevy::asset::RenderAssetUsages;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::sync::Arc;

/// A glyph: the space before, its width, the space after, its rise over the base line, its rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Glyph {
    pre: f32,
    width: f32,
    post: f32,
    rise: f32,
    rect: Rect,
}

#[derive(Debug)]
pub struct BmFont {
    image: Handle<Image>,
    glyphs: HashMap<u32, Glyph>,
    kerning: HashMap<(u32, u32), f32>,
    /// the tallest glyph's rise: the base line under the top
    ascent: f32,
}

/// A `.dif`'s glyphs, kerning and the picture's size.
fn parse(text: &str) -> Option<(HashMap<u32, Glyph>, HashMap<(u32, u32), f32>)> {
    let lines: Vec<Vec<&str>> = text.lines().map(|l| l.split_whitespace().collect()).collect();
    let at = lines.iter().position(|l| l.as_slice() == ["glyphs"])?;
    let n: usize = lines.get(at + 1)?.first()?.parse().ok()?;
    let mut glyphs = HashMap::default();
    for l in lines.iter().skip(at + 2).take(n) {
        let v: Vec<f32> = l.iter().filter_map(|x| x.parse().ok()).collect();
        if v.len() == 9 {
            glyphs.insert(v[0] as u32, Glyph { pre: v[1], width: v[2], post: v[3], rise: v[4], rect: Rect::new(v[5], v[6], v[7], v[8]) });
        }
    }
    let mut kerning = HashMap::default();
    if let Some(k) = lines.iter().position(|l| l.as_slice() == ["kerning"]) {
        for l in lines.iter().skip(k + 2) {
            let v: Vec<f32> = l.iter().filter_map(|x| x.parse().ok()).collect();
            if v.len() == 3 {
                kerning.insert((v[0] as u32, v[1] as u32), v[2]);
            }
        }
    }
    Some((glyphs, kerning))
}

/// An uncompressed grey TGA (type 3, 8 bits) as white with that grey for alpha.
fn grey_tga(b: &[u8]) -> Option<Image> {
    if b.len() < 18 || b[2] != 3 || b[16] != 8 {
        return None;
    }
    let (w, h) = (u16::from_le_bytes([b[12], b[13]]) as usize, u16::from_le_bytes([b[14], b[15]]) as usize);
    let top = b[17] & 0x20 != 0;
    let px = b.get(18 + b[0] as usize..18 + b[0] as usize + w * h)?;
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let row = if top { y } else { h - 1 - y };
        for x in 0..w {
            let o = (y * w + x) * 4;
            rgba[o..o + 3].copy_from_slice(&[255, 255, 255]);
            rgba[o + 3] = px[row * w + x];
        }
    }
    let image = Image::new(Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, TextureDimension::D2, rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::all());
    Some(image)
}

/// The fonts loaded so far by name (`Trebuchet MS14`), or that failed to.
#[derive(Resource, Default)]
pub struct BmFonts(HashMap<String, Option<Arc<BmFont>>>);

impl BmFonts {
    pub fn get(&mut self, vfs: &bytefield_vfs::Vfs, name: &str, images: &mut Assets<Image>) -> Option<Arc<BmFont>> {
        self.0
            .entry(name.to_string())
            .or_insert_with(|| {
                let dif = vfs.read(&format!("font/{name}.dif")).ok()?;
                let (glyphs, kerning) = parse(&String::from_utf8_lossy(&dif))?;
                let image = images.add(grey_tga(&vfs.read(&format!("font/{name}.tga")).ok()?)?);
                let ascent = glyphs.values().map(|g| g.rise).fold(0.0, f32::max);
                Some(Arc::new(BmFont { image, glyphs, kerning, ascent }))
            })
            .clone()
    }
}

/// A line in a bitmap font, at `scale`, in its node's box aligned left, right or centred; with `rim`, a dark edge
/// a pixel around (the original's numbers). Its glyphs are its children.
#[derive(Component, Clone, Debug)]
pub struct BmText {
    pub font: &'static str,
    pub text: String,
    pub color: Color,
    pub scale: f32,
    /// the box's width and how the line sits in it (`Justify`)
    pub width: f32,
    pub justify: Justify,
    pub rim: bool,
    drawn: Option<(String, Color)>,
}

impl BmText {
    pub fn new(font: &'static str, color: Color, scale: f32, width: f32, justify: Justify, rim: bool) -> BmText {
        BmText { font, text: String::new(), color, scale, width, justify, rim, drawn: None }
    }
}

/// `text`'s lines no wider than `width` (the font's own units), broken at spaces; its own line breaks stay.
pub fn wrap(font: &BmFont, text: &str, width: f32) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.lines() {
        let mut line = String::new();
        for word in para.split(' ') {
            let longer = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if lay_out(font, &longer).1 > width && !line.is_empty() {
                out.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = longer;
            }
        }
        out.push(line);
    }
    out
}

/// Where each glyph of `text` goes from the line's left and top, and the line's width (unscaled).
fn lay_out(font: &BmFont, text: &str) -> (Vec<(Vec2, Rect)>, f32) {
    let mut x = 0.0;
    let mut out = Vec::new();
    let mut prev: Option<u32> = None;
    for c in text.chars() {
        let code = c as u32;
        let Some(g) = font.glyphs.get(&code) else { continue };
        if let Some(p) = prev {
            x += font.kerning.get(&(p, code)).copied().unwrap_or(0.0);
        }
        x += g.pre;
        // (a space only moves on: its rectangle may catch the edge of the glyph beside it; and without its width of 1, as the original's lines
        // of `standard6` in the briefing measure, three of them to the pixel)
        if c != ' ' {
            out.push((Vec2::new(x, font.ascent - g.rise), g.rect));
            x += g.width;
        }
        x += g.post;
        prev = Some(code);
    }
    (out, x)
}

/// Where each glyph of `text` goes on the screen at `scale`, in whole pixels from the line's left and top, its size there and its
/// rectangle, and the line's width in pixels. As the original: the space before, the width and the space after are each rounded on
/// their own (a space too, with its width), so every `o` of `standard6` at 1.28 moves on 5 + 1 pixels; it matches the original's
/// briefing column by column over 300 pixels, where rounding the scaled position spread the gaps unevenly.
///
/// At some screen widths the original first remakes every glyph's metrics (the spaces, the width, the rise and the rectangle's width):
/// each one times a factor, rounded half up, times the inverse of the width's scale, so that drawn at that scale a metric is a whole
/// number of pixels of a slightly smaller font (at 1024: times 1.25, not 1.28; the briefing's MS18 title is 165 px as the original's,
/// not 164). Widths without a row are scaled plainly. Level 1, from the client.
fn place(font: &BmFont, text: &str, scale: f32) -> (Vec<(Vec2, Vec2, Rect)>, f32) {
    let (k, f) = width_factors(scale);
    let half_up = |v: f32| (v + 0.5).floor();
    let px = |v: f32| if k > 0.0 { (half_up(v * k) * f * scale).round() } else { (v * scale).round() };
    let mut x = 0.0;
    let mut out = Vec::new();
    let mut prev: Option<u32> = None;
    for c in text.chars() {
        let code = c as u32;
        let Some(g) = font.glyphs.get(&code) else { continue };
        if let Some(p) = prev {
            x += px(font.kerning.get(&(p, code)).copied().unwrap_or(0.0));
        }
        x += px(g.pre);
        if c != ' ' {
            let size = Vec2::new(px(g.rect.width()).max(1.0), px(g.rect.height()).max(1.0));
            out.push((Vec2::new(x, px(font.ascent - g.rise)), size, g.rect));
        }
        x += px(g.width) + px(g.post);
        prev = Some(code);
    }
    (out, x)
}

/// The original's glyph factors for the screen width `800 · scale` (by the width only): the factor each metric is multiplied by and
/// rounded, and the one it is then multiplied by (800 / the width, so the drawing's scale gives back whole pixels); (0, 0) for none
/// (800, 1600 and any width not in the original's list: those it treats as 800 and leaves the font as it is).
fn width_factors(scale: f32) -> (f32, f32) {
    match (800.0 * scale).round() as i32 {
        640 => (0.95, 1.25),
        1024 => (1.25, 0.78125),
        1152 | 1920 | 2048 => (1.5, 0.694_444_4),
        1280 => (1.5, 0.625),
        _ => (0.0, 0.0),
    }
}

fn draw(mut commands: Commands, mut q: Query<(Entity, &mut BmText)>, mut fonts: ResMut<BmFonts>, mut images: ResMut<Assets<Image>>, g: Option<Res<crate::play::Game>>, menu: Option<Res<crate::menu::Menu>>) {
    // (the menu's over a round, the loading screen's, has its own: what matters is that the font is in one)
    let Some(vfs) = g.as_ref().map(|g| &g.session.sim.world.vfs).or(menu.as_ref().map(|m| m.vfs())) else { return };
    for (e, mut t) in q.iter_mut() {
        if t.drawn.as_ref().is_some_and(|(s, c)| *s == t.text && *c == t.color) {
            continue;
        }
        t.drawn = Some((t.text.clone(), t.color));
        commands.entity(e).despawn_related::<Children>();
        let Some(font) = fonts.get(vfs, t.font, &mut images) else { continue };
        let (glyphs, width) = place(&font, &t.text, t.scale);
        let left = match t.justify {
            Justify::Right | Justify::End => (t.width - width).round(),
            Justify::Center => ((t.width - width) / 2.0).round(),
            _ => 0.0,
        };
        let rims: &[(f32, f32)] = if t.rim { &[(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] } else { &[] };
        // (once: the UI blends in gamma since `graphics::ui_in_gamma`; before, dark text went twice to make up for the linear blending)
        let layers = rims.iter().map(|&(dx, dy)| (Vec2::new(dx, dy), Color::srgba(0.0, 0.0, 0.0, 0.85))).chain([(Vec2::ZERO, t.color)]);
        commands.entity(e).with_children(|p| {
            for (off, color) in layers {
                for (at, size, rect) in &glyphs {
                    p.spawn((
                        Node { position_type: PositionType::Absolute, left: Val::Px(left + at.x + off.x), top: Val::Px(at.y + off.y), width: Val::Px(size.x), height: Val::Px(size.y), ..default() },
                        ImageNode { image: font.image.clone(), rect: Some(*rect), color, ..default() },
                    ));
                }
            }
        });
    }
}

pub struct BmFontPlugin;

impl Plugin for BmFontPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BmFonts>().add_systems(PostUpdate, draw.before(bevy::ui::UiSystems::Prepare));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_dif_reads() {
        let text = "header\r\n2\r\nTrebuchet MS\r\n512\r\n190\r\n24\r\nglyphs\r\n2\r\n48\t1\t12\t1\t17\t137\t0\t149\t17\r\n49\t4\t6\t4\t17\t150\t0\t156\t17\r\nkerning\r\n1\r\n48\t49\t-1\r\n";
        let (g, k) = super::parse(text).unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(g[&48].rect.width(), 12.0);
        assert_eq!(k[&(48, 49)], -1.0);
    }

    /// A space moves on by its space before and after, not its width: `standard6`'s, 2 and not 3 (the original's briefing).
    #[test]
    fn a_space_has_no_width() {
        let text = "header\r\n2\r\nx\r\n256\r\n256\r\n8\r\nglyphs\r\n2\r\n32\t0\t1\t2\t0\t0\t0\t1\t1\r\n105\t0\t1\t1\t7\t127\t10\t128\t17\r\n";
        let (glyphs, kerning) = super::parse(text).unwrap();
        let font = super::BmFont { image: Default::default(), glyphs, kerning, ascent: 7.0 };
        assert_eq!(super::lay_out(&font, "i i").1, 2.0 + 2.0 + 2.0);
    }

    /// On the screen each part of a glyph is rounded on its own, as the original's briefing at 1.28: an `o` of 4 + 1 moves on
    /// 5 + 1 pixels and not 6.4, and a space (1 wide, 2 after) 1 + 3.
    #[test]
    fn each_part_of_a_glyph_is_rounded_on_its_own() {
        let text = "header\r\n2\r\nx\r\n256\r\n256\r\n8\r\nglyphs\r\n2\r\n32\t0\t1\t2\t0\t0\t0\t1\t1\r\n111\t0\t4\t1\t5\t152\t10\t156\t15\r\n";
        let (glyphs, kerning) = super::parse(text).unwrap();
        let font = super::BmFont { image: Default::default(), glyphs, kerning, ascent: 7.0 };
        let (placed, width) = super::place(&font, "oo o", 1.28);
        assert_eq!(placed.iter().map(|g| g.0.x).collect::<Vec<_>>(), [0.0, 6.0, 16.0]);
        assert_eq!(placed[0].1, bevy::math::Vec2::new(5.0, 6.0));
        assert_eq!(width, 22.0);
    }

    /// At 1024 wide the original remakes the metrics times 1.25 rounded half up (then drawn at 1.28 they are whole pixels): a glyph 9
    /// wide is 11 pixels, not 12 (11.52); at a width it has no row for, plainly scaled.
    #[test]
    fn at_1024_the_glyphs_are_remade_times_1_25() {
        let text = "header\r\n2\r\nx\r\n256\r\n256\r\n8\r\nglyphs\r\n1\r\n77\t0\t9\t1\t11\t0\t0\t9\t11\r\n";
        let (glyphs, kerning) = super::parse(text).unwrap();
        let font = super::BmFont { image: Default::default(), glyphs, kerning, ascent: 11.0 };
        assert_eq!(super::place(&font, "MM", 1.28).1, 2.0 * (11.0 + 1.0));
        assert_eq!(super::place(&font, "MM", 1.3).1, 2.0 * (12.0 + 1.0));
    }
}
