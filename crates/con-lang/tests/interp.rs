//! The interpreter on made-up scripts (no game data needed).

use std::collections::HashMap;

use bytefield_con::templates::TemplateDb;
use bytefield_con::{Command, Host, Interpreter, Kind, Outcome, ScriptFs};

struct MemFs(HashMap<String, String>);

impl ScriptFs for MemFs {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(&bytefield_vfs::normalize(path)).map(|s| s.as_bytes().to_vec())
    }
}

fn fs(files: &[(&str, &str)]) -> MemFs {
    MemFs(files.iter().map(|(p, s)| (bytefield_vfs::normalize(p), s.to_string())).collect())
}

/// Records every command it gets; answers `game.getGamePlayMode` like the engine would.
#[derive(Default)]
struct Log(Vec<String>);

impl Host for Log {
    fn exec(&mut self, cmd: &Command) -> Outcome {
        if cmd.namespace == "game" && cmd.method == "getgameplaymode" {
            return Outcome::Done(Some("GPM_CQ".into()));
        }
        if cmd.namespace == "nope" {
            return Outcome::Unknown;
        }
        self.0.push(format!("{}.{} {}", cmd.namespace, cmd.method, cmd.args.join(" ")));
        Outcome::Done(None)
    }
}

#[test]
fn run_args_if_else_and_nesting() {
    let f = fs(&[
        ("levels/x/init.con", "run GameTypes/Conquest host\nrun GameTypes/Conquest client\n"),
        ("levels/x/gametypes/conquest.con", "if v_arg1 == host\n  run Conquest/CPs v_arg1\nelse\n  a.client\nendif\n"),
        // found from the level dir, not next to the gametype script (the ancestor rule)
        ("levels/x/conquest/cps.con", "a.cp v_arg1\nif 1 == 2\n  if 1 == 1\n    a.never\n  endif\nelseif 2 == 2\n  a.elseif\nelse\n  a.never\nendif\n"),
    ]);
    let mut it = Interpreter::new(&f, Log::default());
    assert!(it.run_file("levels/x/init.con", &[]));
    assert_eq!(it.host.0, vec!["a.cp host", "a.elseif ", "a.client "]);
    assert_eq!(it.runs.file, 2);
    assert_eq!(it.runs.ancestor, 1);
    assert!(it.diagnostics.is_empty(), "{:?}", it.diagnostics);
}

#[test]
fn var_assign_const_and_include_scope() {
    let f = fs(&[
        ("init.con", "Const c_bots = 0\nVar v_mode\ngame.getGamePlayMode -> v_mode\nif v_mode == GPM_CQ\n  a.cq v_mode\nendif\nif c_bots == 1\n  a.bots\nendif\ninclude common.inc\nrun other\n"),
        ("common.inc", "a.inc v_mode\n"),        // include: same scope, sees v_mode
        ("other.con", "a.other v_mode c_bots\n"), // run: new scope; consts are global
    ]);
    let mut it = Interpreter::new(&f, Log::default());
    it.run_file("init.con", &[]);
    assert_eq!(it.host.0, vec!["a.cq GPM_CQ", "a.inc GPM_CQ", "a.other v_mode 0"]);
}

#[test]
fn an_undeclared_variable_runs_the_body() {
    // GC_Bespin_Night: the Var of Init.con is gone in Conquest.con; the original sets the tickets anyway
    let f = fs(&[
        ("init.con", "Var v_mode\ngame.getGamePlayMode -> v_mode\nrun conquest\n"),
        ("conquest.con", "if v_mode == GPM_CQ\n  a.tickets\nendif\nif 1 == 2\n  if v_mode == X\n    a.never\n  endif\nendif\na.after\n"),
    ]);
    let mut it = Interpreter::new(&f, Log::default());
    it.run_file("init.con", &[]);
    assert_eq!(it.host.0, vec!["a.tickets ", "a.after "]);
}

