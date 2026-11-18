//! The template database: what the `.con` scripts build.
//!
//! Generic over namespaces, because the language is: `X.create <Type> <name>` makes a template
//! of namespace X and makes it X's active one; `X.active <name>` (`X.setActive` for geometry)
//! switches back to an existing one; any other `X.<prop> args` is recorded on X's active
//! template, in order. `Object.create <template>` makes an instance (placed in the level), and
//! `Object.<prop>` applies to the last instance. Everything else (`game.*`, `renderer.*`...) is
//! recorded as a global call, in order.
//!
//! Properties are stored as written (method lowercase + raw args): turning them into typed
//! values is the simulation's job (each type reads what it understands), and keeping the raw
//! form lets the tests compare against the scripts line by line.
//!
//! HYPOTHESES: a second `create` of an existing name replaces it; a
//! property on a namespace with no active template is dropped.

use std::collections::HashMap;

use serde::Serialize;

use crate::{Command, Host, Outcome};

#[derive(Debug, Clone, Serialize)]
pub struct Property {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Template {
    pub namespace: String,
    /// as written in the create (`PlayerControlObject`)
    pub kind: String,
    /// as written (`T34`)
    pub name: String,
    pub props: Vec<Property>,
    /// where it was created, for messages
    pub origin: String,
    /// the order it was created in (among all templates)
    pub seq: usize,
    /// the create's arguments (`aiStrategicArea.create` has its bounds there)
    pub create_args: Vec<String>,
}

impl Template {
    /// The last value of a property (by lowercase name, with or without `set`).
    pub fn get(&self, name: &str) -> Option<&[String]> {
        // (without building the other spelling: the AI asks this every tick)
        let n: std::borrow::Cow<str> = if name.bytes().any(|b| b.is_ascii_uppercase()) { name.to_ascii_lowercase().into() } else { name.into() };
        let n = n.as_ref();
        let stripped = n.strip_prefix("set");
        self.props
            .iter()
            .rev()
            .find(|p| {
                p.name == n
                    || match stripped {
                        Some(alt) => p.name == alt,
                        None => p.name.strip_prefix("set") == Some(n),
                    }
            })
            .map(|p| p.args.as_slice())
    }

