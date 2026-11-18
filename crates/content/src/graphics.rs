//! The graphics profile: how the engine draws, apart from what it draws (the packs).
//! `Original` is the original's look; `Enhanced` takes what our renderer can do beyond it.
//! Loose settings win over the profile, and the variables over both
//! (checks and captures): each setting has a key ([`KEYS`]), the same in `content.cfg`, in
//! `bytefield-content set` and, upper-cased after `BYTEFIELD_`, as a variable (`BYTEFIELD_ANISO`, `BYTEFIELD_AA`…).

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Profile {
    #[default]
    Original,
    Enhanced,
}

impl Profile {
    pub fn parse(s: &str) -> Option<Profile> {
        match s.to_lowercase().as_str() {
            "original" | "vanilla" => Some(Profile::Original),
            "enhanced" | "mejorado" => Some(Profile::Enhanced),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Profile::Original => "original",
            Profile::Enhanced => "enhanced",
        }
    }
}

/// An enum of settings with its names, in the order the menu cycles them.
macro_rules! named {
    ($(#[$m:meta])* $t:ident { $($v:ident = $n:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $t { $($v),+ }

        impl $t {
            pub const ALL: &[$t] = &[$($t::$v),+];

            pub fn parse(s: &str) -> Option<$t> {
                let s = s.trim();
                Self::ALL.iter().copied().find(|v| v.name().eq_ignore_ascii_case(s))
            }

            pub fn name(self) -> &'static str {
                match self { $($t::$v => $n),+ }
            }
        }
    };
}

named!(
    /// The anti-aliasing: Bevy's 4× MSAA (what we have always drawn with), or one of its post-process passes
    /// (no MSAA then: they want it off), the temporal one, or none.
    Aa { Off = "off", Msaa = "msaa", Fxaa = "fxaa", Smaa = "smaa", Taa = "taa" }
);

named!(
    /// The sun's shadow maps: their size, how many cascades and how far
    ShadowQuality { Low = "low", Medium = "medium", High = "high" }
);

named!(
    /// The colour grading over everything
    Look { Neutral = "neutral", Vivid = "vivid", Warm = "warm" }
);

impl Aa {
    /// also `0`/`none` for off
    fn parse_loose(s: &str) -> Option<Aa> {
        if matches!(s.trim(), "0" | "none") { Some(Aa::Off) } else { Aa::parse(s) }
    }
}

/// The settings that may be given one by one (None: the profile's).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Overrides {
    pub aniso: Option<u16>,
    pub max_texture: Option<usize>,
    pub mip_bias: Option<f32>,
    pub shadows: Option<bool>,
    pub shadow_quality: Option<ShadowQuality>,
    pub aa: Option<Aa>,
    pub sharpen: Option<f32>,
    pub reflections: Option<bool>,
    pub coverage: Option<bool>,
    pub bloom: Option<f32>,
    pub look: Option<Look>,
    pub vignette: Option<f32>,
    pub view_distance: Option<f32>,
    pub motion_blur: Option<f32>,
    pub ssao: Option<bool>,
    pub contact_shadows: Option<bool>,
    pub soft_particles: Option<bool>,
    pub dof: Option<bool>,
    pub render_scale: Option<f32>,
    pub water_fx: Option<bool>,
    pub dyn_lights: Option<bool>,
    pub oit: Option<bool>,
    pub lightmaps: Option<bool>,
    pub object_shadows: Option<bool>,
    pub envmap: Option<bool>,
    pub fixed_light: Option<bool>,
    pub water_gamma: Option<bool>,
    pub mesh_shade: Option<bool>,
}

/// The loose settings' keys, in the order they are written.
pub const KEYS: &[&str] = &["aniso", "max_texture", "mip_bias", "shadows", "shadow_quality", "aa", "sharpen", "reflections", "coverage", "bloom", "look", "vignette", "view_distance", "motion_blur", "ssao", "contact_shadows", "soft_particles", "dof", "render_scale", "water_fx", "dyn_lights", "oit", "lightmaps", "object_shadows", "envmap", "fixed_light", "water_gamma", "mesh_shade"];

