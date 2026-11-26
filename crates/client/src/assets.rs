//! Meshes and textures from the game's files as Bevy assets, each loaded once.
//!
//! Frames: Refractor is left-handed with Y up; Bevy is right-handed with Y up. A point
//! (x, y, z) becomes (x, y, -z), and triangles flip their winding.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bytefield_formats::{dds, image::Image as Rgba, rs, sm, tm};
use bytefield_vfs::Vfs;
use bytefield_world::Level;
use bytefield_world::scene::Mat4;

/// The parts (one per material) of a mesh, ready to spawn.
pub type Parts = Vec<(Handle<Mesh>, Handle<StandardMaterial>)>;

/// Meshes by kind and file, textures by name: each read and decoded once.
#[derive(Resource, Default)]
pub struct Cache {
    meshes: HashMap<String, Parts>,
    /// each `.sm`'s shadow LOD as one mesh ([`Cache::shadow`]); None when it has none
    shadows: HashMap<String, Option<Handle<Mesh>>>,
    textures: Textures,
    /// how the trees are lit ([`crate::level::Atmosphere::tree`]); white without it
    pub tree_light: Option<Color>,
    /// the materials with `envmap true` in their `.rs` (lit, not blended): `envmap::swap` gives them the level's cube
    pub envmapped: std::collections::HashSet<AssetId<StandardMaterial>>,
    /// the lit ones (not blended), envmapped or not: the original's light (setting `fixed_light`, `envmap::swap`), with
    /// their `.rs`'s `materialDiffuse`
    pub lit: HashMap<AssetId<StandardMaterial>, Vec3>,
    /// the materials with `textureFade true` (in vanilla, only the black boxes `texture/black_o` that stand for a house's
    /// inside from afar): `level.rs` fades them in with the distance
    pub faded: std::collections::HashSet<AssetId<StandardMaterial>>,
    /// the blended ones with `lighting false` (not additive: the blurred rotors' discs): `envmap::swap` blends them in gamma, as D3D8
    pub unlit_blended: std::collections::HashSet<AssetId<StandardMaterial>>,
    /// each `standardmesh` file's `GeometryTemplate.alphaTestValue` (by lowercase file), see [`Cache::set_alpha_tests`]
    alpha_tests: HashMap<String, f32>,
}

/// Textures by name, each loaded once, and where the level looks first.
#[derive(Default)]
struct Textures {
    loaded: HashMap<String, Option<Handle<Image>>>,
    /// the level's `textureManager.alternativePath` (`Texture/Africa`, DC's `bf1942/levels/<map>/Textures`):
    /// a `texture/…` is looked for there first, by its file's name (El Alamein's desert Panzer IV,
    /// `texture/Africa/p4main_f`, level 2: both files share the name, so it can only come first)
    /// (all of them, the last one first: DC_Bridge's sky is in the first of its `Textures/` and `texture/`)
    alternative: Vec<String>,
    /// what a mesh's missing texture is drawn with
    grey: Option<Handle<Image>>,
}

impl Cache {
    /// The level's `textureManager.alternativePath`s, in their order (before anything loads). From the game's folder: GC's
    /// `../../bf1942/levels/GC_Bespin/Textures/Sky` is the level's own.
    pub fn set_alternative_paths<'a>(&mut self, paths: impl DoubleEndedIterator<Item = &'a str>) {
        self.textures.alternative = paths
            .map(|p| {
                let p = p.trim().trim_matches('"').replace('\\', "/");
                p.trim_start_matches("../").trim_matches('/').to_string()
            })
            .filter(|p| !p.is_empty())
            .rev()
            .collect();
        info!("textures: alternative paths {:?}", self.textures.alternative);
    }

    /// The `GeometryTemplate.alphaTestValue`s (before anything loads): a `.rs` without `alphaTestRef` is cut there. Level 2, from
    /// captures: the M1A1's rear rack (`M1A1_rack`, DXT1 with its gaps at alpha 0, `alphaTestValue 0.08` and no test in its `.rs`)
    /// shows the turret through the bars in the original, black in ours without it. All 44 in the data are 0.08 (the rack, the
    /// Panzer IV's, Chi-Ha's and BMP's wheels, the barbed wire), and per file: no file has two values.
    pub fn set_alpha_tests(&mut self, db: &bytefield_con::templates::TemplateDb) {
        self.alpha_tests = db
            .templates
            .iter()
            .filter(|((ns, _), _)| ns.eq_ignore_ascii_case("geometrytemplate"))
            .filter_map(|(_, t)| {
                let v = t.get("alphatestvalue")?.first()?.trim().parse::<f32>().ok()?;
                Some((t.get("file")?.first()?.replace('\\', "/").trim_start_matches('/').to_lowercase(), v))
            })
            .collect();
    }

    /// A texture by name (`.dds` or `.tga`, also under `texture/`), loaded once.
    pub fn texture(&mut self, vfs: &Vfs, name: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        texture(vfs, name, false, images, &mut self.textures)
    }

    /// A GeometryTemplate's file: `standardmesh`/`animatedmesh` (`standardmesh/<file>.sm`, LOD 0)
    /// or `treemesh`; nothing for other kinds or a file that does not load.
    pub fn get(&mut self, vfs: &Vfs, kind: &str, file: &str, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) -> Parts {
        let key = format!("{kind}:{}", file.to_lowercase());
        if let Some(p) = self.meshes.get(&key) {
            return p.clone();
        }
        let p = match kind {
            "standardmesh" | "animatedmesh" => {
                let alpha_test = self.alpha_tests.get(file.trim_start_matches('/').to_lowercase().as_str()).copied();
                load_sm(vfs, file, alpha_test, meshes, materials, images, &mut self.textures, &mut self.envmapped, &mut self.lit, &mut self.faded, &mut self.unlit_blended)
            }
            "treemesh" => load_tm(vfs, file, meshes, materials, images, &mut self.textures, self.tree_light.unwrap_or(Color::WHITE)),
            _ => Vec::new(),
        };
        self.meshes.insert(key, p.clone());
        p
    }

    /// What the original's silhouette pass draws for a `standardmesh`: its `.sm`'s shadow LOD, in one
    /// mesh, loaded once. None for a file without one (DC's `PatriotBase`, `SA3_base_m1`: their pass comes out empty) or of another kind.
    pub fn shadow(&mut self, vfs: &Vfs, kind: &str, file: &str, meshes: &mut Assets<Mesh>) -> Option<Handle<Mesh>> {
        if kind != "standardmesh" {
            return None;
        }
        let key = file.to_lowercase();
        if let Some(h) = self.shadows.get(&key) {
            return h.clone();
        }
        let h = vfs.read(&format!("standardmesh/{file}.sm")).ok().and_then(|b| sm::parse(&b).ok()).and_then(|m| shadow_lod_mesh(&m)).map(|m| meshes.add(m));
        self.shadows.insert(key, h.clone());
        h
    }
}

