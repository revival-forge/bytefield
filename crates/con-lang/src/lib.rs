//! The Refractor 1 `.con` script interpreter.
//!
//! What it covers is measured, not guessed: every command, keyword and condition the base game, DC
//! and GC use was counted over their scripts (`python/tools/inventory.py`). The language part is small (`run`/`include` with arguments, `var`/`const`, `->`,
//! `if`/`elseif`/`else`/`endif`); the commands themselves go to a [`Host`] (see [`templates`]).
//!
//! Semantics that are still guesses are marked HYPOTHESIS where they are implemented.

pub mod lexer;
pub mod session;
pub mod templates;

use std::collections::HashMap;

use serde::Serialize;

/// Where scripts come from: the VFS for real data, a directory for unpacked mods and tests.
pub trait ScriptFs {
    /// The bytes of a script, by path (case-insensitive, `/` separators).
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

impl ScriptFs for bytefield_vfs::Vfs {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        bytefield_vfs::Vfs::read(self, path).ok()
    }
}

/// A directory tree, looked up without regard to case (built once).
pub struct DirFs {
    files: HashMap<String, std::path::PathBuf>,
}

impl DirFs {
    pub fn new(root: &std::path::Path) -> std::io::Result<DirFs> {
        let mut files = HashMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d)? {
                let p = e?.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                    files.insert(bytefield_vfs::normalize(&rel), p);
                }
            }
        }
        Ok(DirFs { files })
    }

    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }
}

impl ScriptFs for DirFs {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.files.get(&bytefield_vfs::normalize(path))?).ok()
    }
}

/// A command after variable substitution, as the host sees it.
#[derive(Debug, Clone)]
pub struct Command<'a> {
    /// `objecttemplate`, `game`, … lowercase; empty for a command with no dot (`echo`)
    pub namespace: String,
    /// the method, lowercase
    pub method: String,
    /// as written, for messages
    pub raw: &'a str,
    pub args: Vec<String>,
    pub file: &'a str,
    pub line: usize,
}

pub enum Outcome {
    /// Done; the value is what `-> v_x` stores.
    Done(Option<String>),
    /// The host does not know this command. Like the original, the line is skipped.
    Unknown,
    /// Known, but the arguments or state were wrong (still skipped).
    Error(String),
}

pub trait Host {
    fn exec(&mut self, cmd: &Command) -> Outcome;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, PartialOrd, Ord)]
pub enum Kind {
    /// `run`/`include` of a script that is not there
    MissingScript,
    /// a line that is neither a keyword nor `Namespace.method`
    StrayLine,
    UnknownCommand,
    CommandError,
    /// `else`/`endif`/`endrem` with nothing open, or an `if` left open at the end of a file
    Unbalanced,
    BadCondition,
    /// `while` is not used by any known content; not implemented
    Unsupported,
    /// `run` deeper than the limit (a script that runs itself)
    Recursion,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub kind: Kind,
    pub file: String,
    pub line: usize,
    pub text: String,
}

/// How each `run`/`include` target was found. Resolution order is a HYPOTHESIS:
/// next to the running script, then in its parent directories, then from the root.
#[derive(Debug, Default, Clone, Serialize)]
pub struct RunStats {
    pub file: usize,
    pub ancestor: usize,
    pub root: usize,
    pub missing: usize,
}

struct Scope {
    vars: HashMap<String, String>,
}

/// One `if` level: whether some branch was already taken, whether this branch runs.
struct Cond {
    taken: bool,
    active: bool,
    parent_active: bool,
}

pub struct Interpreter<'a, H: Host> {
    pub fs: &'a dyn ScriptFs,
    pub host: H,
    pub diagnostics: Vec<Diagnostic>,
    pub runs: RunStats,
    pub scripts_run: usize,
    pub lines_run: usize,
    consts: HashMap<String, String>,
    scopes: Vec<Scope>,
    depth: usize,
}