#[test]
fn diagnostics() {
    let f = fs(&[("x.con", "run missing\n*** banner ***\nnope.cmd\nendif\nif 1 ==\nendif\nif 1 == 1\nendrem\n")]);
    let mut it = Interpreter::new(&f, Log::default());
    it.run_file("x.con", &[]);
    let kinds: Vec<Kind> = it.diagnostics.iter().map(|d| d.kind).collect();
    assert_eq!(kinds, vec![Kind::MissingScript, Kind::StrayLine, Kind::UnknownCommand, Kind::Unbalanced, Kind::BadCondition, Kind::Unbalanced, Kind::Unbalanced]);
}

#[test]
fn recursion_is_bounded() {
    let f = fs(&[("a.con", "run a\n")]);
    let mut it = Interpreter::new(&f, Log::default());
    it.run_file("a.con", &[]);
    assert!(it.diagnostics.iter().any(|d| d.kind == Kind::Recursion));
}

#[test]
fn templates_active_across_files_and_instances() {
    let f = fs(&[
        ("objects/xwing/objects.con", "ObjectTemplate.create PlayerControlObject XWing\nObjectTemplate.hasMobilePhysics 1\nObjectTemplate.addTemplate XWingEngine\nObjectTemplate.addTemplate XWingWing\n"),
        ("objects/xwing/physics.con", "ObjectTemplate.create Engine XWingEngine\nObjectTemplate.setTorque 120\nObjectTemplate.active XWing\nObjectTemplate.setMaxSpeed 1/2/3\n"),
        ("objects/xwing/network.con", "NetworkableInfo.createNewInfo XWingInfo\nNetworkableInfo.setPredictionMode PMLinear\n"),
        ("level.con", "Object.create XWing\nObject.absolutePosition 1/2/3\nObject.create XWing\ngame.setTeamSkin 1 Rebel\n"),
    ]);
    let mut it = Interpreter::new(&f, TemplateDb::default());
    for p in ["objects/xwing/objects.con", "objects/xwing/physics.con", "objects/xwing/network.con", "level.con"] {
        it.run_file(p, &[]);
    }
    let db = &it.host;
    let x = db.object_template("xwing").unwrap();
    assert_eq!(x.kind, "PlayerControlObject");
    assert_eq!(x.all("addtemplate").len(), 2);
    assert_eq!(x.get("maxSpeed").unwrap(), ["1/2/3"]); // set/no-set both match
    assert_eq!(db.object_template("XWingEngine").unwrap().get("settorque").unwrap(), ["120"]);
    assert_eq!(db.get("networkableinfo", "xwinginfo").unwrap().get("predictionmode").unwrap(), ["PMLinear"]);
    assert_eq!(db.instances.len(), 2);
    assert_eq!(db.instances[0].props[0].args, ["1/2/3"]);
    assert_eq!(db.globals[0].command, "game.setteamskin");
    assert!(it.diagnostics.is_empty(), "{:?}", it.diagnostics);
}

/// A template created twice keeps the first: the
/// second `create` sets no active template, so its lines go nowhere until the next `create` or
/// `active`. GC's `SpaceRocket`, a Projectile and then a SimpleObject, stays the Projectile.
#[test]
fn a_template_created_twice_keeps_the_first() {
    let f = fs(&[(
        "objects/spacerocket/weapons.con",
        "ObjectTemplate.create Projectile SpaceRocket\nObjectTemplate.mass 20\nObjectTemplate.create SimpleObject spacerocket\nObjectTemplate.mass 1\nObjectTemplate.active SpaceRocket\nObjectTemplate.drag .5\n",
    )]);
    let mut it = Interpreter::new(&f, TemplateDb::default());
    it.run_file("objects/spacerocket/weapons.con", &[]);
    let db = &it.host;
    let t = db.object_template("SpaceRocket").unwrap();
    assert_eq!(t.kind, "Projectile");
    assert_eq!(t.all("mass").len(), 1);
    assert_eq!(t.get("mass").unwrap(), ["20"]);
    assert_eq!(t.get("drag").unwrap(), [".5"]);
    assert_eq!(db.recreated.len(), 1);
    assert_eq!((db.muted_props, db.orphan_props), (1, 0));
    assert!(it.diagnostics.is_empty(), "{:?}", it.diagnostics);
}