/// The `.sm`'s first shadow LOD as one mesh (positions and triangles; its materials one after the other), or None without one.
/// Measured in the original (F1): the Stryker's pass is one draw of 249
/// vertices and 83 triangles, `STRYKER_hull.sm`'s shadow LOD exactly (not its collision 1, which has 83 faces too); the AC-130's, 750/250.
pub fn shadow_lod_mesh(m: &sm::StandardMesh) -> Option<Mesh> {
    let lod = m.shadow_lods.first()?;
    let (mut pos, mut idx): (Vec<[f32; 3]>, Vec<u32>) = (Vec::new(), Vec::new());
    for mat in &lod.materials {
        let base = pos.len() as u32;
        pos.extend(mat.positions.iter().map(|p| [p[0], p[1], -p[2]]));
        idx.extend(mat.triangles.iter().flat_map(|t| [base + t[0] as u32, base + t[2] as u32, base + t[1] as u32]));
    }
    if idx.is_empty() {
        return None;
    }
    Some(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos).with_inserted_indices(Indices::U32(idx)))
}

/// Refractor textures tile (UVs go past 0..1), so the sampler repeats. With their mipmaps (a
/// 2×2 box each, in the file's space): without them the water and the far ground shimmer.
pub fn rgba_image(img: &Rgba) -> Image {
    rgba_image_mips(std::slice::from_ref(img))
}

/// The graphics profile's settings (`main` and the options fix them before the
/// textures of a round): the original's look unless told otherwise, with or without packs.
static GRAPHICS: std::sync::RwLock<Option<bytefield_content::Graphics>> = std::sync::RwLock::new(None);

pub fn set_graphics(g: bytefield_content::Graphics) {
    if let Ok(mut w) = GRAPHICS.write() {
        *w = Some(g);
    }
}

pub fn graphics() -> bytefield_content::Graphics {
    GRAPHICS.read().ok().and_then(|g| *g).unwrap_or_else(|| bytefield_content::Graphics::resolve(Default::default(), &Default::default()))
}

/// What is cut by its alpha at `cut` (foliage, fences): alpha to coverage under MSAA when the settings
/// say so, else the original's cut.
fn cut_by_alpha(cut: f32) -> AlphaMode {
    let g = graphics();
    if g.coverage && g.msaa() { AlphaMode::AlphaToCoverage } else { AlphaMode::Mask(cut) }
}

/// The largest side a texture keeps (`BYTEFIELD_MAX_TEXTURE`; 2048, 4096 in the enhanced profile:
/// the packs' 4096² fit since they go up as BC).
fn max_texture() -> usize {
    graphics().max_texture
}

/// Anisotropic filtering (`aniso=` in content.cfg: 1, 2, 4, 8 or 16; 1, the original's look, or 16 in the
/// enhanced profile): the ground seen at a grazing angle stays sharp.
fn anisotropy() -> u16 {
    graphics().aniso
}

/// A level halved (2×2 boxes).
fn half(l: &Rgba) -> Rgba {
    let (w, h) = (l.width, l.height);
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut rgba = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..4 {
                let at = |xx: usize, yy: usize| l.rgba[(yy.min(h - 1) * w + xx.min(w - 1)) * 4 + c] as u32;
                rgba[(y * nw + x) * 4 + c] = ((at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1) + 2) / 4) as u8;
            }
        }
    }
    Rgba { width: nw, height: nh, rgba }
}

/// The same with the mip levels the file carries (a DDS's), the rest made as in `rgba_image`.
pub fn rgba_image_mips(given: &[Rgba]) -> Image {
    // no wider or higher than `max_texture`: from the first carried level that fits, else the
    // last one halved until it does
    let max = max_texture();
    let skip = given.iter().position(|l| l.width.max(l.height) <= max).unwrap_or(given.len() - 1);
    let mut owned: Vec<Rgba> = Vec::new();
    let given = if skip == 0 { given } else { &given[skip..] };
    let given = if given[0].width.max(given[0].height) > max {
        let mut l = given[0].clone();
        while l.width.max(l.height) > max {
            l = half(&l);
        }
        owned.push(l);
        &owned[..]
    } else {
        given
    };
    let img = &given[0];
    let mut data: Vec<u8> = given.iter().flat_map(|l| l.rgba.iter().copied()).collect();
    let last = given.last().unwrap();
    let (mut w, mut h) = (last.width, last.height);
    let mut level = last.clone();
    let mut levels = given.len() as u32;
    while w > 1 || h > 1 {
        level = half(&level);
        data.extend_from_slice(&level.rgba);
        (w, h) = (level.width, level.height);
        levels += 1;
    }
    let mut i = Image::new(
        Extent3d { width: img.width as u32, height: img.height as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        img.rgba.clone(),
        TextureFormat::Rgba8UnormSrgb,
        // (only the GPU's: the main world's copy goes once uploaded; the graphics packs' would
        // take gigabytes twice)
        RenderAssetUsages::RENDER_WORLD,
    );
    i.data = Some(data);
    i.texture_descriptor.mip_level_count = levels;
    i.sampler = repeat_sampler();
    i
}

pub(crate) fn repeat_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: anisotropy(),
        ..default()
    })
}

