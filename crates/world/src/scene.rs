//! A level's placed objects expanded into drawable meshes, in world space.
//!
//! An instance names an ObjectTemplate. A template either has geometry
//! (`ObjectTemplate.geometry <GeometryTemplate>`) or children: each `addTemplate X` adds a
//! child, and the `setPosition`/`setRotation` lines after it (until the next `addTemplate`)
//! place that child in the parent's frame. A `LodObject` lists its levels of detail as
//! children: only the first (the most detailed) is drawn here.
//!
//! Transforms stay in Refractor's frame (left-handed, Y up). Rotations are `yaw/pitch/roll` in
//! degrees, applied roll, then pitch, then yaw: rows Rz(roll)·Rx(pitch)·Ry(yaw), confirmed against
//! the original game's wing matrices. The client converts to its own frame.

use bytefield_con::templates::{Template, TemplateDb};

use crate::parse_vec;

/// Column-major 4x4, Refractor frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);

    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut r = [[0.0; 4]; 4];
        for (c, col) in r.iter_mut().enumerate() {
            for (row, v) in col.iter_mut().enumerate() {
                *v = (0..4).map(|k| self.0[k][row] * o.0[c][k]).sum();
            }
        }
        Mat4(r)
    }

    /// A scale per axis, in the object's own frame (`Object.geometry.scale`).
    pub fn from_scale(s: [f64; 3]) -> Mat4 {
        Mat4([[s[0], 0.0, 0.0, 0.0], [0.0, s[1], 0.0, 0.0], [0.0, 0.0, s[2], 0.0], [0.0, 0.0, 0.0, 1.0]])
    }

    /// Translation then rotation (yaw about Y, pitch about X, roll about Z), degrees.
    pub fn from_pos_ypr(pos: [f64; 3], ypr: [f64; 3]) -> Mat4 {
        let (y, p, r) = (ypr[0].to_radians(), ypr[1].to_radians(), ypr[2].to_radians());
        let ry = Mat4([[y.cos(), 0.0, -y.sin(), 0.0], [0.0, 1.0, 0.0, 0.0], [y.sin(), 0.0, y.cos(), 0.0], [0.0, 0.0, 0.0, 1.0]]);
        let rx = Mat4([[1.0, 0.0, 0.0, 0.0], [0.0, p.cos(), p.sin(), 0.0], [0.0, -p.sin(), p.cos(), 0.0], [0.0, 0.0, 0.0, 1.0]]);
        let rz = Mat4([[r.cos(), r.sin(), 0.0, 0.0], [-r.sin(), r.cos(), 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        let mut m = ry.mul(&rx).mul(&rz);
        m.0[3] = [pos[0], pos[1], pos[2], 1.0];
        m
    }

    /// The yaw, pitch and roll (degrees) of its rotation: [`Mat4::from_pos_ypr`]'s inverse (pitch
    /// within ±90°).
    pub fn ypr(&self) -> [f64; 3] {
        let m = &self.0;
        let p = (-m[2][1]).clamp(-1.0, 1.0).asin();
        [m[2][0].atan2(m[2][2]).to_degrees(), p.to_degrees(), m[0][1].atan2(m[1][1]).to_degrees()]
    }

    pub fn transform_point(&self, p: [f64; 3]) -> [f64; 3] {
        let m = &self.0;
        [0, 1, 2].map(|i| m[0][i] * p[0] + m[1][i] * p[1] + m[2][i] * p[2] + m[3][i])
    }
}

#[derive(Debug, Clone)]
pub struct Draw {
    /// the GeometryTemplate's `file` (`T-34_Hull_M1`), i.e. `standardmesh/<file>.sm`
    pub mesh: String,
    pub kind: String,
    /// the GeometryTemplate's name (`afr_house2_ste_m1`; DC's `DC_mktarch1_m1` has its `file` elsewhere): its
    /// lightmap's
    pub geometry: String,
    pub world: Mat4,
    /// the instance it came from (index into [`TemplateDb::instances`])
    pub instance: usize,
}

impl Draw {
    /// Its lightmap's name in `<level>/ObjectLightmaps/` (lowercase, without `.tga`): the GeometryTemplate's and where it
    /// is, each axis truncated.
    pub fn lightmap(&self) -> String {
        let p = |i: usize| self.world.0[3][i] as i64;
        format!("{}_{}-{}-{}", self.geometry, p(0), p(1), p(2)).to_ascii_lowercase()
    }
}

#[derive(Debug, Default)]
pub struct Expansion {
    pub draws: Vec<Draw>,
    pub missing_templates: usize,
    pub missing_geometry: usize,
    /// what was missing, `template` or `template -> geometry`, once each
    pub missing: std::collections::BTreeSet<String>,
    /// the `EffectBundle`s among what is placed: template, where, instance (Husky's `SmokeIdleXpack`, Omaha's `BuildingSmoke`)
    pub effects: Vec<(String, Mat4, usize)>,
}

pub(crate) fn vec3(args: &[String]) -> Option<[f64; 3]> {
    let v = parse_vec(args.first()?)?;
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
}

/// The children of a template with their local transforms, in order.
pub fn children(t: &Template) -> Vec<(String, Mat4)> {
    let mut out: Vec<(String, [f64; 3], [f64; 3])> = Vec::new();
    for p in &t.props {
        match p.name.as_str() {
            "addtemplate" => {
                if let Some(n) = p.args.first() {
                    out.push((n.clone(), [0.0; 3], [0.0; 3]));
                }
            }
            "setrandomgeometries" => {
                if let Some(last) = out.last_mut() {
                    last.0 = random_child(&last.0, &p.args);
                }
            }
            "setposition" | "position" => {
                if let (Some(last), Some(v)) = (out.last_mut(), vec3(&p.args)) {
                    last.1 = v;
                }
            }
            "setrotation" | "rotation" => {
                if let (Some(last), Some(v)) = (out.last_mut(), vec3(&p.args)) {
                    last.2 = v;
                }
            }
            _ => {}
        }
    }
    out.into_iter().map(|(n, p, r)| (n, Mat4::from_pos_ypr(p, r))).collect()
}

/// `setRandomGeometries n` on the last `addTemplate`: the server
/// creates `<name><k>` instead, k = 1..n from a counter shared by every bundle that goes round. We
/// always take 1: in the data the n variants differ only in their textures (DC's Pickup and Lada
/// hulls: the same collision mesh), so the physics is the same.
pub fn random_child(name: &str, args: &[String]) -> String {
    match args.first().and_then(|n| n.trim().parse::<i32>().ok()) {
        Some(n) if n >= 1 => format!("{name}1"),
        _ => name.to_string(),
    }
}

/// A `ControlPoint` with `setTeamGeometry team geometry`: its flag (the first child, an
/// `AnimatedBundle`) and, team by team, the geometry it takes (a
/// team without one, the neutral 0 unless it is given, shows none). The last line for a team wins.
pub fn team_flag(t: &Template) -> Option<((String, Mat4), Vec<(u32, String)>)> {
    if !t.kind.eq_ignore_ascii_case("controlpoint") {
        return None;
    }
    let mut teams: Vec<(u32, String)> = Vec::new();
    for p in t.props.iter().filter(|p| p.name == "setteamgeometry") {
        let (Some(team), Some(g)) = (p.args.first().and_then(|v| v.trim().parse::<u32>().ok()), p.args.get(1)) else { continue };
        teams.retain(|(k, _)| *k != team);
        teams.push((team, g.clone()));
    }
    if teams.is_empty() {
        return None;
    }
    Some((children(t).into_iter().next()?, teams))
}

fn expand(db: &TemplateDb, name: &str, world: Mat4, instance: usize, out: &mut Expansion, depth: usize) {
    if depth > 16 {
        return;
    }
    let Some(t) = db.object_template(name) else {
        out.missing_templates += 1;
        out.missing.insert(name.to_string());
        return;
    };
    if t.kind.eq_ignore_ascii_case("effectbundle") {
        out.effects.push((t.name.clone(), world, instance));
        return;
    }
    // `geometry null`: no mesh on purpose (DC_Sea_Rigs' control rooms), not a missing one
    if let Some(g) = t.get("geometry").and_then(|a| a.first()).filter(|g| !g.eq_ignore_ascii_case("null")) {
        match db.get("geometrytemplate", g) {
            Some(gt) => {
                if let Some(file) = gt.get("file").and_then(|a| a.first()) {
                    // `GeometryTemplate.scale` (none in vanilla, some in DC), on this draw and not on the children
                    let k = gt.get("scale").and_then(|a| a.first()?.trim().parse::<f64>().ok()).filter(|k| *k > 0.0).unwrap_or(1.0);
                    out.draws.push(Draw { mesh: file.replace('\\', "/"), kind: gt.kind.to_ascii_lowercase(), geometry: g.clone(), world: world.mul(&Mat4::from_scale([k; 3])), instance });
                }
            }
            None => {
                out.missing_geometry += 1;
                out.missing.insert(format!("{name} -> {g}"));
            }
        }
    }
    let mut kids = children(t);
    // a control point's flag (its first child) wears its team's geometry: the client's (`flags.rs`)
    if team_flag(t).is_some() {
        kids.drain(..1);
    }
    let lod = t.kind.eq_ignore_ascii_case("lodobject");
    for (child, local) in kids.iter().take(if lod { 1 } else { usize::MAX }) {
        expand(db, child, world.mul(local), instance, out, depth + 1);
    }
}

/// Every placed instance with a position, expanded.
pub fn expand_level(db: &TemplateDb) -> Expansion {
    let mut out = Expansion::default();
    for (i, inst) in db.instances.iter().enumerate() {
        let Some(world) = instance_world(inst) else { continue };
        expand(db, &inst.template, world, i, &mut out, 0);
    }
    out
}

/// Where a placed instance is (none without `absolutePosition`): its rotation, and `geometry.scale`
/// before it, on the whole object (as the simulation's obstacles).
pub fn instance_world(inst: &bytefield_con::templates::Instance) -> Option<Mat4> {
    let get = |n: &str| inst.props.iter().rev().find(|p| p.name == n).and_then(|p| vec3(&p.args));
    let pos = get("absoluteposition")?;
    let rot = get("rotation").or_else(|| get("setrotation")).unwrap_or([0.0; 3]);
    let scale = get("geometry.scale").unwrap_or([1.0; 3]);
    Some(Mat4::from_pos_ypr(pos, rot).mul(&Mat4::from_scale(scale)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
    }

    #[test]
    fn yaw_turns_about_y_and_translation_applies_last() {
        let m = Mat4::from_pos_ypr([10.0, 0.0, 0.0], [90.0, 0.0, 0.0]);
        // left-handed, yaw +90: +z (forward) goes to +x
        assert!(close(m.transform_point([0.0, 0.0, 1.0]), [11.0, 0.0, 0.0]));
        assert!(close(m.transform_point([0.0, 1.0, 0.0]), [10.0, 1.0, 0.0]));
    }

    #[test]
    fn ypr_undoes_from_pos_ypr() {
        for ypr in [[30.0, 20.0, -10.0], [-178.0, 0.0, 0.0], [95.0, -60.0, 170.0]] {
            assert!(close(Mat4::from_pos_ypr([1.0, 2.0, 3.0], ypr).ypr(), ypr), "{ypr:?}");
        }
    }

    #[test]
    fn the_scale_goes_before_the_rotation() {
        let m = Mat4::from_pos_ypr([10.0, 0.0, 0.0], [90.0, 0.0, 0.0]).mul(&Mat4::from_scale([1.0, 0.5, 2.0]));
        assert!(close(m.transform_point([0.0, 2.0, 1.0]), [12.0, 1.0, 0.0]));
    }

    #[test]
    fn a_lightmap_is_named_by_the_geometry_and_the_truncated_position() {
        // (El Alamein's `afr_house2_ste_m1_332-40-1958.tga`: 332.848/40.0523/1958.4)
        let d = Draw { mesh: "x".into(), kind: "standardmesh".into(), geometry: "Afr_House2_ste_m1".into(), world: Mat4::from_pos_ypr([332.848, 40.0523, 1958.4], [89.856, 0.0, 0.06]), instance: 0 };
        assert_eq!(d.lightmap(), "afr_house2_ste_m1_332-40-1958");
    }

    #[test]
    fn a_random_child_takes_its_first_variant() {
        // (DC's Pickup: the hull is `PickupCockpitExternal1..3`)
        let prop = |name: &str, arg: &str| bytefield_con::templates::Property { name: name.into(), args: vec![arg.into()] };
        let t = bytefield_con::templates::Template {
            namespace: "objecttemplate".into(),
            kind: "LodObject".into(),
            name: "lodPickupCockpit".into(),
            props: vec![prop("addtemplate", "PickupCockpitExternal"), prop("setrandomgeometries", "3"), prop("addtemplate", "PickupCockpitInternal")],
            origin: String::new(),
            seq: 0,
            create_args: Vec::new(),
        };
        let names: Vec<String> = children(&t).into_iter().map(|c| c.0).collect();
        assert_eq!(names, ["PickupCockpitExternal1", "PickupCockpitInternal"]);
    }

    #[test]
    fn composition() {
        let a = Mat4::from_pos_ypr([1.0, 2.0, 3.0], [30.0, 10.0, 5.0]);
        let b = Mat4::from_pos_ypr([-4.0, 0.5, 2.0], [-60.0, 0.0, 20.0]);
        let p = [0.3, -1.0, 2.0];
        assert!(close(a.mul(&b).transform_point(p), a.transform_point(b.transform_point(p))));
        assert_eq!(Mat4::IDENTITY.mul(&a), a);
    }
}
