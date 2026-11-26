//! A vehicle (or any PlayerControlObject) as a tree of physics components, read from templates.
//!
//! This is the data the vehicle physics will run on. It does not say yet *how* an `Engine` or a
//! `Wing` turns its numbers into forces: that is the part to measure against the original. Here every component keeps its template kind, its place in the
//! parent (the `setPosition`/`setRotation` after its `addTemplate`) and its properties, parsed
//! into numbers and vectors where they are numbers and vectors.

use std::collections::BTreeMap;

use bytefield_con::templates::{Template, TemplateDb};
use bytefield_world::scene::children;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(untagged)]
pub enum Value {
    Number(f64),
    Vector(Vec<f64>),
    Text(String),
    List(Vec<String>),
}

impl Value {
    pub fn parse(args: &[String]) -> Value {
        match args {
            [one] => {
                if let Ok(n) = one.parse::<f64>() {
                    Value::Number(n)
                } else if let Some(v) = bytefield_world::parse_vec(one).filter(|v| v.len() > 1) {
                    Value::Vector(v)
                } else {
                    Value::Text(one.clone())
                }
            }
            _ => Value::List(args.to_vec()),
        }
    }

    pub fn number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn vector(&self) -> Option<&[f64]> {
        match self {
            Value::Vector(v) => Some(v),
            _ => None,
        }
    }
}

/// Properties that only matter to rendering, sound, HUD or networking: left out of the model.
const COSMETIC: &[&str] = &[
    "geometry", "setnetworkableinfo", "networkableinfo", "loadsoundscript", "saveinseparatefile",
    "hasdynamicshadow", "setvehicleicon", "setvehicleiconpos", "setminimapicon", "setprimaryammoicon", "setsecondaryammoicon",
    "setprimaryammobar", "setsecondaryammobar", "setnumberofweaponicons", "settooltiptype", "setcrosshairtype", "guiindex",
    "addarmoreffect", "aitemplate", "cullradiusscale", "addtemplate", "setposition", "setrotation", "position", "rotation",
    "lodselector", "setattachtolistener",
];

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub name: String,
    pub kind: String,
    /// position and yaw/pitch/roll in the parent's frame
    pub position: [f64; 3],
    pub rotation: [f64; 3],
    pub params: BTreeMap<String, Value>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn param(&self, name: &str) -> Option<&Value> {
        let n = name.to_ascii_lowercase();
        self.params.get(&n).or_else(|| self.params.get(&format!("set{n}"))).or_else(|| n.strip_prefix("set").and_then(|s| self.params.get(s)))
    }

    /// All components of a kind, depth first.
    pub fn find_all<'a>(&'a self, kind: &str, out: &mut Vec<&'a Component>) {
        if self.kind.eq_ignore_ascii_case(kind) {
            out.push(self);
        }
        for c in &self.children {
            c.find_all(kind, out);
        }
    }

    pub fn count(&self) -> usize {
        1 + self.children.iter().map(Component::count).sum::<usize>()
    }

    /// `internalFindFirstChildOfCID(first child, kind, stop)`, as enter looks for
    /// its `SeatObject` (stop: another `PlayerControlObject`): a node, then its later siblings, then the children of
    /// those siblings from the last one back; a `stop` ends its list there (it and what follows it are not looked at,
    /// the earlier siblings' children still are). Not depth first: the SA-9's gunner sits in the seat of the
    /// `RotationalBundle` that yaws, not in the one of the launcher under it that pitches.
    // TODO: the node flag at +4 bit 0 (set: the node and its children are skipped, its siblings are not) is not
    // modelled; what sets it on an object made from a template is unknown. Also, a `LodObject` keeps only its first
    // child here ([`model`]); the original may find a seat under the others. No data has either case.
    pub fn first_child_of_kind(&self, kind: &str, stop: &str) -> Option<&Component> {
        fn list<'a>(ks: &'a [Component], kind: &str, stop: &str) -> Option<&'a Component> {
            let ks = &ks[..ks.iter().position(|k| k.kind.eq_ignore_ascii_case(stop)).unwrap_or(ks.len())];
            ks.iter().find(|k| k.kind.eq_ignore_ascii_case(kind)).or_else(|| ks.iter().rev().find_map(|k| list(&k.children, kind, stop)))
        }
        list(&self.children, kind, stop)
    }

    /// Of the `SeatObject`s under this `PlayerControlObject` (not under another one), depth first, the index of the one
    /// it seats its player in ([`Component::first_child_of_kind`]); the client and [`crate::entry`] walk depth first.
    pub fn seat_rank(&self) -> Option<usize> {
        fn pre<'a>(c: &'a Component, out: &mut Vec<&'a Component>) {
            for k in c.children.iter().filter(|k| !k.kind.eq_ignore_ascii_case("PlayerControlObject")) {
                if k.kind.eq_ignore_ascii_case("SeatObject") {
                    out.push(k);
                }
                pre(k, out);
            }
        }
        let seat = self.first_child_of_kind("SeatObject", "PlayerControlObject")?;
        let mut all = Vec::new();
        pre(self, &mut all);
        all.iter().position(|s| std::ptr::eq(*s, seat))
    }
}

fn params(t: &Template) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for p in &t.props {
        if COSMETIC.contains(&p.name.as_str()) {
            continue;
        }
        // a seat's flags add up (the T-34-85's gunner: `c_SeatShowHalfBodySoldier`, then `c_SeatIsOutside`)
        if p.name == "seatflags" {
            if let Some(Value::List(all)) = out.get_mut("seatflags") {
                all.extend(p.args.iter().cloned());
            } else {
                out.insert(p.name.clone(), Value::List(p.args.clone()));
            }
            continue;
        }
        out.insert(p.name.clone(), Value::parse(&p.args));
    }
    out
}