/// Whether the GPU takes BC1–3; set once the renderer is up (`BYTEFIELD_BC=0`: never).
static BC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn bc_support(s: Option<Res<bevy::image::CompressedImageFormatSupport>>) {
    let on = s.is_some_and(|s| s.0.contains(bevy::image::CompressedImageFormats::BC)) && std::env::var("BYTEFIELD_BC").map_or(true, |v| v != "0");
    BC.store(on, std::sync::atomic::Ordering::Relaxed);
    info!("textures {}", if on { "uploaded as BC1-3 (DXT)" } else { "decoded to RGBA" });
}

/// A DXT file's blocks as the GPU takes them, without decoding: a quarter (DXT1: an eighth) of
/// the RGBA's memory, which the graphics packs need. From the first carried level no
/// larger than `max_texture`; nothing when that level is not whole blocks or the carried chain
/// stops above 4×4 (the RGBA path makes the rest), or the GPU does not take them.
fn bc_image(c: &dds::Compressed) -> Option<Image> {
    if !BC.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let format = match &c.fourcc {
        b"DXT1" => TextureFormat::Bc1RgbaUnormSrgb,
        b"DXT3" => TextureFormat::Bc2RgbaUnormSrgb,
        _ => TextureFormat::Bc3RgbaUnormSrgb,
    };
    let max = max_texture();
    let size = |l: usize| ((c.width >> l).max(1), (c.height >> l).max(1));
    let skip = (0..c.levels.len()).find(|&l| size(l).0.max(size(l).1) <= max)?;
    let (w, h) = size(skip);
    let (lw, lh) = size(c.levels.len() - 1);
    if w % 4 != 0 || h % 4 != 0 || lw.max(lh) > 4 {
        return None;
    }
    let mut i = Image::default();
    i.data = Some(c.levels[skip..].concat());
    i.texture_descriptor.size = Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 };
    i.texture_descriptor.format = format;
    i.texture_descriptor.mip_level_count = (c.levels.len() - skip) as u32;
    i.asset_usage = RenderAssetUsages::RENDER_WORLD;
    i.sampler = repeat_sampler();
    Some(i)
}

/// The terrain's colour tiles as one BC1 texture, their blocks copied as they come, level by level: the
/// original samples each 1024² tile whole, 4 texels a metre, where the RGBA mosaic, at 8192² some 350 MB, went down
/// to `max_texture` (Tobruk's 8 × 512 to 2048: a texel a metre, and the baked shadows of posts, wire and bushes were
/// gone). `found` and `grid` as [`Level::colour_tiles`], `rows` of them; a missing tile is the
/// level's `terrainDefault.dds` or transparent. Nothing (the RGBA path then) without BC on the GPU or when a tile is
/// not DXT1 of the others' size with its levels down to 4 × 4.
pub fn bc_mosaic(vfs: &bytefield_vfs::Vfs, base: &str, found: &[(usize, usize)], grid: usize, rows: usize) -> Option<Image> {
    // (wgpu's default `max_texture_dimension_2d`)
    const MAX_SIDE: usize = 8192;
    if !BC.load(std::sync::atomic::Ordering::Relaxed) || grid == 0 || rows == 0 {
        return None;
    }
    let dir = &base[..base.rfind(['/', '\\']).map_or(0, |i| i + 1)];
    let default = vfs.read(&format!("{dir}terrainDefault.dds")).ok();
    let files: Vec<((usize, usize), Vec<u8>)> = found.iter().filter(|&&(x, y)| x < grid && y < rows).map(|&(x, y)| Some(((x, y), vfs.read(&format!("{base}{x:02}x{y:02}.dds")).ok()?))).collect::<Option<_>>()?;
    let tiles: Vec<((usize, usize), dds::Compressed)> = files.iter().map(|(c, b)| Some((*c, dds::compressed(b)?))).collect::<Option<_>>()?;
    let default = match &default {
        Some(b) => Some(dds::compressed(b)?),
        None => None,
    };
    let first = tiles.first().map(|t| &t.1).or(default.as_ref())?;
    let side = first.width;
    let ok = |c: &dds::Compressed| &c.fourcc == b"DXT1" && c.width == side && c.height == side && (side >> (c.levels.len() - 1)).max(1) <= 4;
    if !side.is_power_of_two() || side < 4 || !tiles.iter().all(|t| ok(&t.1)) || !default.as_ref().is_none_or(ok) {
        return None;
    }
    // from the first level whose mosaic fits, down to the tiles' 4 × 4
    let skip = (0..).find(|&l| (grid * side) >> l <= MAX_SIDE)?;
    let levels = (side.trailing_zeros() as usize).saturating_sub(1);
    if skip >= levels {
        return None;
    }
    const EMPTY: [u8; 8] = [0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff];
    let mut data = Vec::new();
    for l in skip..levels {
        let tb = (side >> l) / 4;
        let row = grid * tb;
        let start = data.len();
        data.resize(start + row * rows * tb * 8, 0);
        let out = &mut data[start..];
        for yy in 0..rows {
            for xx in 0..grid {
                let src = tiles.iter().find(|t| t.0 == (xx, yy)).map(|t| &t.1).or(default.as_ref()).map(|c| c.levels[l]);
                for r in 0..tb {
                    let dst = &mut out[((yy * tb + r) * row + xx * tb) * 8..][..tb * 8];
                    match src {
                        Some(s) => dst.copy_from_slice(&s[r * tb * 8..][..tb * 8]),
                        None => dst.chunks_exact_mut(8).for_each(|b| b.copy_from_slice(&EMPTY)),
                    }
                }
            }
        }
    }
    let mut i = Image::default();
    i.data = Some(data);
    i.texture_descriptor.size = Extent3d { width: ((grid * side) >> skip) as u32, height: ((rows * side) >> skip) as u32, depth_or_array_layers: 1 };
    i.texture_descriptor.format = TextureFormat::Bc1RgbaUnormSrgb;
    i.texture_descriptor.mip_level_count = (levels - skip) as u32;
    i.asset_usage = RenderAssetUsages::RENDER_WORLD;
    i.sampler = repeat_sampler();
    Some(i)
}