    /// All values of a repeatable property, like `addTemplate`.
    pub fn all(&self, name: &str) -> Vec<&[String]> {
        let n = name.to_ascii_lowercase();
        self.props.iter().filter(|p| p.name == n).map(|p| p.args.as_slice()).collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Instance {
    pub template: String,
    pub props: Vec<Property>,
    pub origin: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Call {
    pub command: String,
    pub args: Vec<String>,
}

/// `X.create <Type> <name>`; with one argument (`aiTemplate.create <name>`,
/// `weaponTemplate.create <name>`) the namespace is the type, and `aiStrategicArea.create`
/// names an area followed by its bounds; NetworkableInfo uses
/// `createNewInfo <name>`.
fn create_shape(namespace: &str, method: &str, args: &[String]) -> Option<(String, String)> {
    match (namespace, method, args) {
        ("object", _, _) => None,
        ("networkableinfo", "createnewinfo", [name, ..]) => Some(("NetworkableInfo".into(), name.clone())),
        // aiStrategicArea.create <name> <min x/z> <max x/z> <height>
        ("aistrategicarea", "create", [name, ..]) => Some(("aiStrategicArea".into(), name.clone())),
        (_, "create", [kind, name, ..]) => Some((kind.clone(), name.clone())),
        (_, "create", [name]) => Some((namespace.to_string(), name.clone())),
        _ => None,
    }
}

/// The console namespaces BF1942.exe knows (each appears as a name in its data section)
///. A line in any other namespace (`remObjectTemplate.x`,
/// `OjectTemplate.x`, `//ObjectTemplate.x`: all in shipped content) is ignored by the game.
/// `render.` is not there either, though DC uses it twice: HYPOTHESIS that it is ignored.
pub const KNOWN_NAMESPACES: &[&str] = &[
    "object", "objecttemplate", "materialmanager", "geometrytemplate", "aitemplateplugin", "animationstatemachine",
    "aitemplate", "game", "spawnpointmanager", "aistrategicarea", "weapontemplate", "aistrategy", "networkableinfo",
    "aisettings", "lodselectortemplate", "water", "ai", "renderer", "sky", "raytracer", "cloud", "controlmap",
    "texturemanager", "soldiersound", "console", "terrain", "aibotmanager", "shadermanager", "kittemplate", "shadow",
    "environmentsound", "objectivemanager", "airadio", "scoremanager", "ailandingzone", "sound", "treerenderer", "menu",
];

/// The namespaces whose managers keep names as written (an `std::map<std::string>` each: the
/// AI's; `DefgunPhysical` and `DefGunPhysical` are two plug-ins). The rest are looked
/// up in lowercase.
pub const CASE_SENSITIVE: &[&str] = &["aitemplate", "aitemplateplugin", "weapontemplate", "kittemplate"];

/// The key a name has in a namespace.
fn key_name(ns: &str, name: &str) -> String {
    if CASE_SENSITIVE.contains(&ns) { name.to_string() } else { name.to_ascii_lowercase() }
}

/// The class names `ObjectTemplate.create` knows, as the original
/// game registers them at start. A name is a class when it is the beginning of one of these, in any
/// case (`getClassIDFromTemplateClassName`: `strncasecmp` with the given name's length), so `DestroyTargetObjective`
/// is one and GC's `SimepleObject` is not.
pub const OBJECT_CLASSES: &[&str] = &[
    "ANDCompositeObjectiveTemplate", "ActiveKitPart", "AnimatedBundle", "AreaObject", "BFSoldier", "Bundle", "Camera",
    "ControlPoint", "DestroyTargetObjectiveTemplate", "DummyObject", "EffectBundle", "Emitter", "Engine", "EntryPoint",
    "FireArms", "FireArmsBundle", "Flag", "FlagBase", "FloatingBundle", "FreeCamera", "HandFireArms", "Item", "Kit",
    "KitPart", "LandingGear", "LensFlare", "Light", "LodObject", "ObjectSpawner", "Obstacle", "Particle",
    "PlayerControlObject", "Projectile", "RotationalBundle", "SeatObject", "SimpleObject", "SonarObject", "SpawnPoint",
    "Spring", "SpriteParticle", "SupplyDepot", "TimerObjectiveTemplate", "Wing",
];

pub fn is_object_class(kind: &str) -> bool {
    !kind.is_empty() && OBJECT_CLASSES.iter().any(|c| c.len() >= kind.len() && c[..kind.len()].eq_ignore_ascii_case(kind))
}

#[derive(Default, Serialize)]
pub struct TemplateDb {
    /// (namespace, name: lowercase, or as written in `CASE_SENSITIVE`) -> template
    pub templates: HashMap<(String, String), Template>,
    pub instances: Vec<Instance>,
    pub globals: Vec<Call>,
    /// names created twice (the second made nothing: the first wins)
    pub recreated: Vec<String>,
    /// lines dropped because their namespace had no active template
    pub orphan_props: usize,
    /// lines dropped because they followed a `create` of a name that already was (the first wins)
    pub muted_props: usize,
    /// object templates of a class the game does not have (nothing made; the lines after them go nowhere)
    pub unknown_class: Vec<String>,
    /// each object template's number (by lowercase name): the original's counter, which
    /// `CreateObjectEvent` sends; `delete_object_templates` keeps counting
    pub object_numbers: HashMap<String, u32>,
    #[serde(skip)]
    next_object_number: u32,
    #[serde(skip)]
    active: HashMap<String, String>,
    /// the namespaces whose last `create` was of a name that already was
    #[serde(skip)]
    muted: std::collections::HashSet<String>,
    #[serde(skip)]
    created: usize,
    #[serde(skip)]
    namespaces_with_templates: std::collections::HashSet<String>,
}

impl TemplateDb {
    pub fn get(&self, namespace: &str, name: &str) -> Option<&Template> {
        let ns = namespace.to_ascii_lowercase();
        let key = key_name(&ns, name);
        self.templates.get(&(ns, key))
    }

    pub fn object_template(&self, name: &str) -> Option<&Template> {
        self.get("objecttemplate", name)
    }

    /// Forgets every object template, as the game's load does before the level (GC's `game/init.con`
    /// makes two cameras that `objects/` makes again, with new numbers). The numbers go on from where they were.
    pub fn delete_object_templates(&mut self) {
        self.templates.retain(|(ns, _), _| ns != "objecttemplate");
        self.object_numbers.clear();
        self.active.remove("objecttemplate");
    }

    fn number(&mut self, name: &str) {
        self.object_numbers.insert(name.to_ascii_lowercase(), self.next_object_number);
        self.next_object_number += 1;
    }

    /// The object templates of a kind, in the order they were created.
    pub fn objects_of_kind(&self, kind: &str) -> Vec<&Template> {
        let mut v: Vec<&Template> = self.templates.iter().filter(|((ns, _), t)| ns == "objecttemplate" && t.kind.eq_ignore_ascii_case(kind)).map(|(_, t)| t).collect();
        v.sort_by_key(|t| t.seq);
        v
    }

    /// Templates counted by (namespace, kind).
    pub fn census(&self) -> Vec<((String, String), usize)> {
        let mut m: HashMap<(String, String), usize> = HashMap::new();
        for t in self.templates.values() {
            *m.entry((t.namespace.clone(), t.kind.to_ascii_lowercase())).or_default() += 1;
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }
}

impl Host for TemplateDb {
    fn exec(&mut self, cmd: &Command) -> Outcome {
        let ns = cmd.namespace.as_str();
        let origin = || format!("{}:{}", cmd.file, cmd.line);
        if !KNOWN_NAMESPACES.contains(&ns) {
            return Outcome::Unknown;
        }

        if let Some((kind, name)) = create_shape(ns, &cmd.method, &cmd.args) {
            let key = (ns.to_string(), key_name(ns, &name));
            // the first one wins: the managers' `createTemplate` finds the name, sets no active
            // template and makes nothing, so what follows the second `create` goes nowhere
            // (`ObjectTemplateManager`, `GeometryTemplateManager`, the AI's…: GC's
            // `SpaceRocket`, a Projectile and then a SimpleObject, is the B-wing's torpedo)
            if self.templates.contains_key(&key) {
                self.recreated.push(format!("{ns}.{name} at {}", origin()));
                self.active.remove(ns);
                self.muted.insert(ns.into());
                return Outcome::Done(None);
            }
            if ns == "objecttemplate" && !is_object_class(&kind) {
                // `createTemplate` gets no class: no template, and no active one
                self.unknown_class.push(format!("{kind} {name} at {}", origin()));
                self.active.remove(ns);
                self.muted.insert(ns.into());
                return Outcome::Done(None);
            }
            if ns == "objecttemplate" {
                self.number(&name);
            }
            self.muted.remove(ns);
            let seq = self.created;
            self.created += 1;
            self.templates.insert(key, Template { namespace: ns.into(), kind, name: name.clone(), props: Vec::new(), origin: origin(), seq, create_args: cmd.args.clone() });
            self.active.insert(ns.into(), key_name(ns, &name));
            self.namespaces_with_templates.insert(ns.into());
            return Outcome::Done(None);
        }
        if ns == "object" {
            if cmd.method == "create" {
                let Some(t) = cmd.args.first() else { return Outcome::Error("create without a template".into()) };
                // the first one places the object at the console camera, which the object manager makes
                // then, as a template like any other: it takes a number and becomes the active one
                let cam = ("objecttemplate".to_string(), "objectmanagercamera".to_string());
                if self.object_template(t).is_some() && !self.templates.contains_key(&cam) {
                    self.number("ObjectManagerCamera");
                    let seq = self.created;
                    self.created += 1;
                    self.templates.insert(cam.clone(), Template { namespace: cam.0.clone(), kind: "Camera".into(), name: "ObjectManagerCamera".into(), props: Vec::new(), origin: origin(), seq, create_args: Vec::new() });
                    self.active.insert(cam.0, cam.1);
                    self.muted.remove("objecttemplate");
                }
                self.instances.push(Instance { template: t.clone(), props: Vec::new(), origin: origin() });
                return Outcome::Done(None);
            }
            return match self.instances.last_mut() {
                Some(i) => {
                    i.props.push(Property { name: cmd.method.clone(), args: cmd.args.clone() });
                    Outcome::Done(None)
                }
                None => {
                    self.orphan_props += 1;
                    Outcome::Error("no object created yet".into())
                }
            };
        }
        if cmd.method == "active" || cmd.method == "setactive" {
            let Some(name) = cmd.args.first() else { return Outcome::Error("active without a name".into()) };
            let key = (ns.to_string(), key_name(ns, name));
            if self.templates.contains_key(&key) {
                self.active.insert(ns.into(), key.1);
                self.muted.remove(ns);
                return Outcome::Done(None);
            }
            return Outcome::Error(format!("no template {name}"));
        }
        if let Some(name) = self.active.get(ns).cloned()
            && let Some(t) = self.templates.get_mut(&(ns.to_string(), name))
        {
            t.props.push(Property { name: cmd.method.clone(), args: cmd.args.clone() });
            return Outcome::Done(None);
        }
        if self.muted.contains(ns) {
            self.muted_props += 1;
            return Outcome::Done(None);
        }
        if self.namespaces_with_templates.contains(ns) {
            self.orphan_props += 1;
            return Outcome::Error("no active template".into());
        }
        // a global command: game.*, renderer.*, … Getters return nothing we know yet.
        self.globals.push(Call { command: format!("{}.{}", cmd.namespace, cmd.method), args: cmd.args.clone() });
        Outcome::Done(None)
    }
}
