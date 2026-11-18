//! The engine's portals: a building
//! whose mesh has a portal mesh (its doors and windows, `.sm` portal block) is a sector, and an
//! object that passes portals (`isPortalPassing 1`: the soldiers, at their position) is inside a
//! sector from when it goes through one of its portals inwards until it goes through one
//! outwards. The AI asks a mobile for its sector: that is the
//! cover it is at.
//!
//! The portal manager's update (each frame, the game's portals update): the segment from where the object
//! was to where it is; if it is in a sector, that sector's portals first (going out of it ends
//! the frame's search); else every sector along the segment, each crossed as many times as it
//! is. The crossings, in order along the segment, are events: into the sector when the segment
//! goes against the portal's normal, out of it otherwise.
//! Simplified: the crossing is the segment against the polygon (the engine's against its BSP
//! faces, the portal mesh's first portal intersection lookup), and the normal is the one `AITemplateCover`
//! works out (outwards in the stock meshes: checked on Berlin's), not the BSP face's.

use crate::vecmath::{sub, dot, cross};


/// An object of the level with an AI template, placed.
#[derive(Clone, Debug)]
pub struct Placed {
    pub template: String,
    pub ai: String,
    pub matrix: [f32; 16],
    /// its mesh's box in its frame (min, max) and portals
    pub bounds: [[f32; 3]; 2],
    pub portals: Vec<Vec<[f32; 3]>>,
}

/// A building with portals, placed in the level.
#[derive(Clone, Debug)]
pub struct Sector {
    /// the instance's template and its `aiTemplate` (a cover when it has a Cover plug-in)
    pub template: String,
    pub ai: Option<String>,
    /// rows, then its position (the AI's 4×4)
    pub matrix: [f32; 16],
    /// the portal polygons in its frame
    pub portals: Vec<Vec<[f32; 3]>>,
    /// the same in the world, with each one's outward normal
    world: Vec<(Vec<[f32; 3]>, [f32; 3])>,
    centre: [f32; 3],
    radius: f32,
}

/// A point by the AI's 4×4 (rows, then the position).
pub fn transform(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| m[i] * p[0] + m[4 + i] * p[1] + m[8 + i] * p[2] + m[12 + i])
}

/// A direction by the AI's 4×4.
pub fn rotate(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| m[i] * p[0] + m[4 + i] * p[1] + m[8 + i] * p[2])
}

/// A portal's normal as from object init works it out: (v2 − v0) × (v1 − v0)
/// (`ref2::cross`: each component in the FPU, stored as a float), made unit in the FPU (its length
/// summed z first).
pub fn portal_normal(poly: &[[f32; 3]]) -> [f32; 3] {
    let (a, b) = (sub(poly[2], poly[0]), sub(poly[1], poly[0]));
    let p = |x: f32, y: f32| x as f64 * y as f64;
    let n = [(p(a[1], b[2]) - p(a[2], b[1])) as f32, (p(a[2], b[0]) - p(a[0], b[2])) as f32, (p(a[0], b[1]) - p(a[1], b[0])) as f32];
    let l2 = (p(n[2], n[2]) + p(n[1], n[1])) + p(n[0], n[0]);
    let eps = f32::EPSILON as f64;
    if (l2 - 1.0).abs() < eps {
        return n;
    }
    if l2.abs() < eps {
        return [0.0; 3];
    }
    let k = 1.0 / l2.sqrt();
    n.map(|x| (x as f64 * k) as f32)
}

impl Sector {
    pub fn new(template: String, ai: Option<String>, matrix: [f32; 16], portals: Vec<Vec<[f32; 3]>>) -> Sector {
        let world: Vec<_> = portals.iter().filter(|p| p.len() >= 3).map(|p| (p.iter().map(|&v| transform(&matrix, v)).collect::<Vec<_>>(), rotate(&matrix, portal_normal(p)))).collect();
        let all: Vec<[f32; 3]> = world.iter().flat_map(|(p, _)| p.iter().copied()).collect();
        let n = all.len().max(1) as f32;
        let centre = [0, 1, 2].map(|i| all.iter().map(|v| v[i]).sum::<f32>() / n);
        let radius = all.iter().map(|&v| dot(sub(v, centre), sub(v, centre)).sqrt()).fold(0.0, f32::max);
        Sector { template, ai, matrix, portals, world, centre, radius }
    }