/// Alpha-tested textures: each mip level's alpha scaled so that as much of it passes `cutoff` as
/// of the top level: with the plain mip chain the test strips thin detail (wire, nets) within a
/// few metres. Idempotent.
fn keep_coverage(img: &mut Image, cutoff: f32) {
    let (w0, h0) = (img.texture_descriptor.size.width as usize, img.texture_descriptor.size.height as usize);
    let levels = img.texture_descriptor.mip_level_count as usize;
    let Some(data) = img.data.as_mut() else { return };
    let r = (cutoff * 255.0) as u32;
    let pass = |px: &[u8], s: f32| px.chunks_exact(4).filter(|p| ((p[3] as f32 * s).min(255.0)) as u32 > r).count() as f32 / (px.len() / 4).max(1) as f32;
    let top = pass(&data[..w0 * h0 * 4], 1.0);
    if top == 0.0 {
        return;
    }
    let (mut off, mut w, mut h) = (w0 * h0 * 4, (w0 / 2).max(1), (h0 / 2).max(1));
    for _ in 1..levels {
        let lv = &mut data[off..off + w * h * 4];
        let (mut lo, mut hi) = (1.0f32, 64.0f32);
        if pass(lv, 1.0) < top {
            for _ in 0..16 {
                let mid = 0.5 * (lo + hi);
                if pass(lv, mid) < top { lo = mid } else { hi = mid }
            }
            for p in lv.chunks_exact_mut(4) {
                p[3] = (p[3] as f32 * hi).min(255.0) as u8;
            }
        }
        off += w * h * 4;
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
    }
}