const MAX_DEPTH: usize = 64;

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn join(dir: &str, rel: &str) -> String {
    if dir.is_empty() { rel.to_string() } else { format!("{dir}/{rel}") }
}

impl<'a, H: Host> Interpreter<'a, H> {
    pub fn new(fs: &'a dyn ScriptFs, host: H) -> Self {
        Interpreter {
            fs,
            host,
            diagnostics: Vec::new(),
            runs: RunStats::default(),
            scripts_run: 0,
            lines_run: 0,
            consts: HashMap::new(),
            scopes: Vec::new(),
            depth: 0,
        }
    }

    fn diag(&mut self, kind: Kind, file: &str, line: usize, text: String) {
        self.diagnostics.push(Diagnostic { kind, file: file.to_string(), line, text });
    }

    /// Find a script: `run` may leave out `.con`.
    fn find(&mut self, from_dir: &str, target: &str) -> Option<(String, Vec<u8>)> {
        let target = target.replace('\\', "/");
        let low = target.to_ascii_lowercase();
        let names: Vec<String> = if low.ends_with(".con") || low.ends_with(".inc") {
            vec![target.clone()]
        } else {
            vec![format!("{target}.con"), target.clone()]
        };
        let mut dirs: Vec<(&str, String)> = vec![("file", from_dir.to_string())];
        let mut d = from_dir;
        while !d.is_empty() {
            d = dir_of(d);
            if !d.is_empty() {
                dirs.push(("ancestor", d.to_string()));
            }
        }
        dirs.push(("root", String::new()));
        for (how, dir) in dirs {
            for n in &names {
                let path = bytefield_vfs::normalize(&join(&dir, n));
                if let Some(bytes) = self.fs.read(&path) {
                    match how {
                        "file" => self.runs.file += 1,
                        "ancestor" => self.runs.ancestor += 1,
                        _ => self.runs.root += 1,
                    }
                    return Some((path, bytes));
                }
            }
        }
        self.runs.missing += 1;
        None
    }

    pub fn exists(&self, path: &str) -> bool {
        self.fs.read(&bytefield_vfs::normalize(path)).is_some()
    }

