//! What to draw for a simulated object: the geometries of its template tree, each hung from the
//! part of the simulation that moves it.
//!
//! The walk is `vehicle::model`'s (the same children, a `LodObject`'s first only), so the
//! moving parts come in the order [`bytefield_sim::craft::Craft::new`] lists them: a component named as the next part
//! is that part, and what hangs below it is placed in its frame ([`bytefield_sim::craft::Craft::frames`]).

use std::collections::HashMap;

use bytefield_con::templates::TemplateDb;
use bytefield_sim::craft::Part;
use bytefield_world::parse_vec;
use bytefield_world::scene::Mat4;

/// One geometry of an object: its kind and file, the part it hangs from (None: the body) and
/// where it is in that part's frame; whether it is the soldier's parachute (seen only open);
/// an animated mesh's skin (`setSkin`) and whether it hangs from a soldier's skeleton (drawn
/// unskinned, it needs [`SKELETON_ROOT`]); whether it is a first-person part (the arms seen
/// from the soldier's own eyes, `setIsFirstPersonPart 1`).
#[derive(Debug, Clone)]
pub struct Draw {
    pub kind: String,
    pub file: String,
    pub part: Option<usize>,
    pub local: Mat4,
    pub parachute: bool,
    pub skin: Option<String>,
    pub soldier: bool,
    pub first_person: bool,
    /// the template it comes from and which of its kind in the walk (the n-th node so named):
    /// the n-th contactor of that name is it (`World::contactors` walks the same way)
    pub template: String,
    pub instance: usize,
    /// a track (an `AnimatedBundle` with a skeleton, not a soldier's): its `.ske` and, bone by
    /// bone, the wheel that moves it (`useAsBone`: template and instance)
    // TODO: `useAsBone`'s offset (0/-0.15/0 on the T34's wheels) and `setBoneOriginOffset` (0/-0.5/0 on 145 bundles) are not read: where the
    // track bends under each wheel. Compare a tank's lower run from the side with the original.
    pub track: Option<(String, Vec<(String, usize)>)>,
    /// `setAnimatedTextureSpeed` (u/v) when not 0/0: its texture runs with the engine (a track's tread)
    pub tex_speed: Option<[f32; 2]>,
    /// a `Spring` with a geometry: a wheel, which rolls (`play.rs`, `Spin`)
    pub wheel: bool,
    /// one side of a cockpit (a `LodObject` with a `DistCompareSelector`)
    pub cockpit: Option<Cockpit>,
    /// a side of a `CompareSelector`: its `addLodComparison`, and whether this is the child from there on (the blurred propeller)
    pub compare: Option<(f32, bool)>,
    /// a weapon's visible dummy projectile (`visibleDummyProjectileTemplate`: the rocket on the
    /// Wasserfall's ramp, DC's TOW and AIM-9), hung from its weapon's part
    pub dummy: bool,
    /// its `GeometryTemplate.hasDynamicShadow 1`: the original's silhouette pass draws its `.sm`'s shadow LOD
    pub shadow: bool,
    /// under the object's first `LodObject` with a destroyed LOD (`DistCompareSelector2`,
    /// `hasDestroyedLod 1`): whole only (false) or destroyed only (true); the armor's status tells
    /// that selector to use it when the object is destroyed, and it is the child after the last
    /// distance (`getLodLevelRelative`: `ShermanWreck`, the third of `lodSherman`)
    pub wreck: Option<bool>,
    /// its alpha follows the wreck's: the destroyed LOD's own
    /// geometry, or with no `LodObject` the object's own
    pub fade: bool,
    /// not a geometry but a `SeatObject`, of this `PlayerControlObject` ([`pco_id`]): [`seats`] takes them, [`draws`] leaves them
    pub seat: Option<u64>,
}

/// Where and how a seat shows its soldier: its place (like a [`Draw`]'s)
/// and the body's two animations, its template's or else the soldier's `Lb_SitInVehicle` or `Lb_StandInVehicle`
/// (`c_SeatShowStandingSoldier`) and `Ub_SitInVehicle`. Only the seats with a known flag: the rest hide it.
/// Level 2, against the original: no `SeatObject` hides him
/// (T34, the Hanomag's driver); `c_SeatShowStandingSoldier` alone shows him standing (`Stationary_mg42`, the Hanomag's gun); a flag name
/// the game does not know adds nothing, and `c_SeatIsOutside` alone (DC's `DesertPatrolVehicle` 1 and 2, misspelt `c_SeatShowSittingSoldier`)
/// shows his whole body sitting.
/// TODO: with a `SeatObject` and no flag at all (the snowspeeder's copilot, misspelt; `AW52`) he stays hidden by assumption: in the original neither
/// vehicle appeared (XPack2 did not load as a mod; GC's spawner gave nothing). Which client code hides him is not found.
/// The player's own head turns with the mouse (the `seat_head` of `Game`). TODO: up and down, other seats, other players and bots.
#[derive(Debug, Clone)]
pub struct SeatShow {
    pub owner: u64,
    /// which of the vehicle's `PlayerControlObject`s of that template, in the tree's order (a Fletcher's two deck guns)
    pub instance: usize,
    pub part: Option<usize>,
    pub local: Mat4,
    pub lower: String,
    pub upper: String,
    /// `c_SeatShowHalfBodySoldier`: `setUseSeat` disables the bone tree of `Bip01 Pelvis`, the legs
    pub half_body: bool,
}

/// A side of a cockpit: its first child (outside) or its second (inside: what the crew sees,
/// `1P_Sherman_Gunner_M1`), and where the `LodObject` is in the part's frame. Inside is drawn
/// when the player's own camera, in first person, passes the distance LOD selector's test:
/// d²·[`COCKPIT_K`] ≤ `addLodDistance`.
/// `addLodComparison` is against the compare value, which the client sets (not traced) for the crew.
/// `m_distanceLodFactor`·fov² at the default view. Level 2: not read from the original. Upper bound measured in the original: the AH64 (d²/limit 1.76)
/// sees its inside, the F-14B's pilot (2.39) too, and the AC-130's pilot (4.52:
/// `lodAC-130Cockpit` has no `setPosition`, its centre is the plane's, 9.5 m from the eye) too (the fuselage's
/// outside around him with 0.4), so k ≤ 0.221. From below: the Millennium Falcon's pilot (23.9) sees the hull from outside in the original,
/// so k > 1/23.9 ≈ 0.042; the MH-53 (4.24) looks the same with 0.5 and 0.2.
pub const COCKPIT_K: f32 = 0.2;