    /// Where along `from`→`to` (0..1) it goes through each portal, and whether inwards.
    fn crossings(&self, from: [f32; 3], to: [f32; 3], out: &mut Vec<(f32, bool)>) {
        let d = sub(to, from);
        // the broad phase: the segment's nearest point to the portals' sphere
        let l2 = dot(d, d);
        let t = if l2 > 0.0 { (dot(sub(self.centre, from), d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let near = [from[0] + d[0] * t, from[1] + d[1] * t, from[2] + d[2] * t];
        if dot(sub(near, self.centre), sub(near, self.centre)) > self.radius * self.radius {
            return;
        }
        for (poly, n) in &self.world {
            let dn = dot(d, *n);
            if dn == 0.0 {
                continue;
            }
            let t = dot(sub(poly[0], from), *n) / dn;
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            let q = [from[0] + d[0] * t, from[1] + d[1] * t, from[2] + d[2] * t];
            // inside the (convex) polygon: on the same side of every edge
            let mut sign = 0.0f32;
            let inside = (0..poly.len()).all(|i| {
                let s = dot(cross(sub(poly[(i + 1) % poly.len()], poly[i]), sub(q, poly[i])), *n);
                if sign == 0.0 {
                    sign = s;
                }
                s * sign >= 0.0
            });
            if inside {
                out.push((t, dn < 0.0));
            }
        }
    }
}

/// An object that passes portals: the sector it is in (an index of [`crate::World::sectors`])
/// and where it was last frame. It starts at the origin: it is registered
/// (`registerPortalPassingObject`, from the mobile object's init) before it is placed, so its
/// first segment runs from (0, 0, 0) to where it spawns, and a spawn whose line from the origin
/// goes in by a door of a building and out through a wall starts in that building (Berlin's
/// coop spawns: the original's reference run `ai_berlin_sectors`).
#[derive(Clone, Debug, Default)]
pub struct Passing {
    pub sector: Option<usize>,
    last: Option<[f32; 3]>,
}

impl Passing {
    /// It has not been updated yet.
    pub fn is_new(&self) -> bool {
        self.last.is_none()
    }

    /// The portal manager's update for this object, now at `pos`.
    pub fn update(&mut self, sectors: &[Sector], pos: [f32; 3]) {
        let from = self.last.replace(pos).unwrap_or([0.0; 3]);
        if from == pos {
            return;
        }
        if let Some(s) = self.sector {
            let mut c = Vec::new();
            sectors[s].crossings(from, pos, &mut c);
            if let Some(&(_, inwards)) = c.iter().min_by(|a, b| a.0.total_cmp(&b.0)) {
                if !inwards {
                    self.sector = None;
                }
                return;
            }
        }
        let mut events: Vec<(f32, usize, bool)> = Vec::new();
        for (i, s) in sectors.iter().enumerate() {
            let mut c = Vec::new();
            s.crossings(from, pos, &mut c);
            events.extend(c.into_iter().map(|(t, inwards)| (t, i, inwards)));
        }
        // (sorted by `PortalCollisionPred` on the intersection's distance, which
        // getFirstPortalIntersection measures from the far end: the first crossing along the
        // segment is the last event)
        events.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, i, inwards) in events {
            self.sector = if inwards { Some(i) } else { None };
        }
    }
}

impl crate::World {
    /// The level's buildings with portals: each placed
    /// object whose geometry (its own, or below its first LodObject: `getPortalMesh`,
    /// `findLodPortalMesh`) has a portal mesh.
    pub fn sectors(&self) -> &[Sector] {
        self.sectors.get_or_init(|| self.build_sectors())
    }

    /// Every object of the level with an `aiTemplate`, placed: its template, AI template, matrix
    /// (as a sector's), and its mesh's bounds and portals (none without a mesh).
    pub fn placed_ai_objects(&self) -> Vec<Placed> {
        let vec3 = |args: &[String]| args.first().and_then(|a| bytefield_world::parse_vec(a)).filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]);
        let mut out = Vec::new();
        for inst in &self.db.instances {
            let get = |n: &str| inst.props.iter().rev().find(|p| p.name == n).and_then(|p| vec3(&p.args));
            let Some(pos) = get("absoluteposition") else { continue };
            let rot = get("rotation").or_else(|| get("setrotation")).unwrap_or([0.0; 3]);
            let Some(t) = self.db.object_template(&inst.template) else { continue };
            let Some(ai) = t.get("aitemplate").and_then(|a| a.first()).cloned() else { continue };
            let mesh = self.physics_geometry(&inst.template).and_then(|g| self.mesh(&g));
            // (as the engine builds it: ref2::setRotation)
            let matrix = crate::ref2::placed(pos, rot);
            let (bounds, portals) = mesh.map_or(([[0.0; 3]; 2], vec![]), |m| (m.bounds, m.portals));
            out.push(Placed { template: t.name.clone(), ai, matrix, bounds, portals });
        }
        out
    }