impl Overrides {
    /// Sets `key` from its text (`-` clears it). Err: not a key, or not one of its values.
    pub fn set(&mut self, key: &str, v: &str) -> Result<(), String> {
        let v = v.trim();
        let clear = v == "-";
        let bad = || format!("{key}: not a value: {v}");
        let num = |lo: f32, hi: f32| v.parse::<f32>().ok().filter(|x| x.is_finite()).map(|x| x.clamp(lo, hi)).ok_or_else(bad);
        macro_rules! put {
            ($f:ident, $e:expr) => {
                self.$f = if clear { None } else { Some($e) }
            };
        }
        match key {
            "aniso" => put!(aniso, v.parse().map_err(|_| bad())?),
            "max_texture" => put!(max_texture, v.parse().ok().filter(|m| *m >= 1).ok_or_else(bad)?),
            "mip_bias" => put!(mip_bias, num(-4.0, 4.0)?),
            "shadows" => put!(shadows, parse_bool(v).ok_or_else(bad)?),
            "shadow_quality" => put!(shadow_quality, ShadowQuality::parse(v).ok_or_else(bad)?),
            "aa" => put!(aa, Aa::parse_loose(v).ok_or_else(bad)?),
            "sharpen" => put!(sharpen, num(0.0, 1.0)?),
            "reflections" => put!(reflections, parse_bool(v).ok_or_else(bad)?),
            "coverage" => put!(coverage, parse_bool(v).ok_or_else(bad)?),
            "bloom" => put!(bloom, num(0.0, 1.0)?),
            "look" => put!(look, Look::parse(v).ok_or_else(bad)?),
            "vignette" => put!(vignette, num(0.0, 1.0)?),
            "view_distance" => put!(view_distance, num(0.5, 4.0)?),
            "motion_blur" => put!(motion_blur, num(0.0, 1.0)?),
            "ssao" => put!(ssao, parse_bool(v).ok_or_else(bad)?),
            "contact_shadows" => put!(contact_shadows, parse_bool(v).ok_or_else(bad)?),
            "soft_particles" => put!(soft_particles, parse_bool(v).ok_or_else(bad)?),
            "dof" => put!(dof, parse_bool(v).ok_or_else(bad)?),
            "render_scale" => put!(render_scale, num(0.5, 1.0)?),
            "water_fx" => put!(water_fx, parse_bool(v).ok_or_else(bad)?),
            "dyn_lights" => put!(dyn_lights, parse_bool(v).ok_or_else(bad)?),
            "oit" => put!(oit, parse_bool(v).ok_or_else(bad)?),
            "lightmaps" => put!(lightmaps, parse_bool(v).ok_or_else(bad)?),
            "object_shadows" => put!(object_shadows, parse_bool(v).ok_or_else(bad)?),
            "envmap" => put!(envmap, parse_bool(v).ok_or_else(bad)?),
            "fixed_light" => put!(fixed_light, parse_bool(v).ok_or_else(bad)?),
            "water_gamma" => put!(water_gamma, parse_bool(v).ok_or_else(bad)?),
            "mesh_shade" => put!(mesh_shade, parse_bool(v).ok_or_else(bad)?),
            _ => return Err(format!("{key}: one of {}", KEYS.join(", "))),
        }
        Ok(())
    }

    /// The ones given, as `(key, value)`, in [`KEYS`]' order.
    pub fn entries(&self) -> Vec<(&'static str, String)> {
        let b = |x: bool| if x { "1" } else { "0" }.to_string();
        let all = [
            self.aniso.map(|x| x.to_string()),
            self.max_texture.map(|x| x.to_string()),
            self.mip_bias.map(|x| x.to_string()),
            self.shadows.map(b),
            self.shadow_quality.map(|x| x.name().to_string()),
            self.aa.map(|x| x.name().to_string()),
            self.sharpen.map(|x| x.to_string()),
            self.reflections.map(b),
            self.coverage.map(b),
            self.bloom.map(|x| x.to_string()),
            self.look.map(|x| x.name().to_string()),
            self.vignette.map(|x| x.to_string()),
            self.view_distance.map(|x| x.to_string()),
            self.motion_blur.map(|x| x.to_string()),
            self.ssao.map(b),
            self.contact_shadows.map(b),
            self.soft_particles.map(b),
            self.dof.map(b),
            self.render_scale.map(|x| x.to_string()),
            self.water_fx.map(b),
            self.dyn_lights.map(b),
            self.oit.map(b),
            self.lightmaps.map(b),
            self.object_shadows.map(b),
            self.envmap.map(b),
            self.fixed_light.map(b),
            self.water_gamma.map(b),
            self.mesh_shade.map(b),
        ];
        KEYS.iter().zip(all).filter_map(|(k, v)| Some((*k, v?))).collect()
    }