fn build(db: &TemplateDb, name: &str, position: [f64; 3], rotation: [f64; 3], depth: usize) -> Option<Component> {
    if depth > 16 {
        return None;
    }
    let t = db.object_template(name)?;
    // children with their placement: re-read the raw position/rotation, not the matrix
    let mut placed: Vec<(String, [f64; 3], [f64; 3])> = Vec::new();
    for p in &t.props {
        let v = || p.args.first().and_then(|a| bytefield_world::parse_vec(a)).filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]);
        match p.name.as_str() {
            "addtemplate" => {
                if let Some(n) = p.args.first() {
                    placed.push((n.clone(), [0.0; 3], [0.0; 3]));
                }
            }
            "setposition" | "position" => {
                if let (Some(last), Some(v)) = (placed.last_mut(), v()) {
                    last.1 = v;
                }
            }
            "setrotation" | "rotation" => {
                if let (Some(last), Some(v)) = (placed.last_mut(), v()) {
                    last.2 = v;
                }
            }
            "setrandomgeometries" => {
                if let Some(last) = placed.last_mut() {
                    last.0 = bytefield_world::scene::random_child(&last.0, &p.args);
                }
            }
            _ => {}
        }
    }
    debug_assert_eq!(placed.len(), children(t).len());
    let lod = t.kind.eq_ignore_ascii_case("lodobject");
    let kids = placed
        .into_iter()
        .take(if lod { 1 } else { usize::MAX })
        .filter_map(|(n, p, r)| build(db, &n, p, r, depth + 1))
        .collect();
    Some(Component { name: t.name.clone(), kind: t.kind.clone(), position, rotation, params: params(t), children: kids })
}

/// The component tree of a template, or None if it does not exist.
pub fn model(db: &TemplateDb, template: &str) -> Option<Component> {
    build(db, template, [0.0; 3], [0.0; 3], 0)
}

/// A one-line physics summary, for listings.
#[derive(Debug, Serialize)]
pub struct Summary {
    pub name: String,
    pub mass: Option<f64>,
    pub drag: Option<f64>,
    pub components: usize,
    pub engines: Vec<String>,
    pub wings: usize,
    pub springs: usize,
    pub landing_gear: usize,
    pub floating: usize,
}

pub fn summary(c: &Component) -> Summary {
    let mut engines = Vec::new();
    c.find_all("engine", &mut engines);
    let count = |k: &str| {
        let mut v = Vec::new();
        c.find_all(k, &mut v);
        v.len()
    };
    Summary {
        name: c.name.clone(),
        mass: c.param("mass").and_then(Value::number),
        drag: c.param("drag").and_then(Value::number),
        components: c.count(),
        engines: engines
            .iter()
            .map(|e| match e.param("engineType") {
                Some(Value::Text(t)) => t.clone(),
                _ => "?".into(),
            })
            .collect(),
        wings: count("wing"),
        springs: count("spring"),
        landing_gear: count("landinggear"),
        floating: count("floatingbundle"),
    }
}

#[cfg(test)]
mod tests {
    use super::Component;

    fn c(name: &str, kind: &str, children: Vec<Component>) -> Component {
        Component { name: name.into(), kind: kind.into(), position: [0.0; 3], rotation: [0.0; 3], params: Default::default(), children }
    }

    /// `internalFindFirstChildOfCID`'s order: siblings before children, the children of the last sibling first, and
    /// a `PlayerControlObject` ends the list it is in.
    #[test]
    fn the_seat_the_original_finds() {
        let seat = |n: &str| c(n, "SeatObject", vec![]);
        // the SA-9: holder [turret [launcher, seat t], seat h, camera]: h, the second depth first
        let pco = c("p", "PlayerControlObject", vec![c("entry", "EntryPoint", vec![]), c("holder", "RotationalBundle", vec![c("turret", "RotationalBundle", vec![c("launcher", "FireArms", vec![]), seat("t")]), seat("h"), c("cam", "Camera", vec![])])]);
        assert_eq!(pco.first_child_of_kind("SeatObject", "PlayerControlObject").map(|s| s.name.as_str()), Some("h"));
        assert_eq!(pco.seat_rank(), Some(1));
        // [a [seat a], b [seat b]]: b's children are looked at before a's
        let pco = c("p", "PlayerControlObject", vec![c("a", "Bundle", vec![seat("a")]), c("b", "Bundle", vec![seat("b")])]);
        assert_eq!(pco.first_child_of_kind("SeatObject", "PlayerControlObject").map(|s| s.name.as_str()), Some("b"));
        assert_eq!(pco.seat_rank(), Some(1));
        // [a [seat a], other PCO, b [seat b]]: the search stops at the PCO, b is never seen
        let pco = c("p", "PlayerControlObject", vec![c("a", "Bundle", vec![seat("a")]), c("o", "PlayerControlObject", vec![seat("o")]), c("b", "Bundle", vec![seat("b")])]);
        assert_eq!(pco.first_child_of_kind("SeatObject", "PlayerControlObject").map(|s| s.name.as_str()), Some("a"));
        assert_eq!(pco.seat_rank(), Some(0));
        // one seat: the usual case
        let pco = c("p", "PlayerControlObject", vec![c("a", "Bundle", vec![c("b", "Bundle", vec![seat("s")])])]);
        assert_eq!(pco.seat_rank(), Some(0));
    }
}