pub fn terrain_mesh(lvl: &Level, step: usize) -> Mesh {
    let n = lvl.heightmap.size;
    let cells = (n - 1) / step;
    let side = cells + 1;
    // node (c, r) at (c, r) · worldSize / n, as the engine
    let ws = lvl.terrain.world_size as f32;
    let cell = ws / n as f32 * step as f32;
    let h = |c: usize, r: usize| lvl.terrain.height_m(lvl.heightmap.at((c * step).min(n - 1), (r * step).min(n - 1))) as f32;
    let mut pos = Vec::with_capacity(side * side);
    let mut nor = Vec::with_capacity(side * side);
    let mut uv = Vec::with_capacity(side * side);
    for r in 0..side {
        for c in 0..side {
            let (x, z) = (c as f32 * cell, r as f32 * cell);
            pos.push([x, h(c, r), -z]);
            let dx = h((c + 1).min(cells), r) - h(c.saturating_sub(1), r);
            let dz = h(c, (r + 1).min(cells)) - h(c, r.saturating_sub(1));
            nor.push(Vec3::new(-dx, 2.0 * cell, dz).normalize().to_array());
            uv.push([x / ws, z / ws]);
        }
    }
    let mut idx = Vec::with_capacity(cells * cells * 6);
    for r in 0..cells {
        for c in 0..cells {
            let i = (r * side + c) as u32;
            let s = side as u32;
            // (the diagonal in a checkerboard, as the engine's ground and the original's drawn terrain: a
            // ridge's silhouette near the eye, Kharkov's hill)
            if (c * step + r * step) % 2 == 0 {
                idx.extend_from_slice(&[i, i + 1, i + s + 1, i, i + s + 1, i + s]);
            } else {
                idx.extend_from_slice(&[i, i + 1, i + s, i + 1, i + s + 1, i + s]);
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}

/// The water's alpha as the original makes it: on its own grid of 16 m, at
/// each node `waterShallowAlpha` if the ground is above the water, else the depth over
/// `waterAlphaDepth` (to 1); between the nodes, interpolated. So the shore fades over a whole cell.
fn water_alpha(lvl: &Level, shallow_alpha: f32, alpha_depth: f32) -> impl Fn(f32, f32) -> f32 + '_ {
    const GRID: f32 = 16.0;
    let wl = lvl.terrain.water_level as f32;
    let n = lvl.heightmap.size;
    let per = lvl.terrain.world_size as f32 / n as f32;
    let node = move |i: i64, j: i64| {
        let at = |k: i64| ((k as f32 * GRID / per).round().max(0.0) as usize).min(n - 1);
        let d = wl - lvl.terrain.height_m(lvl.heightmap.at(at(i), at(j))) as f32;
        if d < 0.0 { shallow_alpha } else { (d / alpha_depth).min(1.0) }
    };
    move |x, z| {
        let (gx, gz) = (x / GRID, z / GRID);
        let (i, j) = (gx.floor() as i64, gz.floor() as i64);
        let (fx, fz) = (gx - i as f32, gz - j as f32);
        let top = node(i, j) * (1.0 - fx) + node(i + 1, j) * fx;
        let bottom = node(i, j + 1) * (1.0 - fx) + node(i + 1, j + 1) * fx;
        top * (1.0 - fz) + bottom * fz
    }
}

/// The water over the terrain: each cell with a corner below the water level, at that level, its
/// depth in `uv.x` (negative above the ground: the shore falls inside the cell) and its alpha in
/// `uv.y` (`water_alpha`); around the map, a border of deep water as wide as the map. None without
/// water.
pub fn water_mesh(lvl: &Level, shallow_alpha: f32, alpha_depth: f32) -> Option<Mesh> {
    let wl = lvl.terrain.water_level as f32;
    let n = lvl.heightmap.size;
    // big maps (the sea) in coarser cells
    let step = if n > 1100 { 4 } else { 2 };
    let cells = (n - 1) / step;
    let cell = lvl.terrain.world_size as f32 / n as f32 * step as f32;
    let depth = |c: usize, r: usize| wl - lvl.terrain.height_m(lvl.heightmap.at((c * step).min(n - 1), (r * step).min(n - 1))) as f32;
    let alpha = water_alpha(lvl, shallow_alpha, alpha_depth);
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    let mut quad = |corners: [(f32, f32, f32); 4]| {
        let i = pos.len() as u32;
        for (x, z, d) in corners {
            pos.push([x, wl, -z]);
            uv.push([d, alpha(x, z)]);
        }
        idx.extend_from_slice(&[i, i + 1, i + 2, i + 1, i + 3, i + 2]);
    };
    // every cell below the water. The original leaves dry the plain under Wake's bridge (1.8 m deep);
    // drawing only the patches that go deep enough (BYTEFIELD_WATER_PATCH=<samples>:<metres>,
    // 16:3 fitted there) cuts the rest of the coast in squares, so it is off: the bridge stays open.
    let patch: Option<(usize, f32)> = std::env::var("BYTEFIELD_WATER_PATCH")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once(':')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        });
    let deep_patch = |c: usize, r: usize| {
        let Some((p, cut)) = patch else { return true };
        let (c0, r0) = (c * step / p * p, r * step / p * p);
        (c0..=(c0 + p).min(n - 1)).any(|cc| (r0..=(r0 + p).min(n - 1)).any(|rr| wl - lvl.terrain.height_m(lvl.heightmap.at(cc, rr)) as f32 > cut))
    };
    let (mut edge_wet, mut any) = (false, false);
    for r in 0..cells {
        for c in 0..cells {
            let d = [depth(c, r), depth(c + 1, r), depth(c, r + 1), depth(c + 1, r + 1)];
            if d.iter().all(|&d| d <= 0.0) || !deep_patch(c, r) {
                continue;
            }
            any = true;
            edge_wet |= r == 0 || c == 0 || r == cells - 1 || c == cells - 1;
            let (x, z) = (c as f32 * cell, r as f32 * cell);
            quad([(x, z, d[0]), (x + cell, z, d[1]), (x, z + cell, d[2]), (x + cell, z + cell, d[3])]);
        }
    }
    if !any {
        return None;
    }
    if edge_wet {
        let s = lvl.terrain.world_size as f32;
        let deep = 100.0;
        for (x0, z0, x1, z1) in [(-s, -s, 2.0 * s, 0.0), (-s, s, 2.0 * s, 2.0 * s), (-s, 0.0, 0.0, s), (s, 0.0, 2.0 * s, s)] {
            quad([(x0, z0, deep), (x1, z0, deep), (x0, z1, deep), (x1, z1, deep)]);
        }
    }
    let count = pos.len();
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_indices(Indices::U32(idx)),
    )
}

/// One material of LOD 0 as a Bevy mesh, in Refractor coordinates mirrored to Bevy's; with the lightmap's UVs
/// when it has them (the second set, `level::Lightmaps`).
fn sm_mesh(m: &sm::MeshMaterial) -> Mesh {
    let pos: Vec<[f32; 3]> = m.positions.iter().map(|p| [p[0], p[1], -p[2]]).collect();
    let nor: Vec<[f32; 3]> = m.normals.iter().map(|n| [n[0], n[1], -n[2]]).collect();
    let idx: Vec<u32> = m.triangles.iter().flat_map(|t| [t[0] as u32, t[2] as u32, t[1] as u32]).collect();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs.clone())
        .with_inserted_indices(Indices::U32(idx));
    if m.lightmap_uvs.len() == m.positions.len() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, m.lightmap_uvs.clone());
    }
    mesh
}

/// Refractor world matrix -> Bevy transform (mirror z on both sides: S M S).
pub fn to_transform(m: &Mat4) -> Transform {
    let s = |r: usize, c: usize| -> f32 {
        let sign = if (r == 2) ^ (c == 2) { -1.0 } else { 1.0 };
        (m.0[c][r] * sign) as f32
    };
    let mat = bevy::math::Mat4::from_cols(
        Vec4::new(s(0, 0), s(1, 0), s(2, 0), 0.0),
        Vec4::new(s(0, 1), s(1, 1), s(2, 1), 0.0),
        Vec4::new(s(0, 2), s(1, 2), s(2, 2), 0.0),
        Vec4::new(m.0[3][0] as f32, m.0[3][1] as f32, -m.0[3][2] as f32, 1.0),
    );
    Transform::from_matrix(mat)
}

/// A frame of the simulation (rows: the axes in the world, pos: the origin) as a matrix.
pub fn frame_matrix(rows: &[[f32; 3]; 3], pos: [f32; 3]) -> Mat4 {
    let c = |k: usize| [rows[k][0] as f64, rows[k][1] as f64, rows[k][2] as f64, 0.0];
    Mat4([c(0), c(1), c(2), [pos[0] as f64, pos[1] as f64, pos[2] as f64, 1.0]])
}