    /// Keeps only what differs from the profile's (what the menu leaves after a change).
    pub fn without_defaults(mut self, profile: Profile) -> Overrides {
        let base = Graphics::of(profile, &Overrides::default());
        macro_rules! drop_same {
            ($($f:ident),+) => { $(if self.$f == Some(base.$f) { self.$f = None; })+ };
        }
        drop_same!(aniso, max_texture, mip_bias, shadows, shadow_quality, aa, sharpen, reflections, coverage, bloom, look, vignette, view_distance, motion_blur, ssao, contact_shadows, soft_particles, dof, render_scale, water_fx, dyn_lights, oit, lightmaps, object_shadows, envmap, fixed_light, water_gamma, mesh_shade);
        self
    }
}

/// What the renderer uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Graphics {
    pub profile: Profile,
    /// anisotropic filtering: 1, 2, 4, 8 or 16
    pub aniso: u16,
    /// the largest side a texture keeps
    pub max_texture: usize,
    /// the cameras' mip bias (the HD Remaster's `VideoDefault.con` says −1)
    pub mip_bias: f32,
    /// the sun's shadows from what has `hasDynamicShadow 1` (soldiers, vehicles, hand weapons)
    pub shadows: bool,
    pub shadow_quality: ShadowQuality,
    pub aa: Aa,
    /// contrast-adaptive sharpening (AMD's CAS) after everything, 0–1; 0 is off
    pub sharpen: f32,
    /// the water reflects the world
    pub reflections: bool,
    /// alpha to coverage for what is cut by its alpha (foliage, fences): soft edges under MSAA, nothing without it
    pub coverage: bool,
    /// the glow of what is brighter than white (flashes, fire), 0–1; 0 is off
    pub bloom: f32,
    pub look: Look,
    /// the screen's corners darkened, 0–1; 0 is off
    pub vignette: f32,
    /// the fog's start and end times this (1: the level's, as the original)
    pub view_distance: f32,
    /// the blur of what moves on the screen (the shutter's angle, 0–1; 0 is off), on the world's camera
    pub motion_blur: f32,
    /// screen-space ambient occlusion on the world's camera
    pub ssao: bool,
    /// the sun's short shadows marched in the depth (feet, wheels), on the world's camera
    pub contact_shadows: bool,
    /// the effects' sprites fade where they near what is behind them (the world's depth), not cut by it
    pub soft_particles: bool,
    /// depth of field while aiming through a sight (`play::Zoom`): what is far from the aimed point blurs
    pub dof: bool,
    /// the world drawn at this part of the window's size (0.5–1) and scaled up
    pub render_scale: f32,
    /// the water refracts what is under it and foams where it is shallow
    pub water_fx: bool,
    /// flashes and explosions light what is around them for a moment
    pub dyn_lights: bool,
    /// what is blended (smoke, flashes, glass, water) sorted by pixel, not by object (Bevy's OIT; no MSAA with it)
    pub oit: bool,
    /// the statics' baked light (`ObjectLightmaps`, the original's `game.setLightmaps 1` at medium and high quality)
    /// Texture × the level's palette × 2, with no light of their own
    pub lightmaps: bool,
    /// the original's shadows of soldiers and vehicles on the terrain:
    /// their silhouettes from the sun, near the camera; the enhanced profile has the sun's instead
    pub object_shadows: bool,
    /// the original's reflection on vehicles and weapons (`envmap true` in their `.rs`, `game.setEnvironmentMapping 1`
    /// at high quality): the level's cube, where the texture's alpha leaves room
    pub envmap: bool,
    /// the original's light on lit meshes: texture × 2 min(A + D N·L, 1) in the
    /// screen's space, no specular; the enhanced profile has Bevy's
    pub fixed_light: bool,
    /// The water and the trees' branches and sprites blended over the
    /// world in the screen's (gamma) space, as D3D does, not in Bevy's linear one
    pub water_gamma: bool,
    /// Vehicles, soldiers and small statics darkened in the terrain's baked shadow as the original does (its
    /// TEXTUREFACTOR; with `fixed_light`)
    pub mesh_shade: bool,
}

impl Graphics {
    /// Whether the cameras multisample: SSAO and the contact shadows read a depth that is not
    /// multisampled, and Bevy's OIT does not take MSAA (it panics)
    pub fn msaa(&self) -> bool {
        self.aa == Aa::Msaa && !self.ssao && !self.contact_shadows && !self.oit
    }