    fn build_sectors(&self) -> Vec<Sector> {
        let vec3 = |args: &[String]| args.first().and_then(|a| bytefield_world::parse_vec(a)).filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]);
        let mut out = Vec::new();
        for inst in &self.db.instances {
            let get = |n: &str| inst.props.iter().rev().find(|p| p.name == n).and_then(|p| vec3(&p.args));
            let Some(pos) = get("absoluteposition") else { continue };
            let rot = get("rotation").or_else(|| get("setrotation")).unwrap_or([0.0; 3]);
            let Some(t) = self.db.object_template(&inst.template) else { continue };
            let Some(mesh) = self.physics_geometry(&inst.template).and_then(|g| self.mesh(&g)) else { continue };
            if mesh.portals.is_empty() {
                continue;
            }
            // (as the engine builds it: ref2::setRotation)
            let matrix = crate::ref2::placed(pos, rot);
            let ai = t.get("aitemplate").and_then(|a| a.first()).cloned();
            out.push(Sector::new(t.name.clone(), ai, matrix, mesh.portals));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square door at x = 5 facing +x: walking in through it puts the walker in the sector,
    /// walking out takes it out, walking past it does nothing.
    #[test]
    fn in_and_out_through_a_door() {
        // (v2 − v0) × (v1 − v0) is +x for this winding
        let door = vec![[5.0, 0.0, -1.0], [5.0, 0.0, 1.0], [5.0, 2.0, 1.0], [5.0, 2.0, -1.0]];
        assert_eq!(portal_normal(&door), [1.0, 0.0, 0.0]);
        let mut id = [0.0; 16];
        id[0] = 1.0;
        id[5] = 1.0;
        id[10] = 1.0;
        id[15] = 1.0;
        let s = vec![Sector::new("house".into(), None, id, vec![door])];
        let mut p = Passing::default();
        p.update(&s, [7.0, 1.0, 0.0]);
        p.update(&s, [3.0, 1.0, 0.0]);
        assert_eq!(p.sector, Some(0));
        // through the wall beside the door: still in
        p.update(&s, [7.0, 1.0, 4.0]);
        assert_eq!(p.sector, Some(0));
        p.update(&s, [3.0, 1.0, 0.0]);
        p.update(&s, [7.0, 1.0, 0.0]);
        assert_eq!(p.sector, None);
        // beside the door
        p.update(&s, [3.0, 1.0, 3.0]);
        assert_eq!(p.sector, None);
    }
}