    /// Run a script by its full path (an entry point), in a new scope.
    pub fn run_file(&mut self, path: &str, args: &[String]) -> bool {
        let path = bytefield_vfs::normalize(path);
        match self.fs.read(&path) {
            Some(bytes) => {
                self.exec_script(&path, &bytes, args, true);
                true
            }
            None => {
                self.diag(Kind::MissingScript, &path, 0, "entry point not found".into());
                false
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<&String> {
        let low = name.to_ascii_lowercase();
        self.scopes.last().and_then(|s| s.vars.get(&low)).or_else(|| self.consts.get(&low))
    }

    fn substitute(&self, args: &[String]) -> Vec<String> {
        args.iter()
            .map(|a| if a.len() > 2 && (a.starts_with("v_") || a.starts_with("V_") || a.starts_with("c_") || a.starts_with("C_")) {
                self.lookup(a).cloned().unwrap_or_else(|| a.clone())
            } else {
                a.clone()
            })
            .collect()
    }

    fn undeclared<'s>(&self, args: &'s [String]) -> Option<&'s String> {
        args.iter().find(|a| {
            let low = a.to_ascii_lowercase();
            low.len() > 2 && low.starts_with("v_") && !low.starts_with("v_arg") && self.lookup(a).is_none()
        })
    }

    fn condition(&mut self, args: &[String], file: &str, line: usize) -> bool {
        let a = self.substitute(args);
        if a.len() != 3 {
            self.diag(Kind::BadCondition, file, line, a.join(" "));
            return false;
        }
        let (l, op, r) = (&a[0], a[1].as_str(), &a[2]);
        let num = l.parse::<f64>().ok().zip(r.parse::<f64>().ok());
        match (op, num) {
            ("==", Some((x, y))) => x == y,
            ("!=", Some((x, y))) => x != y,
            ("<", Some((x, y))) => x < y,
            (">", Some((x, y))) => x > y,
            ("<=", Some((x, y))) => x <= y,
            (">=", Some((x, y))) => x >= y,
            ("==", None) => l.eq_ignore_ascii_case(r),
            ("!=", None) => !l.eq_ignore_ascii_case(r),
            _ => {
                self.diag(Kind::BadCondition, file, line, a.join(" "));
                false
            }
        }
    }

    fn exec_script(&mut self, path: &str, bytes: &[u8], args: &[String], new_scope: bool) {
        if self.depth >= MAX_DEPTH {
            self.diag(Kind::Recursion, path, 0, format!("run depth {MAX_DEPTH}"));
            return;
        }
        self.depth += 1;
        self.scripts_run += 1;
        if new_scope {
            let mut vars = HashMap::new();
            // `run x` with no arguments: x still sees the caller's `v_argN` (Berlin's
            // GameTypes/Conquest.con runs ControlPoints.con so, inside its `if v_arg1 == host`,
            // and that file asks `v_arg1 == host` again: the server does make the points)
            if args.is_empty()
                && let Some(top) = self.scopes.last()
            {
                vars.extend(top.vars.iter().filter(|(k, _)| k.starts_with("v_arg")).map(|(k, v)| (k.clone(), v.clone())));
            }
            for (i, a) in args.iter().enumerate() {
                vars.insert(format!("v_arg{}", i + 1), a.clone());
            }
            self.scopes.push(Scope { vars });
        }
        // `include x a b`: x shares the caller's scope but sees its own `v_argN`
        // (`include copyToAllWeapons.inc Thompson`); the caller's come back after it
        let mut saved = Vec::new();
        if !new_scope
            && !args.is_empty()
            && let Some(top) = self.scopes.last_mut()
        {
            for (i, a) in args.iter().enumerate() {
                let k = format!("v_arg{}", i + 1);
                saved.push((k.clone(), top.vars.insert(k, a.clone())));
            }
        }
        let text = lexer::decode(bytes);
        let dir = dir_of(path).to_string();
        let mut conds: Vec<Cond> = Vec::new();

        for ln in lexer::lines(&text) {
            let active = conds.last().is_none_or(|c| c.active);
            let kw = ln.command.to_ascii_lowercase();
            match kw.as_str() {
                "if" => {
                    // a variable nobody declared (in this scope: a `Var` dies with its file) makes the
                    // line an error for the original console (`exchangeVarsAndConsts` fails): no block
                    // opens, the body runs, and its `endif` closes the one around it. GC_Bespin_Night's
                    // `Conquest.con` asks `v_gamePlayMode` from its `Init.con` and the server gives 80
                    // tickets. `v_argN` always exist: `run` sets the eight.
                    if active && let Some(name) = self.undeclared(&ln.args) {
                        self.diag(Kind::BadCondition, path, ln.number, format!("{name} undeclared: the body runs"));
                        continue;
                    }
                    let v = active && self.condition(&ln.args, path, ln.number);
                    conds.push(Cond { taken: v, active: v, parent_active: active });
                    continue;
                }
                "elseif" => {
                    let Some(top) = conds.last() else {
                        self.diag(Kind::Unbalanced, path, ln.number, "elseif".into());
                        continue;
                    };
                    let (taken, parent) = (top.taken, top.parent_active);
                    let v = parent && !taken && self.condition(&ln.args, path, ln.number);
                    let top = conds.last_mut().unwrap();
                    top.active = v;
                    top.taken |= v;
                    continue;
                }
                "else" => {
                    match conds.last_mut() {
                        Some(top) => {
                            top.active = top.parent_active && !top.taken;
                            top.taken = true;
                        }
                        None => self.diag(Kind::Unbalanced, path, ln.number, "else".into()),
                    }
                    continue;
                }
                "endif" => {
                    if conds.pop().is_none() {
                        self.diag(Kind::Unbalanced, path, ln.number, "endif".into());
                    }
                    continue;
                }
                _ => {}
            }
            if !active {
                continue;
            }
            self.lines_run += 1;
            match kw.as_str() {
                "endrem" => self.diag(Kind::Unbalanced, path, ln.number, "endrem".into()),
                "rem" | "beginrem" => {}
                "while" | "endwhile" | "return" => {
                    self.diag(Kind::Unsupported, path, ln.number, kw.clone());
                }
                "var" => {
                    for name in &ln.args {
                        if let Some(s) = self.scopes.last_mut() {
                            s.vars.entry(name.to_ascii_lowercase()).or_default();
                        }
                    }
                }
                "const" => {
                    // const c_x = value
                    match ln.args.as_slice() {
                        [name, eq, value] if eq == "=" => {
                            self.consts.insert(name.to_ascii_lowercase(), value.clone());
                        }
                        _ => self.diag(Kind::BadCondition, path, ln.number, format!("const {}", ln.args.join(" "))),
                    }
                }
                "run" | "include" => {
                    let Some(target) = ln.args.first() else {
                        self.diag(Kind::MissingScript, path, ln.number, kw.clone());
                        continue;
                    };
                    let run_args = self.substitute(&ln.args[1..]);
                    match self.find(&dir, target) {
                        Some((p, b)) => self.exec_script(&p, &b, &run_args, kw == "run"),
                        None => self.diag(Kind::MissingScript, path, ln.number, format!("{kw} {target}")),
                    }
                }
                _ => self.command(&ln, path),
            }
        }
        if !conds.is_empty() {
            self.diag(Kind::Unbalanced, path, 0, format!("{} if left open", conds.len()));
        }
        if new_scope {
            self.scopes.pop();
        } else if let Some(top) = self.scopes.last_mut() {
            for (k, old) in saved {
                match old {
                    Some(v) => top.vars.insert(k, v),
                    None => top.vars.remove(&k),
                };
            }
        }
        self.depth -= 1;
    }

    fn command(&mut self, ln: &lexer::Line, path: &str) {
        if ln.command.chars().any(|c| c.is_control()) {
            // binary junk (a file saved as UTF-16, a corrupt archive entry)
            self.diag(Kind::StrayLine, path, ln.number, ln.command.escape_debug().to_string());
            return;
        }
        let (namespace, method) = match ln.command.split_once('.') {
            Some((ns, m)) if !ns.is_empty() && !m.is_empty() => (ns.to_ascii_lowercase(), m.to_ascii_lowercase()),
            _ if ln.command.eq_ignore_ascii_case("echo") => (String::new(), "echo".to_string()),
            _ => {
                self.diag(Kind::StrayLine, path, ln.number, format!("{} {}", ln.command, ln.args.join(" ")));
                return;
            }
        };
        // `cmd args -> v_result`
        let (args, target) = match ln.args.iter().position(|a| a == "->") {
            Some(i) => (&ln.args[..i], ln.args.get(i + 1).cloned()),
            None => (&ln.args[..], None),
        };
        let args = self.substitute(args);
        let cmd = Command { namespace, method, raw: &ln.command, args, file: path, line: ln.number };
        match self.host.exec(&cmd) {
            Outcome::Done(value) => {
                if let (Some(t), Some(v)) = (target, value)
                    && let Some(s) = self.scopes.last_mut()
                {
                    s.vars.insert(t.to_ascii_lowercase(), v);
                }
            }
            Outcome::Unknown => self.diag(Kind::UnknownCommand, path, ln.number, ln.command.clone()),
            Outcome::Error(e) => self.diag(Kind::CommandError, path, ln.number, format!("{}: {e}", ln.command)),
        }
    }

    /// Diagnostics counted by kind.
    pub fn summary(&self) -> Vec<(Kind, usize)> {
        let mut m: HashMap<Kind, usize> = HashMap::new();
        for d in &self.diagnostics {
            *m.entry(d.kind).or_default() += 1;
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort();
        v
    }
}