    /// The profile's values, under the overrides.
    pub fn of(profile: Profile, o: &Overrides) -> Graphics {
        let base = match profile {
            Profile::Original => Graphics {
                profile,
                aniso: 1,
                max_texture: 2048,
                mip_bias: 0.0,
                shadows: false,
                shadow_quality: ShadowQuality::Medium,
                aa: Aa::Msaa,
                sharpen: 0.0,
                reflections: false,
                coverage: false,
                bloom: 0.0,
                look: Look::Neutral,
                vignette: 0.0,
                view_distance: 1.0,
                motion_blur: 0.0,
                ssao: false,
                contact_shadows: false,
                soft_particles: false,
                dof: false,
                render_scale: 1.0,
                water_fx: false,
                dyn_lights: false,
                oit: false,
                lightmaps: true,
                object_shadows: true,
                envmap: true,
                fixed_light: true,
                water_gamma: true,
                mesh_shade: true,
            },
            // (its post-process: SMAA, no CAS. With `water_gamma`, blended in linear space:
            // the shallow water came out as a light sheet with straight edges, Wake and Iwo Jima)
            Profile::Enhanced => Graphics { profile, aniso: 16, max_texture: 4096, mip_bias: -1.0, shadows: true, aa: Aa::Smaa, reflections: true, coverage: true, object_shadows: false, fixed_light: false, mesh_shade: false, ..Graphics::of(Profile::Original, &Overrides::default()) },
        };
        Graphics {
            profile,
            aniso: snap_aniso(o.aniso.unwrap_or(base.aniso)),
            max_texture: o.max_texture.filter(|m| *m >= 1).unwrap_or(base.max_texture),
            mip_bias: o.mip_bias.unwrap_or(base.mip_bias),
            shadows: o.shadows.unwrap_or(base.shadows),
            shadow_quality: o.shadow_quality.unwrap_or(base.shadow_quality),
            aa: o.aa.unwrap_or(base.aa),
            sharpen: o.sharpen.unwrap_or(base.sharpen).clamp(0.0, 1.0),
            reflections: o.reflections.unwrap_or(base.reflections),
            coverage: o.coverage.unwrap_or(base.coverage),
            bloom: o.bloom.unwrap_or(base.bloom).clamp(0.0, 1.0),
            look: o.look.unwrap_or(base.look),
            vignette: o.vignette.unwrap_or(base.vignette).clamp(0.0, 1.0),
            view_distance: o.view_distance.unwrap_or(base.view_distance).clamp(0.5, 4.0),
            motion_blur: o.motion_blur.unwrap_or(base.motion_blur).clamp(0.0, 1.0),
            ssao: o.ssao.unwrap_or(base.ssao),
            contact_shadows: o.contact_shadows.unwrap_or(base.contact_shadows),
            soft_particles: o.soft_particles.unwrap_or(base.soft_particles),
            dof: o.dof.unwrap_or(base.dof),
            render_scale: o.render_scale.unwrap_or(base.render_scale).clamp(0.5, 1.0),
            water_fx: o.water_fx.unwrap_or(base.water_fx),
            dyn_lights: o.dyn_lights.unwrap_or(base.dyn_lights),
            oit: o.oit.unwrap_or(base.oit),
            lightmaps: o.lightmaps.unwrap_or(base.lightmaps),
            object_shadows: o.object_shadows.unwrap_or(base.object_shadows),
            envmap: o.envmap.unwrap_or(base.envmap),
            fixed_light: o.fixed_light.unwrap_or(base.fixed_light),
            water_gamma: o.water_gamma.unwrap_or(base.water_gamma),
            mesh_shade: o.mesh_shade.unwrap_or(base.mesh_shade),
        }
    }

    /// [`Graphics::of`], then the variables (`BYTEFIELD_<KEY>`; a bad value is skipped).
    pub fn resolve(profile: Profile, o: &Overrides) -> Graphics {
        let mut o = *o;
        for k in KEYS {
            if let Ok(v) = std::env::var(format!("BYTEFIELD_{}", k.to_uppercase())) {
                let _ = o.set(k, &v);
            }
        }
        Graphics::of(profile, &o)
    }

