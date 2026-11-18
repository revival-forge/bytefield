//! `createObject` in its form: one way to make an object of
//! any template, whoever asks (a spawner, a weapon, the round's start). The server makes it by
//! its template's `createObject`, its constructor makes its children (`addBundleChilds`, the tree
//! of [`bytefield_world::tree`]) and each child's `init` registers it wherever it is: a spawner or a
//! depot in a vehicle's, a projectile's or a static's tree works as a placed one. Here the object
//! goes to the system that handles its kind ([`Sim::instantiate`]); what none handles is counted
//! in [`Sim::unhandled`], so a gap shows instead of passing unnoticed.

use std::sync::Arc;

use serde::Serialize;

use crate::Sim;
use crate::vecmath::V3;

/// What an object hangs from (`addChild`'s parent): what its children follow.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Parent {
    /// placed by the level's scripts
    Level,
    /// in a placed static's tree, or in that of an object that does not move ([`Item`])
    Static,
    /// in a vehicle's tree (`vehicles`)
    Vehicle(usize),
    /// in a projectile's tree (`projectiles`): DC's deployed mortar and PKM, GC's E-Web
    Projectile(usize),
}

/// What [`Sim::instantiate`] made.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Made {
    /// a `PlayerControlObject` the sim moves, mans or destroys (`vehicles`)
    Vehicle(usize),
    /// an object that only lies there (`items`)
    Item(usize),
}

/// An object made that only lies where it was made: a kit (carried or on the ground, `kit.rs`), a
/// bundle or a `SimpleObject` from a spawner (DC's `armory_m82`, GC's ammunition).
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub template: String,
    /// lowercase
    pub kind: String,
    pub pos: V3,
    pub rows: [V3; 3],
    pub removed: bool,
    /// a kit: the soldier (`vehicles`) that carries it; on the ground, None
    pub carrier: Option<usize>,
    /// a kit on the ground that was carried: its weapons as they were (ammunition and all)
    pub weapons: Vec<crate::firearms::FireArms>,
    /// and its active parts as they were (their heat: `kitpart.rs`)
    pub parts: Vec<crate::kitpart::KitPart>,
    /// a fallen kit: the step it goes (the kit's timer)
    pub expires: Option<u64>,
}

/// The kinds made elsewhere: the objectives of objective mode (`objective.rs` reads them from the
/// spawners that make them).
const ELSEWHERE: &[&str] = &["andcompositeobjective", "destroytargetobjective", "timerobjective"];

/// The kinds that only lie there when nothing else takes them ([`Item`]).
const ITEMS: &[&str] = &["kit", "bundle", "simpleobject", "animatedbundle", "rotationalbundle", "playercontrolobject"];

impl Sim<'_> {
    /// The template manager's create object of `template` at `pos` with axes `rows` (a root: what a
    /// spawner makes hangs from nothing), and its children's `init`s ([`Sim::add_tree`]). A `PlayerControlObject` with
    /// mobile physics, a seat or hit points is a vehicle (at rest; a fixed gun or a target does
    /// not move); kits, bundles and other objects, an [`Item`]. Anything else, and a template
    /// that does not exist, is counted in `unhandled` and not made.
    pub fn instantiate(&mut self, template: &str, pos: V3, rows: [V3; 3]) -> Option<Made> {
        let Some(t) = self.world.db.object_template(template) else {
            *self.journal.unhandled.entry(format!("? {template}")).or_default() += 1;
            return None;
        };
        self.journal.created.push(template.to_string());
        let kind = t.kind.to_ascii_lowercase();
        let vehicle = kind == "playercontrolobject" && (self.world.is_mobile(template) || self.world.is_fixed_gun(template) || self.world.is_target(template));
        if vehicle && let Some(i) = self.spawn_vehicle(template, pos, [0.0; 3]) {
            self.vehicles[i].craft.body.rows = rows;
            self.add_tree(template, Parent::Vehicle(i), pos, rows);
            return Some(Made::Vehicle(i));
        }
        if ELSEWHERE.contains(&kind.as_str()) {
            return None;
        }
        if !ITEMS.contains(&kind.as_str()) {
            *self.journal.unhandled.entry(format!("{kind} {template}")).or_default() += 1;
            return None;
        }
        self.items.push(Item { template: template.to_string(), kind, pos, rows, removed: false, carrier: None, weapons: Vec::new(), parts: Vec::new(), expires: None });
        self.add_tree(template, Parent::Static, pos, rows);
        Some(Made::Item(self.items.len() - 1))
    }

    /// The templates of the spawners' first objects in the order the server makes them, once the round has started:
    /// its spawners' list is the scripts' order (ours runs reversed, [`Sim::start_conquest`]), the level's spawners
    /// first, then those in their objects' trees. The ghost ids of the original follow it (
    /// checked on El Alamein).
    pub fn first_spawns_in_script_order(&self) -> Vec<String> {
        // the level's ([`SpawnerState::of`]: placed, or in a placed static's tree) and those in the tree of an object a
        // spawner made, pushed
        // right after the spawner that made the object
        let (mut top, mut kids, mut owner) = (Vec::new(), Vec::new(), None);
        for s in &self.spawners {
            if s.local.is_none() {
                owner = Some(top.len());
                top.push(s);
            } else {
                kids.push((owner, s));
            }
        }
        let n = top.len();
        top.reverse();
        // those in an object's tree after all the level's, in the order of their objects (in each, as its tree has them)
        kids.sort_by_key(|(o, _)| o.map_or(usize::MAX, |o| n - 1 - o));
        top.iter().copied().chain(kids.iter().map(|(_, s)| *s)).filter(|s| s.slots.first().is_some_and(|o| o.is_some())).map(|s| s.template.clone()).collect()
    }

    /// The `init`s of the spawners and depots in `template`'s tree (the object just made at `pos`
    /// with axes `rows`, hanging from `parent`): registered as they are, following their object.
    /// (A vehicle's depots are added by [`Sim::spawn_vehicle`].) A spawner makes its first object
    /// in its next frame, unless `spawnDelayAtStart`.
    pub(crate) fn add_tree(&mut self, template: &str, parent: Parent, pos: V3, rows: [V3; 3]) {
        for (sp, m) in bytefield_world::child_spawners(&self.world.db, template) {
            let mut st = crate::damage::SpawnerState::child(&sp, &m, parent);
            st.follow(pos, rows);
            self.spawners.push(st);
        }
        match parent {
            Parent::Vehicle(_) => {}
            Parent::Projectile(k) => self.add_projectile_depots(k, template),
            Parent::Level | Parent::Static => {
                for (name, m) in bytefield_world::child_templates_of_kind(&self.world.db, template, "supplydepot") {
                    let Some(dt) = self.world.depot_template(&name) else { continue };
                    let c = m.0;
                    let l = [c[3][0] as f32, c[3][1] as f32, c[3][2] as f32];
                    let at = [0, 1, 2].map(|j| pos[j] + l[0] * rows[0][j] + l[1] * rows[1][j] + l[2] * rows[2][j]);
                    let d = crate::supply::Depot::new(Arc::new(dt), None, at, crate::supply::phase(self.depots.len()));
                    self.depots.push(d);
                }
            }
        }
    }
}