/// A Refractor point or direction in Bevy's frame.
pub fn to_bevy(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], -v[2])
}

/// `rgba`: decoded even if the GPU takes the file's blocks (`keep_coverage` changes its pixels).
fn texture(vfs: &Vfs, name: &str, rgba: bool, images: &mut Assets<Image>, texcache: &mut Textures) -> Option<Handle<Image>> {
    let alternative = texcache.alternative.clone();
    texcache
        .loaded
        .entry(format!("{}{}", name.to_lowercase(), if rgba { "#rgba" } else { "" }))
        .or_insert_with(|| {
            let n = name.replace('\\', "/");
            // (some name their file with its extension: GC's snowtrooper, `texture/snowtrooper/body.dds`)
            let lower = n.to_ascii_lowercase();
            let n = if lower.ends_with(".dds") || lower.ends_with(".tga") { &n[..n.len() - 4] } else { &n[..] };
            // (by the file's own name: DC_Coastal_Hammer's `CustomTextures/BMP-2.dds` stands for DC's
            // `texture/DesertCombat/BMP2/BMP-2`; vanilla's regional folders are flat, so either reading fits them.
            // Any name, not only `texture/…`: GC_Ryloth's hangar asks `textures/custom/trench`, which is only
            // in its `Textures/Custom`)
            let file = n.rsplit('/').next().unwrap_or(n);
            let bytes = alternative.iter().map(|a| format!("{a}/{file}.dds")).chain([format!("{n}.dds"), format!("texture/{n}.dds")]).find_map(|p| vfs.read(&p).ok());
            if let Some(i) = bytes.as_deref().filter(|_| !rgba).and_then(dds::compressed).and_then(|c| bc_image(&c)) {
                return Some(images.add(i));
            }
            // the effects' are TGA
            let dds = bytes.and_then(|b| dds::decode_mips(&b).ok());
            let tga = || [format!("{n}.tga"), format!("texture/{n}.tga")].iter().find_map(|p| vfs.read(p).ok()).and_then(|b| bytefield_formats::tga::decode(&b).ok()).map(|i| vec![i]);
            // (GC's screens and holograms: a Bink movie in the game's folder)
            dds.or_else(tga).map(|mips| images.add(rgba_image_mips(&mips))).or_else(|| crate::movie::texture(name, images))
        })
        .clone()
}

fn load_sm(
    vfs: &Vfs,
    file: &str,
    alpha_test: Option<f32>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    texcache: &mut Textures,
    envmapped: &mut std::collections::HashSet<AssetId<StandardMaterial>>,
    lit: &mut HashMap<AssetId<StandardMaterial>, Vec3>,
    faded: &mut std::collections::HashSet<AssetId<StandardMaterial>>,
    unlit_blended: &mut std::collections::HashSet<AssetId<StandardMaterial>>,
) -> Parts {
    let Ok(bytes) = vfs.read(&format!("standardmesh/{file}.sm")) else { return Vec::new() };
    let Ok(mesh) = sm::parse(&bytes) else { return Vec::new() };
    let shaders = vfs.read(&format!("standardmesh/{file}.rs")).map(|b| rs::parse(&String::from_utf8_lossy(&b))).unwrap_or_default();
    let Some(lod) = mesh.lods.first() else { return Vec::new() };
    lod.materials
        .iter()
        .map(|m| {
            let sub = shaders.iter().find(|s| s.name.eq_ignore_ascii_case(&m.name));
            // (its own `alphaTestRef` first, else its geometry's `alphaTestValue`)
            let own = sub.and_then(|s| s.alpha_test).or(alpha_test);
            let cutoff = sub.filter(|s| !s.transparent).and(own);
            // A file that no archive has is what the texture manager gives for any name it lacks, `texture/defaultTexture.dds` (128² DXT5,
            // a flat 76, 72, 74), as for a missing lightmap. Read in the original's rendering: the subset of
            // DC_First_Light's `air_runway_m1` whose `.rs` names a missing file gets that texture in stage 0.
            // Before, a flat 75/255 (Iwo Jima's `pf_w2_r` R 21.5 against 21.8, the M2A3's antenna 109 against 110).
            // The 75 stays for a tree with no `bf1942/texture.rfa`.
            let missing = |images: &mut Assets<Image>, texcache: &mut Textures| {
                texture(vfs, "texture/defaultTexture", cutoff.is_some(), images, texcache)
                    .unwrap_or_else(|| texcache.grey.get_or_insert_with(|| images.add(rgba_image_mips(&[Rgba { width: 1, height: 1, rgba: vec![75, 75, 75, 255] }]))).clone())
            };
            let texture = sub.and_then(|s| s.texture.clone()).map(|t| texture(vfs, &t, cutoff.is_some(), images, texcache).unwrap_or_else(|| missing(images, texcache)));
            // (blended ones fade with the file's mips, as in the original)
            // (a texture already uploaded has no pixels left here: it stays as it went)
            if let (Some(h), Some(cut)) = (&texture, cutoff)
                && images.get(h).is_some_and(|i| i.data.is_some())
                && let Some(mut img) = images.get_mut(h)
            {
                keep_coverage(&mut img, cut);
            }
            // `blendDest one` adds to what is behind (flashes, fire, GC's lightsabers and track lights): Bevy's
            // [`AlphaMode::Add`] (colour × alpha, added); `lighting false`: as it is
            let key = |k: &str| sub.and_then(|s| s.keys.get(k)).map(|v| v.trim().trim_end_matches(';').to_ascii_lowercase());
            let additive = sub.is_some_and(|s| s.transparent) && key("blenddest").as_deref() == Some("one");
            let alpha = match sub {
                _ if additive => AlphaMode::Add,
                Some(s) if s.transparent => AlphaMode::Blend,
                Some(_) if own.is_some() => cut_by_alpha(own.unwrap()),
                _ => AlphaMode::Opaque,
            };
            let mat = materials.add(StandardMaterial {
                base_color_texture: texture,
                alpha_mode: alpha,
                unlit: key("lighting").as_deref() == Some("false"),
                double_sided: sub.is_some_and(|s| s.two_sided),
                cull_mode: if sub.is_some_and(|s| s.two_sided) { None } else { Some(bevy::render::render_resource::Face::Back) },
                perceptual_roughness: 0.9,
                // (no specular: the textures were made for D3D's light without it. With Bevy's 0.04 the sun's sheen
                // turned the dark metal of the weapons flat grey, the enhanced profile)
                reflectance: 0.0,
                ..default()
            });
            // (the glass too, blended: the original draws the Mi-8's windscreen with the same stage 1 on the cube, blended 5/6, and with
            // the reflection on both tint the sand and sky alike, 230,198,139 against 231,197,138. The additive ones, no: flashes and fire)
            if key("envmap").as_deref() == Some("true") && key("lighting").as_deref() != Some("false") && !additive {
                envmapped.insert(mat.id());
            }
            if key("lighting").as_deref() != Some("false") && !additive {
                lit.insert(mat.id(), Vec3::from(sub.map_or([1.0; 3], |s| s.diffuse)));
            }
            if key("texturefade").as_deref() == Some("true") {
                faded.insert(mat.id());
            }
            if key("lighting").as_deref() == Some("false") && sub.is_some_and(|s| s.transparent) && !additive {
                unlit_blended.insert(mat.id());
            }
            (meshes.add(sm_mesh(m)), mat)
        })
        .collect()
}