/// What a `CompareSelector`'s value is, from an engine's rpm. Level 2, from captures: empty and with the Mustang's idle (rpm 0) the
/// original keeps the static blades; the AH64 with a driver (our idle rpm 0.042, limit 0.08) draws the blurred ones: so the factor is over 1.93.
// TODO: read what calls `setLodSelectorCompareValue` in the client (a virtual call, no direct caller in the server), and bound this with a Mustang at low throttle.
pub const COMPARE_RPM: f32 = 2.0;

#[derive(Debug, Clone, Copy)]
pub struct Cockpit {
    pub inside: bool,
    pub distance: f32,
    pub center: Mat4,
    /// the `PlayerControlObject` it hangs from ([`pco_id`]): only that seat's camera sees it from inside. A seat with a
    /// PCO of its own (the Panzer IV's machine gun, by the turret) sees the turret from outside: the search for a
    /// `LodObject` stops at another PCO (`internalFindFirstChildOfCID`). Level 2: what sets the selector's compare value
    /// is in the client, not traced; the distance LOD selector wants it over `addLodDistance`.
    pub owner: u64,
}

/// A `PlayerControlObject`'s template, comparable (names are not case sensitive).
pub fn pco_id(name: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.to_ascii_lowercase().hash(&mut h);
    h.finish()
}

/// A soldier's animated meshes in their bind pose are z up and face +y (3ds max's frame): up to
/// y, y forward (+z), and its feet `setCharacterHeight` (−1 m) under the soldier's centre.
pub const SKELETON_ROOT: Mat4 = Mat4([[-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, -1.0, 0.0, 1.0]]);

/// Children of a template with their placement, whether each is a first-person part
/// (`setIsFirstPersonPart 1`: the soldier's arms seen from its own camera) and whether it
/// moves a bone of its parent's skeleton (`useAsBone`: a track's wheel). `random`: the first of
/// a `setRandomGeometries` child's variants (`scene::random_child`; not the soldiers' detailed
/// head, which needs more).
fn children(t: &bytefield_con::templates::Template, random: bool) -> Vec<(String, Mat4, bool, bool)> {
    let mut out: Vec<(String, [f64; 3], [f64; 3], bool, bool)> = Vec::new();
    for p in &t.props {
        let v = || p.args.first().and_then(|a| parse_vec(a)).filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]);
        match p.name.as_str() {
            "addtemplate" => {
                if let Some(n) = p.args.first() {
                    out.push((n.clone(), [0.0; 3], [0.0; 3], false, false));
                }
            }
            "setposition" | "position" => {
                if let (Some(last), Some(v)) = (out.last_mut(), v()) {
                    last.1 = v;
                }
            }
            "setrotation" | "rotation" => {
                if let (Some(last), Some(v)) = (out.last_mut(), v()) {
                    last.2 = v;
                }
            }
            "setisfirstpersonpart" => {
                if let Some(last) = out.last_mut() {
                    last.3 = p.args.first().is_some_and(|a| a.trim() == "1");
                }
            }
            "setrandomgeometries" if random => {
                if let Some(last) = out.last_mut() {
                    last.0 = bytefield_world::scene::random_child(&last.0, &p.args);
                }
            }
            "useasbone" => {
                if let Some(last) = out.last_mut() {
                    last.4 = true;
                }
            }
            _ => {}
        }
    }
    out.into_iter().map(|(n, p, r, fp, bone)| (n, Mat4::from_pos_ypr(p, r), fp, bone)).collect()
}