    /// One line for the log and `bytefield-content list`.
    pub fn describe(&self) -> String {
        let on = |b: bool| if b { "on" } else { "off" };
        format!(
            "{} (aniso {}x, textures up to {}, mip bias {}, shadows {} {}, aa {}, sharpen {}, reflections {}, coverage {}, bloom {}, look {}, vignette {}, view distance {}, motion blur {}, ssao {}, contact shadows {}, soft particles {}, dof {}, render scale {}, water fx {}, dynamic lights {}, oit {}, lightmaps {}, object shadows {}, envmap {}, fixed light {})",
            self.profile.name(),
            self.aniso,
            self.max_texture,
            self.mip_bias,
            on(self.shadows),
            self.shadow_quality.name(),
            self.aa.name(),
            self.sharpen,
            on(self.reflections),
            on(self.coverage),
            self.bloom,
            self.look.name(),
            self.vignette,
            self.view_distance,
            self.motion_blur,
            on(self.ssao),
            on(self.contact_shadows),
            on(self.soft_particles),
            on(self.dof),
            self.render_scale,
            on(self.water_fx),
            on(self.dyn_lights),
            on(self.oit),
            on(self.lightmaps),
            on(self.object_shadows),
            on(self.envmap),
            on(self.fixed_light)
        )
    }
}

/// 1/0, on/off, true/false
pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "1" | "on" | "true" | "yes" => Some(true),
        "0" | "off" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn snap_aniso(a: u16) -> u16 {
    [1u16, 2, 4, 8, 16].into_iter().rfind(|&n| n <= a).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles() {
        let g = Graphics::of(Profile::Original, &Overrides::default());
        assert_eq!((g.aniso, g.max_texture, g.mip_bias, g.shadows), (1, 2048, 0.0, false));
        assert_eq!((g.aa, g.sharpen, g.coverage, g.bloom, g.look, g.vignette, g.view_distance), (Aa::Msaa, 0.0, false, 0.0, Look::Neutral, 0.0, 1.0));
        let g = Graphics::of(Profile::Enhanced, &Overrides { aniso: Some(6), ..Default::default() });
        assert_eq!((g.aniso, g.max_texture, g.mip_bias, g.shadows, g.shadow_quality), (4, 4096, -1.0, true, ShadowQuality::Medium));
        let g = Graphics::of(Profile::Enhanced, &Overrides { shadows: Some(false), ..Default::default() });
        assert!(!g.shadows);
        let mut o = Overrides::default();
        o.set("aa", "FXAA").unwrap();
        o.set("sharpen", "2").unwrap();
        let g = Graphics::of(Profile::Enhanced, &o);
        assert_eq!((g.aa, g.sharpen), (Aa::Fxaa, 1.0));
        assert!(g.reflections && !Graphics::of(Profile::Original, &Overrides::default()).reflections);
        // (MSAA goes with SSAO, the contact shadows and OIT)
        assert!(Graphics::of(Profile::Original, &Overrides::default()).msaa());
        assert!(!Graphics::of(Profile::Original, &Overrides { oit: Some(true), ..Default::default() }).msaa());
    }

    #[test]
    fn every_key_there_and_back() {
        let mut o = Overrides::default();
        for (k, v) in KEYS.iter().zip(["8", "1024", "-0.5", "1", "high", "taa", "0.3", "1", "1", "0.2", "vivid", "0.4", "1.5", "0.5", "1", "1", "1", "1", "0.75", "1", "1", "1", "0", "0", "0", "0", "1", "0"]) {
            o.set(k, v).unwrap();
        }
        let e = o.entries();
        assert_eq!(e.len(), KEYS.len());
        let mut back = Overrides::default();
        for (k, v) in &e {
            back.set(k, v).unwrap();
        }
        assert_eq!(back, o);
        o.set("look", "-").unwrap();
        assert_eq!(o.look, None);
        assert!(o.set("look", "sepia").is_err() && o.set("nope", "1").is_err());
        assert_eq!(Aa::parse_loose("none"), Some(Aa::Off));
    }

    #[test]
    fn without_defaults_keeps_what_differs() {
        let o = Overrides { aa: Some(Aa::Smaa), bloom: Some(0.0), vignette: Some(0.3), ..Default::default() };
        assert_eq!(o.without_defaults(Profile::Enhanced), Overrides { vignette: Some(0.3), ..Default::default() });
        assert_eq!(o.without_defaults(Profile::Original), Overrides { aa: Some(Aa::Smaa), vignette: Some(0.3), ..Default::default() });
    }
}