/// Trunks and branches as they are; sprites with their corners' offsets on the second UVs:
/// `shaders/tree.wesl` turns them to the camera (the game's `Shaders/Tree/Sprite`).
fn load_tm(
    vfs: &Vfs,
    file: &str,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    texcache: &mut Textures,
    light: Color,
) -> Parts {
    let Ok(bytes) = vfs.read(&format!("treemesh/{file}.tm")) else { return Vec::new() };
    let Ok(t) = tm::parse(&bytes) else { return Vec::new() };
    let mut out = Vec::new();
    let colours: Vec<[f32; 4]> = t.colours.iter().map(|c| [c[2] as f32 / 255.0, c[1] as f32 / 255.0, c[0] as f32 / 255.0, c[3] as f32 / 255.0]).collect();
    let mut emit = |pos: Vec<[f32; 3]>, nor: Vec<[f32; 3]>, uv: Vec<[f32; 2]>, idx: Vec<u32>, tex: &str, alpha: AlphaMode, offsets: Vec<[f32; 2]>| {
        if idx.is_empty() {
            return;
        }
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, offsets)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours.clone())
            .with_inserted_indices(Indices::U32(idx));
        let mask = !matches!(alpha, AlphaMode::Opaque);
        let mat = materials.add(StandardMaterial {
            base_color_texture: texture(vfs, tex, false, images, texcache),
            alpha_mode: alpha,
            double_sided: mask,
            cull_mode: if mask { None } else { Some(bevy::render::render_resource::Face::Back) },
            perceptual_roughness: 1.0,
            // unlit, at a constant light ([`crate::level::Atmosphere::tree`]): with Bevy's lights the palms came out
            // near black, and their leaves are two-sided
            base_color: light,
            unlit: true,
            ..default()
        });
        out.push((meshes.add(mesh), mat));
    };
    let pos: Vec<[f32; 3]> = t.positions.iter().map(|p| [p[0], p[1], -p[2]]).collect();
    let nor: Vec<[f32; 3]> = t.normals.iter().map(|n| [n[0], n[1], -n[2]]).collect();
    let tri = |b: &tm::Block| -> Vec<u32> {
        let (a, n) = (b.first_index as usize, b.triangles as usize * 3);
        t.indices.get(a..a + n).map(|s| s.chunks_exact(3).flat_map(|c| [c[0] as u32, c[2] as u32, c[1] as u32]).collect()).unwrap_or_default()
    };
    for b in &t.trunks {
        emit(pos.clone(), nor.clone(), t.uvs.clone(), tri(b), &b.texture, AlphaMode::Opaque, vec![[0.0; 2]; pos.len()]);
    }
    // (blended in the original, with no alpha test; in a Kharkov fir at 1 m the cut leaves a solid fan)
    let branch = if matches!(cut_by_alpha(0.5), AlphaMode::AlphaToCoverage) { AlphaMode::AlphaToCoverage } else { AlphaMode::Blend };
    for b in &t.branches {
        emit(pos.clone(), nor.clone(), t.uvs.clone(), tri(b), &b.texture, branch, vec![[0.0; 2]; pos.len()]);
    }
    // (as the original: blended, without writing the depth, `Tree/Sprite`'s draws in the original's rendering)
    for b in &t.sprites {
        emit(pos.clone(), nor.clone(), t.uvs.clone(), tri(b), &b.texture, AlphaMode::Blend, t.sprite_offsets.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    /// The M1A1's rear rack has no `alphaTestRef` in its `.rs`: its geometry's `alphaTestValue` cuts it (its gaps showed black).
    #[test]
    fn a_geometry_s_alpha_test_cuts_its_mesh() {
        let game = bytefield_testdata::require!(Base, Dc);
        let w = bytefield_sim::World::load(&game, "DC_Final", "Aberdeen", "conquest").unwrap();
        let mut cache = super::Cache::default();
        cache.set_alpha_tests(&w.db);
        let (mut meshes, mut materials, mut images) = (Assets::<Mesh>::default(), Assets::<StandardMaterial>::default(), Assets::<Image>::default());
        let parts = cache.get(&w.vfs, "standardmesh", "/DesertCombat/M1A1/M1A1_rack", &mut meshes, &mut materials, &mut images);
        assert!(!parts.is_empty());
        for (_, m) in parts {
            assert!(matches!(materials.get(&m).unwrap().alpha_mode, AlphaMode::Mask(_) | AlphaMode::AlphaToCoverage), "{:?}", materials.get(&m).unwrap().alpha_mode);
        }
    }

    /// El Alamein's `textureManager.alternativePath Texture/Africa`: the Panzer IV's `texture/P4Main_f` is
    /// the desert one, `texture/Africa/p4main_f`.
    #[test]
    fn the_level_s_alternative_path_comes_first() {
        let game = bytefield_testdata::require!(Base);
        let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut images = Assets::<Image>::default();
        let mut desert = super::Textures::default();
        desert.alternative = vec!["Texture/Africa".into()];
        let a = super::texture(&w.vfs, "texture/P4Main_f", true, &mut images, &mut desert).unwrap();
        let b = super::texture(&w.vfs, "texture/P4Main_f", true, &mut images, &mut super::Textures::default()).unwrap();
        let red = |h: &Handle<Image>| {
            let d = images.get(h).unwrap().data.clone().unwrap();
            d.chunks(4).map(|p| p[0] as f64 - p[2] as f64).sum::<f64>() / (d.len() / 4) as f64
        };
        // (sand is redder than blue; the European grey is not)
        assert!(red(&a) > red(&b) + 10.0, "{} {}", red(&a), red(&b));
    }

    /// DC_Coastal_Hammer's own BMP-2 (`CustomTextures/BMP-2.dds`) for DC's `texture/DesertCombat/BMP2/BMP-2`.
    #[test]
    fn the_alternative_path_takes_the_file_s_name() {
        let Some(game) = bytefield_testdata::with_mod("DC_Final") else { return; };
        let w = bytefield_sim::World::load(&game, "DC_Final", "DC_Coastal_Hammer", "conquest").unwrap();
        let mut images = Assets::<Image>::default();
        let mut own = super::Textures::default();
        own.alternative = vec!["bf1942/levels/DC_Coastal_Hammer/CustomTextures".into()];
        let a = super::texture(&w.vfs, "texture/DesertCombat/BMP2/BMP-2", true, &mut images, &mut own).unwrap();
        let b = super::texture(&w.vfs, "texture/DesertCombat/BMP2/BMP-2", true, &mut images, &mut super::Textures::default()).unwrap();
        assert_ne!(images.get(&a).unwrap().data, images.get(&b).unwrap().data);
    }

    /// GC_Ryloth's imperial hangar asks `textures/custom/trench`: only its `Textures/Custom` has it (it was white).
    #[test]
    fn the_alternative_path_serves_any_name() {
        let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
        let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Ryloth", "conquest").unwrap();
        let mut images = Assets::<Image>::default();
        let mut own = super::Textures::default();
        own.alternative = vec!["bf1942/levels/GC_ryloth/Textures/Custom".into()];
        assert!(super::texture(&w.vfs, "textures/custom/trench", true, &mut images, &mut own).is_some());
        assert!(super::texture(&w.vfs, "textures/custom/trench", true, &mut images, &mut super::Textures::default()).is_none());
    }

    /// DC_Bridge names three: its sky is in the second, not in the last.
    #[test]
    fn all_the_alternative_paths_count() {
        let Some(game) = bytefield_testdata::with_mod("DC_Final") else { return; };
        let w = bytefield_sim::World::load(&game, "DC_Final", "DC_Bridge", "conquest").unwrap();
        let mut images = Assets::<Image>::default();
        let mut cache = super::Cache::default();
        cache.set_alternative_paths(["Texture/Africa", "bf1942/levels/DC_Bridge/Textures/", "bf1942/levels/DC_Bridge/texture/"].into_iter());
        let a = cache.texture(&w.vfs, "texture/Sky_Bocage_01", &mut images).unwrap();
        let b = super::texture(&w.vfs, "texture/Sky_Bocage_01", false, &mut images, &mut super::Textures::default()).unwrap();
        assert_ne!(a, b);
        assert_ne!(images.get(&a).unwrap().data, images.get(&b).unwrap().data);
    }

    /// the shadow LOD as one mesh: its materials one after the other, z mirrored, winding turned; none without one
    #[test]
    fn shadow_lod_as_one_mesh() {
        let mat = |n: usize| bytefield_formats::sm::MeshMaterial { name: String::new(), settings: 0, positions: (0..3 * n).map(|k| [k as f32, 0.0, 1.0]).collect(), normals: Vec::new(), uvs: Vec::new(), lightmap_uvs: Vec::new(), triangles: (0..n as u16).map(|t| [3 * t, 3 * t + 1, 3 * t + 2]).collect() };
        let mut m = bytefield_formats::sm::StandardMesh { version: 10, bounds: [[0.0; 3]; 2], collision: Vec::new(), lods: vec![bytefield_formats::sm::Lod { materials: vec![mat(5)] }], shadow_lods: Vec::new(), portals: Vec::new(), unknown_sections: 0 };
        assert!(super::shadow_lod_mesh(&m).is_none());
        m.shadow_lods.push(bytefield_formats::sm::Lod { materials: vec![mat(2), mat(1)] });
        let mesh = super::shadow_lod_mesh(&m).unwrap();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        assert_eq!(pos.len(), 9);
        assert_eq!(pos[0], [0.0, 0.0, -1.0]);
        let Some(bevy::mesh::Indices::U32(idx)) = mesh.indices() else { panic!() };
        assert_eq!(idx, &[0, 2, 1, 3, 5, 4, 6, 8, 7]);
    }
}