thread_local! {
    /// Which of a `setRandomGeometries n` this vehicle wears, see [`draws_with_cockpits`]
    static BODY: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `setRandomGeometries n`: the sim always takes the variant 1 (`scene::random_child`: they differ only in the textures), and here
/// the draw wears the one [`BODY`] says. The original's is the bundle template's counter, 1..n round every bundle that has
/// one, in the order they are created.
fn random_body(db: &TemplateDb, parent: &bytefield_con::templates::Template, child: &str, draws: &mut [Draw]) {
    let body = BODY.with(|b| b.get());
    if body == 0 {
        return;
    }
    let mut last = None;
    for p in &parent.props {
        match p.name.as_str() {
            "addtemplate" => last = p.args.first(),
            "setrandomgeometries" => {
                let Some(base) = last.filter(|b| format!("{b}1").eq_ignore_ascii_case(child)) else { continue };
                let Some(n) = p.args.first().and_then(|n| n.trim().parse::<usize>().ok()).filter(|n| *n >= 1) else { continue };
                let file = db.object_template(&format!("{base}{}", (body - 1) % n + 1)).and_then(|t| db.get("geometrytemplate", t.get("geometry")?.first()?)?.get("file")?.first().cloned());
                if let (Some(file), Some(d)) = (file, draws.iter_mut().find(|d| d.template.eq_ignore_ascii_case(child))) {
                    d.file = file.replace('\\', "/");
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn walk(db: &TemplateDb, name: &str, anchor: Option<usize>, local: Mat4, root: bool, parts: &[Part], next: &mut usize, out: &mut Vec<Draw>, parachute: bool, soldier: bool, first_person: bool, seen: &mut HashMap<String, usize>, depth: usize, cockpit: Option<Cockpit>, owner: u64, inside: bool, lod_found: &mut bool) {
    if depth > 16 {
        return;
    }
    let Some(t) = db.object_template(name) else { return };
    let owner = if t.kind.eq_ignore_ascii_case("playercontrolobject") { pco_id(&t.name) } else { owner };
    // `internalFindFirstChildOfCID(LodObject, PlayerControlObject)`: no LOD under another PCO
    let mut beyond = true;
    let lod_found = if !root && t.kind.eq_ignore_ascii_case("playercontrolobject") { &mut beyond } else { lod_found };
    let count = seen.entry(t.name.to_ascii_lowercase()).or_insert(0);
    let instance = *count;
    *count += 1;
    let lod = t.kind.eq_ignore_ascii_case("lodobject");
    let mut kids = children(t, !soldier);
    // the first `LodObject`'s destroyed child, if its selector has one
    let first_lod = lod && !std::mem::replace(lod_found, *lod_found || lod);
    let wreck = first_lod
        .then(|| t.get("lodselector").and_then(|a| a.first()))
        .flatten()
        .and_then(|sel| db.get("lodselectortemplate", sel))
        .filter(|s| s.kind.eq_ignore_ascii_case("distcompareselector2") && s.get("hasdestroyedlod").and_then(|a| a.first()).is_some_and(|v| v.trim() != "0"))
        .and_then(|s| kids.get(s.all("addloddistance").len() + 1).cloned());
    // a cockpit's inside: walked apart (it is no part of the craft, and counts no instance)
    let distance = lod
        .then(|| t.get("lodselector").and_then(|a| a.first()))
        .flatten()
        .and_then(|sel| db.get("lodselectortemplate", sel))
        .filter(|s| s.kind.eq_ignore_ascii_case("distcompareselector"))
        .and_then(|s| {
            // `addLodDistance` is the limit on d²·factor·fov²; none (the M2A3's passengers), no limit (`getLodLevelRelative`)
            Some(s.get("addloddistance").and_then(|a| a.first()?.trim().parse::<f32>().ok()).filter(|d| *d > 0.0).unwrap_or(f32::INFINITY))
        });
    // an inside whose mesh has no file (DC's M1A1: `rem GeometryTemplate.file`) still is the LOD chosen: nothing of it is drawn, and
    // the outside goes
    let inner = match distance {
        Some(d) if inside && kids.len() > 1 => Some((kids.swap_remove(1), Cockpit { inside: true, distance: d, center: local, owner })),
        _ => None,
    };
    // a `CompareSelector` (propeller, rotor, afterburner): child 0 under its `addLodComparison`, child 1 from there on. The second
    // is walked apart, like a cockpit's inside. Level 2: what sets the compare value is in the client, not traced; rpm fits
    let compare = lod
        .then(|| t.get("lodselector").and_then(|a| a.first()))
        .flatten()
        .and_then(|sel| db.get("lodselectortemplate", sel))
        .filter(|s| s.kind.eq_ignore_ascii_case("compareselector"))
        .and_then(|s| s.get("addlodcomparison").and_then(|a| a.first()?.trim().parse::<f32>().ok()));
    let second = compare.filter(|_| inside && kids.len() > 1).map(|at| (kids.swap_remove(1), at));
    kids.truncate(if lod { 1 } else { usize::MAX });
    let cockpit = distance.map(|d| Cockpit { inside: false, distance: d, center: local, owner }).or(cockpit);
    let parachute = parachute || t.name.eq_ignore_ascii_case("parachute");
    let (anchor, local) = if !root && parts.get(*next).is_some_and(|p| p.name.eq_ignore_ascii_case(&t.name)) {
        *next += 1;
        (Some(*next - 1), Mat4::IDENTITY)
    } else {
        (anchor, local)
    };
    // `createInvisible 1`: created (physics, collision) but not drawn; DC's deployables hold themselves up on four of them with
    // the UH-60's rear wheel as geometry, which the original never shows. Level 2: only
    // the node's own geometry, its children (none in the data's springs) still drawn
    let invisible = t.get("createinvisible").and_then(|a| a.first()).is_some_and(|v| v.trim() == "1");
    if !invisible
        && let Some(g) = t.get("geometry").and_then(|a| a.first())
        && let Some(gt) = db.get("geometrytemplate", g)
        && let Some(file) = gt.get("file").and_then(|a| a.first())
    {
        let kind = gt.kind.to_ascii_lowercase();
        // `GeometryTemplate.scale`: none in vanilla; DC's T72, M1A1, M2A3, BMP, Pickup…
        let local = match gt.get("scale").and_then(|a| a.first()?.trim().parse::<f64>().ok()).filter(|s| *s > 0.0 && *s != 1.0) {
            Some(k) => local.mul(&Mat4([[k, 0.0, 0.0, 0.0], [0.0, k, 0.0, 0.0], [0.0, 0.0, k, 0.0], [0.0, 0.0, 0.0, 1.0]])),
            None => local,
        };
        let skin = gt.get("setskin").and_then(|a| a.first()).map(|f| f.replace('\\', "/"));
        // (the parachute hangs from the soldier, but on a skeleton of its own: `Parachute.skn` does not fit his)
        let soldier = soldier && kind == "animatedmesh" && !parachute;
        // a track's bones: its useAsBone children, each the instance its walk will give it
        let track = (!soldier && kind == "animatedmesh" && t.kind.eq_ignore_ascii_case("animatedbundle"))
            .then(|| t.get("createskeleton").and_then(|a| a.first()).map(|f| f.replace('\\', "/")))
            .flatten()
            .map(|ske| {
                let mut ahead: HashMap<String, usize> = HashMap::new();
                let mut bones = Vec::new();
                for (c, _, _, bone) in &kids {
                    let key = c.to_ascii_lowercase();
                    let k = seen.get(&key).copied().unwrap_or(0) + *ahead.get(&key).unwrap_or(&0);
                    *ahead.entry(key).or_insert(0) += 1;
                    if *bone {
                        bones.push((c.clone(), k));
                    }
                }
                (ske, bones)
            });
        let tex_speed = t
            .get("setanimatedtexturespeed")
            .and_then(|a| parse_vec(a.first()?))
            .filter(|v| v.len() == 2 && (v[0] != 0.0 || v[1] != 0.0))
            .map(|v| [v[0] as f32, v[1] as f32]);
        let shadow = gt.get("hasdynamicshadow").and_then(|a| a.first()).is_some_and(|v| v.trim() == "1");
        out.push(Draw { kind, file: file.replace('\\', "/"), part: anchor, local, parachute, skin, soldier, first_person, template: t.name.clone(), instance, track, tex_speed, wheel: t.kind.eq_ignore_ascii_case("spring"), cockpit, compare: None, dummy: false, shadow, wreck: None, fade: false, seat: None });
    }
    // (a `PlayerControlObject` too, before its children: [`seats`] counts them)
    if t.kind.eq_ignore_ascii_case("seatobject") || t.kind.eq_ignore_ascii_case("playercontrolobject") {
        out.push(Draw { kind: t.kind.clone(), file: String::new(), part: anchor, local, parachute, skin: None, soldier: false, first_person, template: t.name.clone(), instance, track: None, tex_speed: None, wheel: false, cockpit, compare: None, dummy: false, shadow: false, wreck: None, fade: false, seat: Some(owner) });
    }
    // The weapon's dummy projectiles: one at each `addFireArmsPosition` (none: at the
    // weapon), walked apart (no part of the craft, no instance counted). Only the position: an identity matrix
    // with its translation, the position's turn is not used (the F-14B's MK83 at 0/35/0
    // hang level in the original)
    if t.kind.eq_ignore_ascii_case("firearms")
        && let Some(d) = t.get("visibledummyprojectiletemplate").or_else(|| t.get("setvisibledummyprojectiletemplate")).and_then(|a| a.first())
    {
        let vec = |a: Option<&String>| a.and_then(|a| parse_vec(a)).filter(|v| v.len() == 3).map_or([0.0; 3], |v| [v[0], v[1], v[2]]);
        let mut at: Vec<Mat4> = t.all("addfirearmsposition").into_iter().map(|a| Mat4::from_pos_ypr(vec(a.first()), [0.0; 3])).collect();
        if at.is_empty() {
            at.push(Mat4::IDENTITY);
        }
        for m in at {
            let mut dummies = Vec::new();
            walk(db, d, anchor, local.mul(&m), false, &[], &mut 0, &mut dummies, parachute, soldier, first_person, &mut seen.clone(), depth + 1, cockpit, owner, inside, &mut true);
            out.extend(dummies.into_iter().map(|x| Draw { dummy: true, ..x }));
        }
    }
    let whole = out.len();
    for (child, m, fp, _) in kids {
        let from = out.len();
        walk(db, &child, anchor, local.mul(&m), false, parts, next, out, parachute, soldier, first_person || fp, seen, depth + 1, cockpit, owner, inside, lod_found);
        random_body(db, t, &child, &mut out[from..]);
    }
    if let Some(((child, m, fp, _), c)) = inner {
        walk(db, &child, anchor, local.mul(&m), false, &[], &mut 0, out, parachute, soldier, first_person || fp, &mut seen.clone(), depth + 1, Some(c), owner, inside, &mut true);
    }
    if let Some(((child, m, fp, _), at)) = second {
        for d in &mut out[whole..] {
            d.compare = Some((at, false));
        }
        let mut high = Vec::new();
        walk(db, &child, anchor, local.mul(&m), false, &[], &mut 0, &mut high, parachute, soldier, first_person || fp, &mut seen.clone(), depth + 1, cockpit, owner, inside, &mut true);
        out.extend(high.into_iter().map(|d| Draw { compare: Some((at, true)), ..d }));
    }
    // the destroyed child: walked apart, like a cockpit's inside (no part of the craft)
    if let Some((child, m, fp, _)) = wreck {
        for d in &mut out[whole..] {
            d.wreck = Some(false);
        }
        let mut destroyed = Vec::new();
        walk(db, &child, anchor, local.mul(&m), false, &[], &mut 0, &mut destroyed, parachute, soldier, first_person || fp, &mut seen.clone(), depth + 1, cockpit, owner, inside, &mut true);
        out.extend(destroyed.into_iter().map(|d| Draw { wreck: Some(true), fade: d.template.eq_ignore_ascii_case(&child), ..d }));
    }
}

/// The geometries of an object template, hung from `parts` (its craft's).
pub fn draws(db: &TemplateDb, template: &str, parts: &[Part]) -> Vec<Draw> {
    let mut out = walk_all(db, template, parts, false);
    out.retain(|d| d.seat.is_none());
    out
}

/// [`draws`] and the insides of its cockpits (a vehicle the player can board).
pub fn draws_with_cockpits(db: &TemplateDb, template: &str, parts: &[Part], body: usize) -> Vec<Draw> {
    BODY.with(|b| b.set(body));
    let mut out = walk_all(db, template, parts, true);
    BODY.with(|b| b.set(0));
    out.retain(|d| d.seat.is_none());
    out
}

/// The seats of an object that show their soldier: of each `PlayerControlObject`, its first `SeatObject` (a flag name the
/// game does not know, the data's `c_SeatHalfBodySoldier` or `c_SeatShowSittingSoldier`, adds nothing; see [`SeatShow`]).
pub fn seats(db: &TemplateDb, template: &str, parts: &[Part]) -> Vec<SeatShow> {
    // of each template of `PlayerControlObject`: the one being walked, the seat objects seen in it, and which of them
    // (depth first) seats its player (`seat_rank`: not always the first, the SA-9's gunner)
    let mut open: HashMap<u64, (usize, usize, usize)> = HashMap::new();
    let rank = |pco: &str| bytefield_sim::vehicle::model(db, pco).and_then(|c| c.seat_rank()).unwrap_or(0);
    let mut out = Vec::new();
    for d in walk_all(db, template, parts, false) {
        let Some(owner) = d.seat else { continue };
        if d.kind.eq_ignore_ascii_case("playercontrolobject") {
            open.entry(owner).and_modify(|o| *o = (o.0 + 1, 0, o.2)).or_insert_with(|| (0, 0, rank(&d.template)));
            continue;
        }
        let (instance, seen, want) = open.entry(owner).or_insert((0, 0, 0));
        *seen += 1;
        if *seen != *want + 1 {
            continue;
        }
        let instance = *instance;
        let Some(t) = db.object_template(&d.template) else { continue };
        let flag = |name: &str| t.all("seatflags").into_iter().any(|a| a.first().is_some_and(|f| f.trim().eq_ignore_ascii_case(name)));
        let standing = flag("c_SeatShowStandingSoldier");
        let half_body = flag("c_SeatShowHalfBodySoldier");
        if !(standing || flag("c_SeatShowFullBodySoldier") || half_body || flag("c_SeatShowHeadOfSoldier") || flag("c_SeatIsOutside")) {
            continue;
        }
        let anim = |key: &str, default: &str| t.get(key).and_then(|a| a.first()).map_or_else(|| default.to_string(), |s| s.trim().to_string());
        out.push(SeatShow { owner, instance, part: d.part, local: d.local, lower: anim("seatanimationlowerbody", if standing { "Lb_StandInVehicle" } else { "Lb_SitInVehicle" }), upper: anim("seatanimationupperbody", "Ub_SitInVehicle"), half_body });
    }
    out
}

/// Of [`seats`], the one of the vehicle's seat `position` (none: it hides its soldier). Two seats of one template of
/// `PlayerControlObject` are told apart by their order.
pub fn seat_at<'a>(shows: &'a [SeatShow], seats: &[bytefield_sim::entry::Seat], position: usize) -> Option<&'a SeatShow> {
    let pco = &seats.get(position)?.pco;
    let instance = seats[..position].iter().filter(|s| s.pco.eq_ignore_ascii_case(pco)).count();
    let owner = pco_id(pco);
    shows.iter().find(|s| s.owner == owner && s.instance == instance)
}

fn walk_all(db: &TemplateDb, template: &str, parts: &[Part], inside: bool) -> Vec<Draw> {
    let mut out = Vec::new();
    let mut next = 0;
    let soldier = db.object_template(template).is_some_and(|t| t.kind.eq_ignore_ascii_case("bfsoldier"));
    let mut lod_found = false;
    walk(db, template, None, Mat4::IDENTITY, true, parts, &mut next, &mut out, false, soldier, false, &mut HashMap::new(), 0, None, 0, inside, &mut lod_found);
    if !lod_found {
        for d in out.iter_mut().filter(|d| d.part.is_none() && d.instance == 0 && d.template.eq_ignore_ascii_case(template)) {
            d.fade = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    /// The Sherman's `lodSherman` (`DistCompareSelector2`, `hasDestroyedLod 1`, one distance):
    /// `ShermanComplex` while whole, `ShermanWreck` (the third child) once destroyed, which
    /// fades; the cockpit's LOD further down is no destroyed one.
    #[test]
    fn a_destroyed_tank_shows_its_wreck() {
        let game = bytefield_testdata::require!(Base);
        let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let v = w.vehicle("Sherman").unwrap();
        let d = super::draws_with_cockpits(&w.db, "Sherman", &v.craft.parts, 0);
        let wreck: Vec<_> = d.iter().filter(|d| d.wreck == Some(true)).collect();
        assert!(!wreck.is_empty() && wreck.iter().all(|d| d.template.eq_ignore_ascii_case("ShermanWreck") && d.fade), "{wreck:?}");
        assert!(d.iter().filter(|d| d.wreck == Some(false)).count() > 3);
        assert!(d.iter().all(|d| d.wreck.is_some() || d.fade == false));
        assert!(!d.iter().any(|d| d.fade && d.wreck != Some(true)));
    }

    /// A seat shows its soldier by its flags and in its animations, its own or the soldier's: the jeep's two, the Panzer IV's
    /// machine gunner and not its closed crew.
    #[test]
    fn the_seats_that_show_their_soldier() {
        let game = bytefield_testdata::require!(Base);
        let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let v = w.vehicle("Kubelwagen").unwrap();
        let k = super::seats(&w.db, "Kubelwagen", &v.craft.parts);
        assert_eq!(k.len(), 2, "{k:?}");
        let driver = k.iter().find(|s| s.owner == super::pco_id("Kubelwagen")).expect("its driver");
        assert_eq!((driver.lower.as_str(), driver.upper.as_str()), ("Lb_SitInVehicle", "Ub_SitInVehicle"));
        let passenger = k.iter().find(|s| s.owner == super::pco_id("KubelwagenPassangerSeat")).expect("its passenger");
        assert_eq!(passenger.lower, "Lb_PassengerInKubelwagen");
        let v = w.vehicle("PanzerIV").unwrap();
        let p = super::seats(&w.db, "PanzerIV", &v.craft.parts);
        assert!(p.iter().any(|s| s.owner == super::pco_id("PanzerIV_Browning_PCO1")), "{p:?}");
        assert!(!p.iter().any(|s| s.owner == super::pco_id("PanzerIV")), "{p:?}");
        // and they are no geometries
        assert!(super::draws_with_cockpits(&w.db, "Kubelwagen", &[], 0).iter().all(|d| d.seat.is_none()));
    }

    /// A Fletcher's two deck guns share a template of `PlayerControlObject`, each gunner at his own; a fighter's pilot, half
    /// a body, has his legs at his pelvis.
    #[test]
    fn a_seat_shows_him_with_any_known_flag() {
        // as the original: the misspelt flag adds nothing and `c_SeatIsOutside` alone shows him, sitting
        let game = bytefield_testdata::require!(Base, Dc);
        let w = bytefield_sim::World::load(&game, "DC_Final", "Aberdeen", "conquest").unwrap();
        let v = w.vehicle("DesertPatrolVehicle").unwrap();
        let shows = super::seats(&w.db, "DesertPatrolVehicle", &v.craft.parts);
        for k in 0..3 {
            let s = super::seat_at(&shows, &v.entry.seats, k).unwrap();
            assert_eq!(s.lower, "Lb_SitInVehicle");
            assert!(!s.half_body);
        }
        let w = bytefield_sim::World::load(&game, "bf1942", "Kursk", "conquest").unwrap();
        let v = w.vehicle("T34").unwrap();
        let shows = super::seats(&w.db, "T34", &v.craft.parts);
        assert!(super::seat_at(&shows, &v.entry.seats, 0).is_none());
    }

    #[test]
    fn two_seats_of_one_template_and_half_a_body() {
        let game = bytefield_testdata::require!(Base);
        let w = bytefield_sim::World::load(&game, "bf1942", "Midway", "conquest").unwrap();
        let v = w.vehicle("Fletcher").unwrap();
        let shows = super::seats(&w.db, "Fletcher", &v.craft.parts);
        let found: Vec<usize> = v.entry.seats.iter().enumerate().filter(|(_, s)| s.pco.eq_ignore_ascii_case("FletcherDeckBrowningPCO")).map(|(k, _)| k).collect();
        assert_eq!(found.len(), 2);
        let (a, b) = (super::seat_at(&shows, &v.entry.seats, found[0]).unwrap(), super::seat_at(&shows, &v.entry.seats, found[1]).unwrap());
        assert_eq!((a.instance, b.instance), (0, 1));
        assert_ne!(v.entry.seats[found[0]].ride, v.entry.seats[found[1]].ride);
        let s = w.soldier("USSoldier").unwrap();
        let m = s.movement.as_ref().unwrap();
        let sk = m.posed_skeleton_seated("Lb_SitInVehicle", "Ub_SitInVehicle", true, 0.0).unwrap();
        let bone = |n: &str| sk.bone_pos(sk.file.index(n).unwrap());
        assert_eq!(bone("Bip01 L Foot"), bone("Bip01 Pelvis"));
        assert_eq!(bone("Bip01 R Thigh"), bone("Bip01 Pelvis"));
        assert_ne!(bone("Bip01 Head"), bone("Bip01 Pelvis"));
        let whole = m.posed_skeleton_seated("Lb_SitInVehicle", "Ub_SitInVehicle", false, 0.0).unwrap();
        assert_ne!(whole.bone_pos(whole.file.index("Bip01 L Foot").unwrap()), whole.bone_pos(whole.file.index("Bip01 Pelvis").unwrap()));
        // (turning with the mouse: the head goes, the hands stay on the wheel)
        let turned = m.posed_skeleton_seated("Lb_SitInVehicle", "Ub_SitInVehicle", false, 18.0).unwrap();
        let of = |sk: &bytefield_sim::skeleton::Skeleton, n: &str| sk.bone_pos(sk.file.index(n).unwrap());
        assert_eq!(of(&turned, "Bip01 R Hand"), of(&whole, "Bip01 R Hand"));
        assert_eq!(of(&turned, "Bip01 L Foot"), of(&whole, "Bip01 L Foot"));
        let at = |sk: &bytefield_sim::skeleton::Skeleton| sk.abs[sk.file.index("Bip01 Head").unwrap()];
        assert_ne!(at(&turned), at(&whole));
    }

    /// Every seat of every vehicle of a mod's levels, against what the client will do with it: `cargo test --release -p bytefield-client
    /// seats_audit -- --ignored --nocapture`, with `BYTEFIELD_SEATS_MODS=bf1942,DC_Final,GCMOD` (the default: all of them). Writes
    /// `data/out/seats-audit/<mod>.tsv` and prints the problems. Checks the seat found by [`super::seats`] against an independent
    /// reading (the simulation's tree: its `SeatObject`, flags and `ride`), that its two animations exist with bones, and where the
    /// posed body ends up (head over pelvis, within the meshes' box).
    #[test]
    #[ignore]
    fn seats_audit() {
        use std::collections::{BTreeMap, BTreeSet};
        use std::fmt::Write as _;
        let game = bytefield_testdata::game();
        let mods = std::env::var("BYTEFIELD_SEATS_MODS").unwrap_or_else(|_| "bf1942,XPack1,XPack2,DesertCombat,DC_Final,DC_Extended,GCMOD,GC_Redux".into());
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/out/seats-audit");
        std::fs::create_dir_all(&out).unwrap();
        fn seat_objects<'a>(c: &'a bytefield_sim::vehicle::Component, out: &mut Vec<&'a bytefield_sim::vehicle::Component>) {
            for k in &c.children {
                if k.kind.eq_ignore_ascii_case("PlayerControlObject") {
                    continue;
                }
                if k.kind.eq_ignore_ascii_case("SeatObject") {
                    out.push(k);
                }
                seat_objects(k, out);
            }
        }
        fn pcos<'a>(c: &'a bytefield_sim::vehicle::Component, out: &mut BTreeMap<String, &'a bytefield_sim::vehicle::Component>) {
            for k in &c.children {
                if k.kind.eq_ignore_ascii_case("PlayerControlObject") {
                    out.entry(k.name.to_ascii_lowercase()).or_insert(k);
                }
                pcos(k, out);
            }
        }
        const KNOWN: [(&str, u32); 5] = [("c_seatshowhalfbodysoldier", 1), ("c_seatshowfullbodysoldier", 2), ("c_seatshowheadofsoldier", 4), ("c_seatisoutside", 8), ("c_seatshowstandingsoldier", 16)];
        let mut failed_checks = Vec::new();
        for m in mods.split(',') {
            let Ok((_, vfs)) = bytefield_con::session::mount(&game, m) else {
                println!("{m}: does not mount");
                continue;
            };
            let levels = bytefield_con::session::levels(&vfs);
            let mut done: BTreeSet<String> = BTreeSet::new();
            let mut tsv = String::from("level\tvehicle\tseat\tpco\tseat_object\tflags\tcode\tshown\tlower\tupper\tride_error\tpelvis_y\thead_y\tfeet_y\thead_up\tbox_excess\tproblems\n");
            let mut problems: BTreeMap<String, Vec<String>> = BTreeMap::new();
            let mut unknown: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            let (mut loaded, mut failed, mut seats_total, mut shown_total) = (0, 0, 0, 0);
            for (level, modes) in &levels {
                let mode = modes.first().copied().unwrap_or("conquest");
                let Ok(w) = bytefield_sim::World::load(&game, m, level, mode) else {
                    failed += 1;
                    continue;
                };
                loaded += 1;
                let soldiers = w.db.objects_of_kind("bfsoldier");
                let Some(sold) = soldiers.first().and_then(|t| w.soldier(&t.name)) else {
                    problems.entry("level without a soldier".into()).or_default().push(level.clone());
                    continue;
                };
                let move_ = sold.movement.as_ref().unwrap();
                let skeletons: BTreeSet<String> = soldiers.iter().filter_map(|t| t.get("createskeleton")?.first().map(|f| f.to_ascii_lowercase())).collect();
                if skeletons.len() > 1 {
                    problems.entry("several_skeletons".into()).or_default().push(format!("{level} {skeletons:?}"));
                }
                let roots: Vec<String> = w.db.objects_of_kind("playercontrolobject").iter().map(|t| t.name.clone()).collect();
                let mut children: BTreeSet<String> = BTreeSet::new();
                for r in &roots {
                    if let Some(c) = bytefield_sim::vehicle::model(&w.db, r) {
                        let mut p = BTreeMap::new();
                        pcos(&c, &mut p);
                        children.extend(p.into_keys());
                    }
                }
                for r in roots.iter().filter(|r| !children.contains(&r.to_ascii_lowercase())) {
                    if !done.insert(r.to_ascii_lowercase()) {
                        continue;
                    }
                    let Some(v) = w.vehicle(r) else {
                        problems.entry("does not build".into()).or_default().push(r.clone());
                        continue;
                    };
                    let Some(tree) = bytefield_sim::vehicle::model(&w.db, r) else { continue };
                    let mut tree_pcos = BTreeMap::new();
                    pcos(&tree, &mut tree_pcos);
                    tree_pcos.insert(tree.name.to_ascii_lowercase(), &tree);
                    let client = super::seats(&w.db, r, &v.craft.parts);
                    // the meshes' box, in the vehicle's frame
                    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                    let frames = v.craft.frames();
                    let frame_of = |part: Option<usize>| match part {
                        Some(k) => crate::assets::frame_matrix(&frames[k].child_rows, frames[k].pos),
                        None => crate::assets::frame_matrix(&v.craft.body.rows, v.craft.body.pos),
                    };
                    for d in super::draws(&w.db, r, &v.craft.parts) {
                        if d.kind != "standardmesh" {
                            continue;
                        }
                        let Ok(bytes) = w.vfs.read(&format!("standardmesh/{}.sm", d.file)).or_else(|_| w.vfs.read(&format!("{}.sm", d.file))) else { continue };
                        let Ok(sm) = bytefield_formats::sm::parse(&bytes) else { continue };
                        let f = frame_of(d.part).mul(&d.local);
                        for x in [sm.bounds[0][0], sm.bounds[1][0]] {
                            for y in [sm.bounds[0][1], sm.bounds[1][1]] {
                                for z in [sm.bounds[0][2], sm.bounds[1][2]] {
                                    let q = f.transform_point([x as f64, y as f64, z as f64]);
                                    for k in 0..3 {
                                        lo[k] = lo[k].min(q[k]);
                                        hi[k] = hi[k].max(q[k]);
                                    }
                                }
                            }
                        }
                    }
                    let origin = v.craft.body.pos;
                    let mut names: BTreeMap<String, usize> = BTreeMap::new();
                    for s in &v.entry.seats {
                        *names.entry(s.pco.to_ascii_lowercase()).or_insert(0) += 1;
                    }
                    for (k, s) in v.entry.seats.iter().enumerate() {
                        seats_total += 1;
                        let mut bad: Vec<String> = Vec::new();
                        let key = s.pco.to_ascii_lowercase();
                        let mut so = Vec::new();
                        let mut want = 0;
                        if let Some(c) = tree_pcos.get(&key) {
                            seat_objects(c, &mut so);
                            want = c.seat_rank().unwrap_or(0);
                        } else {
                            bad.push("pco_outside_the_tree".into());
                        }
                        if so.len() > 1 {
                            bad.push(format!("several_seatobject({})", so.len()));
                        }
                        if names[&key] > 1 {
                            bad.push(format!("pco_repeated({})", names[&key]));
                        }
                        let first = so.get(want);
                        let mut code = 0;
                        let mut flags = Vec::new();
                        if let Some(t) = first.and_then(|c| w.db.object_template(&c.name)) {
                            for a in t.all("seatflags") {
                                let f = a.first().map_or("", |f| f.trim());
                                flags.push(f.to_string());
                                match KNOWN.iter().find(|(n, _)| f.eq_ignore_ascii_case(n)) {
                                    Some((_, b)) => code |= b,
                                    None => {
                                        unknown.entry(f.to_string()).or_default().insert(r.clone());
                                    }
                                }
                            }
                        }
                        let expect = code != 0;
                        let found = super::seat_at(&client, &v.entry.seats, k);
                        if expect != found.is_some() {
                            bad.push(if expect { "should_show_and_does_not".into() } else { "shows_without_flag".to_string() });
                        }
                        let (mut ride_err, mut pelvis, mut head, mut feet, mut head_up, mut excess) = (f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN);
                        let (mut lower, mut upper) = (String::new(), String::new());
                        if let Some(c) = found {
                            shown_total += 1;
                            lower.clone_from(&c.lower);
                            upper.clone_from(&c.upper);
                            let at = frame_of(c.part).mul(&c.local);
                            let p = at.transform_point([0.0; 3]);
                            let world_ride = [origin[0] as f64 + s.ride[0] as f64, origin[1] as f64 + s.ride[1] as f64, origin[2] as f64 + s.ride[2] as f64];
                            ride_err = (0..3).map(|i| (p[i] - world_ride[i]).powi(2)).sum::<f64>().sqrt();
                            if ride_err > 0.05 {
                                bad.push("place_differs_from_ride".into());
                            }
                            for (what, name) in [("lower", &c.lower), ("upper", &c.upper)] {
                                match move_.machine.find(name) {
                                    None => bad.push(format!("missing_state_{what}:{name}")),
                                    Some(i) if move_.machine.animation(i).is_none_or(|a| a.bones.is_empty()) => bad.push(format!("state_without_animation_{what}:{name}")),
                                    _ => {}
                                }
                            }
                            if let Some(sk) = move_.posed_skeleton_seated(&c.lower, &c.upper, c.half_body, 0.0) {
                                if !sk.abs.iter().all(|m| m.iter().all(|x| x.is_finite())) {
                                    bad.push("pose_not_finite".into());
                                }
                                let bone = |n: &str| sk.file.index(n).map(|b| sk.bone_pos(b)).map(|q| [q[0] as f64, q[1] as f64, q[2] as f64]);
                                if let (Some(pe), Some(he), Some(fl), Some(fr)) = (bone("Bip01 Pelvis"), bone("Bip01 Head"), bone("Bip01 L Foot"), bone("Bip01 R Foot")) {
                                    let (wp, wh) = (at.transform_point(pe), at.transform_point(he));
                                    pelvis = wp[1] - origin[1] as f64;
                                    head = wh[1] - origin[1] as f64;
                                    feet = at.transform_point(fl)[1].min(at.transform_point(fr)[1]) - origin[1] as f64;
                                    if c.half_body != (fl == pe && fr == pe) {
                                        bad.push("legs_wrong".into());
                                    }
                                    // (in the seat's frame: the data lay some down, a medical bed's)
                                    head_up = he[1] - pe[1];
                                    if head_up < 0.2 {
                                        bad.push("head_not_over_pelvis".into());
                                    }
                                    if lo[0] <= hi[0] {
                                        excess = (0..3).map(|i| (lo[i] - wp[i]).max(wp[i] - hi[i]).max(0.0)).fold(0.0, f64::max);
                                        if excess > 0.5 {
                                            bad.push("pelvis_outside_the_box".into());
                                        }
                                    }
                                } else {
                                    bad.push("without_reference_bones".into());
                                }
                            } else {
                                bad.push("without_skeleton".into());
                            }
                        }
                        for b in &bad {
                            problems.entry(b.split(':').next().unwrap().split('(').next().unwrap().to_string()).or_default().push(format!("{r}[{k}] {}{}", s.pco, b.split_once(':').map_or(String::new(), |(_, n)| format!(" {n}"))));
                        }
                        writeln!(tsv, "{level}\t{r}\t{k}\t{}\t{}\t{}\t{code}\t{}\t{lower}\t{upper}\t{ride_err:.3}\t{pelvis:.2}\t{head:.2}\t{feet:.2}\t{head_up:.2}\t{excess:.2}\t{}", s.pco, first.map_or("-", |c| c.name.as_str()), flags.join("+"), found.is_some(), bad.join(",")).unwrap();
                    }
                }
            }
            std::fs::write(out.join(format!("{m}.tsv")), &tsv).unwrap();
            println!("\n=== {m}: {loaded} levels ({failed} not loaded), {} vehicles, {seats_total} seats, {shown_total} show the soldier", done.len());
            for (f, who) in &unknown {
                println!("  unknown flag {f}: {}", who.iter().cloned().collect::<Vec<_>>().join(" "));
            }
            for (p, who) in &problems {
                println!("  {p} ×{}: {}", who.len(), who.iter().take(12).cloned().collect::<Vec<_>>().join(" | "));
            }
            // what can only be our fault (the rest are notices about the data: a box, a state it does not have)
            const FAILS: [&str; 8] = ["pco_outside_the_tree", "should_show_and_does_not", "shows_without_flag", "place_differs_from_ride", "pose_not_finite", "head_not_over_pelvis", "legs_wrong", "without_skeleton"];
            failed_checks.extend(problems.iter().filter(|(p, _)| FAILS.contains(&p.as_str())).map(|(p, who)| format!("{m}: {p} ×{}", who.len())));
        }
        assert!(failed_checks.is_empty(), "{failed_checks:?}");
    }

    /// The Panzer IV's turret has a gunner's inside, for its own PCO and not for the machine gunner's beside it.
    #[test]
    fn a_cockpit_belongs_to_its_seat() {
        let game = bytefield_testdata::require!(Base);
        let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let v = w.vehicle("PanzerIV").unwrap();
        let d = super::draws_with_cockpits(&w.db, "PanzerIV", &v.craft.parts, 0);
        let inner = d.iter().find(|d| d.template.eq_ignore_ascii_case("PanzerIVCockpitInternal")).and_then(|d| d.cockpit).expect("its inside");
        assert!(inner.inside);
        assert_eq!(inner.owner, super::pco_id("PanzerIV"));
        assert_ne!(inner.owner, super::pco_id("PanzerIV_Browning_PCO1"));
    }

    /// The Wasserfall's ramp carries its rocket (`WasserfallRocketDummy`, one fire position), on
    /// the weapon's part.
    #[test]
    fn a_launcher_shows_its_dummy_projectiles() {
        let game = bytefield_testdata::require!(Base, Xpack2);
        let w = bytefield_sim::World::load(&game, "XPack2", "Essen", "conquest").unwrap();
        let v = w.vehicle("Wasserfall").unwrap();
        let d: Vec<_> = super::draws(&w.db, "Wasserfall", &v.craft.parts).into_iter().filter(|d| d.dummy).collect();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].template, "WasserfallRocketDummy");
        assert!(d[0].part.is_some_and(|k| matches!(v.craft.parts[k].kind, bytefield_sim::craft::Kind::Weapon(_))));
    }

    /// A dummy projectile takes only its `addFireArmsPosition`'s position, not its turn: the F-14B's four MK83
    /// (`0/35/0`) hang level, as in the original.
    #[test]
    fn dummy_projectiles_take_no_turn() {
        let Some(game) = bytefield_testdata::with_mod("DC_Final") else { return; };
        let w = bytefield_sim::World::load(&game, "DC_Final", "El_Alamein", "conquest").unwrap();
        let v = w.vehicle("F-14B").unwrap();
        let d: Vec<_> = super::draws(&w.db, "F-14B", &v.craft.parts).into_iter().filter(|d| d.dummy && d.template.eq_ignore_ascii_case("MK83Dummy")).collect();
        assert_eq!(d.len(), 4);
        for x in &d {
            // the rack hangs straight under the plane: no pitch on the dummy's forward axis
            let z = x.local.0[2];
            assert!(z[1].abs() < 1e-6 && (z[2] - 1.0).abs() < 1e-6, "{:?}", x.local);
        }
    }
}
