"""Static scan of Refractor 1 `.con` scripts: what commands, types and properties a game or mod uses.

This is not the interpreter (that is `crates/con-lang`). It reads every file on its own, without
following `run`/`include`, and counts. Its output is the "contract" the interpreter has to cover,
and later a second, independent implementation to diff the Rust one against.

Dialect, as seen in BF1942 and its mods:
- one command per line: `Namespace.method arg arg ...`, or a keyword (`run`, `include`, `if`, ...)
- commands and keywords are case-insensitive
- `rem ...` comments a line; `beginrem` ... `endrem` comments a block
- arguments are split on blanks (space, tab, VT, FF); `"..."` keeps spaces
- `X.create <Type> <name>` makes <name> the active X; later `X.<prop>` lines apply to it, and
  `X.active <name>` switches back to an earlier one
- `var v_x`, `const c_x = 1`, `cmd -> v_x` (store a result), `if a == b` / `else` / `endif`,
  `run file arg...` (arguments are `v_arg1`..), `include file` (same scope)
"""

from __future__ import annotations

import re
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

KEYWORDS = {
    "run", "include", "echo", "if", "elseif", "else", "endif", "while", "endwhile",
    "var", "const", "return", "beginrem", "endrem", "rem",
}
SCRIPT_SUFFIXES = {".con", ".inc"}
# these `create`s name an instance (`aiStrategicArea.create <name>`, `Object.create <template>`), not a type
CREATE_WITHOUT_TYPE = {"aistrategicarea", "object"}  # Object.create <template> makes an instance
# CRLF, a lone CR and a lone LF each end a line; blanks are ASCII only (the Rust lexer agrees)
_LINE_END = re.compile(r"\r\n|\r|\n")
_BLANK = " \t\v\f"
_REM = re.compile(r"rem(?![A-Za-z0-9_.])", re.IGNORECASE)
_INT = re.compile(r"[+-]?\d+")
_FLOAT = re.compile(r"[+-]?(\d+\.\d*|\.\d+|\d+)([eE][+-]?\d+)?")


@dataclass
class Line:
    path: str        # relative to the scanned root, '/' separators
    number: int      # 1-based
    command: str     # as written
    args: list[str]


def split_args(text: str) -> list[str]:
    """Split on whitespace, keeping "quoted strings" (quotes removed) as one argument."""
    out: list[str] = []
    for m in re.finditer(r'"([^"]*)"?|([^ \t\v\f]+)', text):
        out.append(m.group(1) if m.group(1) is not None else m.group(2))
    return out


def iter_lines(text: str, path: str = ""):
    """Yield the command lines of one script, with comments and blank lines removed."""
    in_block = False
    for number, raw in enumerate(_LINE_END.split(text), start=1):
        s = raw.strip(_BLANK)
        if not s:
            continue
        low = s.lower()
        if in_block:
            if low.startswith("endrem"):
                in_block = False
            continue
        if low.startswith("beginrem"):
            in_block = True
            continue
        if _REM.match(s):
            continue
        parts = split_args(s)
        yield Line(path, number, parts[0], parts[1:])


def value_kind(arg: str) -> str:
    """A coarse shape for an argument, to learn what each property takes."""
    low = arg.lower()
    if low.startswith(("v_", "c_")):
        return "var"
    if _INT.fullmatch(arg):
        return "int"
    if _FLOAT.fullmatch(arg):
        return "float"
    if "/" in arg and all(_FLOAT.fullmatch(p) for p in arg.split("/")):
        return f"vec{arg.count('/') + 1}"
    if "/" in arg or "\\" in arg or "." in arg:
        return "path"
    return "name"


@dataclass
class Inventory:
    files: int = 0
    lines: int = 0
    # lowercase command -> spelling counts, total uses, files that use it
    spellings: dict[str, Counter] = field(default_factory=lambda: defaultdict(Counter))
    commands: Counter = field(default_factory=Counter)
    command_files: dict[str, set] = field(default_factory=lambda: defaultdict(set))
    # (namespace, type) -> number of `create`s; types are case-insensitive, keys are lowercase
    creates: Counter = field(default_factory=Counter)
    type_spellings: dict[str, Counter] = field(default_factory=lambda: defaultdict(Counter))
    # (namespace, Type) -> property -> uses
    props: dict[tuple, Counter] = field(default_factory=lambda: defaultdict(Counter))
    # lowercase command -> Counter of argument shape tuples
    shapes: dict[str, Counter] = field(default_factory=lambda: defaultdict(Counter))
    keywords: Counter = field(default_factory=Counter)
    conditions: Counter = field(default_factory=Counter)
    run_targets: int = 0
    run_resolution: Counter = field(default_factory=Counter)  # file / ancestor / root / unresolved
    run_unresolved: list = field(default_factory=list)
    stray: list = field(default_factory=list)      # lines that are neither a command nor a keyword
    no_active: Counter = field(default_factory=Counter)  # X.prop with no X created before in that file
    # (namespace, Type or "" outside a template, method) -> Counter of argument tuples
    values: dict[tuple, Counter] = field(default_factory=lambda: defaultdict(Counter))


def _resolve(root: Path, base: Path, target: str, index: set[str]) -> str | None:
    """Where does a `run`/`include` target exist on disk? Case-insensitive, and `run` may leave
    out `.con`. Returns "file" (relative to the script), "ancestor" (relative to a parent
    directory: level gametype scripts do this), "root", or None. Refractor also resolves from
    the archive roots, so None is a lead to check, not proof of a broken mod."""
    target = target.replace("\\", "/")
    names = [target] if Path(target).suffix.lower() in SCRIPT_SUFFIXES else [target + ".con", target]
    starts = [("file", base)] + [("ancestor", a) for a in base.parents if a != root and root in a.parents]
    starts.append(("root", root))
    for how, start in starts:
        if any(str((start / n).resolve()).lower() in index for n in names):
            return how
    return None


def scan(root: Path) -> Inventory:
    """Scan every .con/.inc under root."""
    root = root.resolve()
    paths = sorted(p for p in root.rglob("*") if p.is_file() and p.suffix.lower() in SCRIPT_SUFFIXES)
    index = {str(p).lower() for p in paths}
    inv = Inventory()

    # Pass 1: every template name -> its type, so `X.active name` can be attributed across files.
    template_type: dict[tuple[str, str], str] = {}
    parsed: list[tuple[Path, list[Line]]] = []
    for p in paths:
        rel = p.relative_to(root).as_posix()
        lines = list(iter_lines(p.read_bytes().decode("latin-1"), rel))
        parsed.append((p, lines))
        for ln in lines:
            ns, _, method = ln.command.lower().partition(".")
            if method == "create" and len(ln.args) >= 2:
                template_type[(ns, ln.args[1].lower())] = ln.args[0].lower()

    creatable = {ns for ns, _ in template_type}

    # Pass 2: count.
    for p, lines in parsed:
        inv.files += 1
        active: dict[str, str] = {}   # namespace -> Type of the active template in this file
        for ln in lines:
            inv.lines += 1
            low = ln.command.lower()
            if low in KEYWORDS:
                inv.keywords[low] += 1
                if low in ("if", "elseif", "while"):
                    inv.conditions[" ".join(ln.args)] += 1
                if low in ("run", "include") and ln.args:
                    inv.run_targets += 1
                    how = _resolve(root, p.parent, ln.args[0], index)
                    inv.run_resolution[how or "unresolved"] += 1
                    if how is None:
                        inv.run_unresolved.append(f"{ln.path}:{ln.number} {low} {ln.args[0]}")
                continue
            if low.startswith(("v_", "c_")) or "." not in low:
                inv.stray.append(f"{ln.path}:{ln.number} {ln.command} {' '.join(ln.args)}".rstrip())
                continue

            inv.spellings[low][ln.command] += 1
            inv.commands[low] += 1
            inv.command_files[low].add(ln.path)
            args = ln.args[:ln.args.index("->")] if "->" in ln.args else ln.args
            inv.shapes[low][tuple(value_kind(a) for a in args)] += 1

            ns, _, method = low.partition(".")
            if method == "create" and ns in CREATE_WITHOUT_TYPE:
                active[ns] = ns
            elif method == "create" and len(args) >= 2:
                inv.creates[(ns, args[0].lower())] += 1
                inv.type_spellings[args[0].lower()][args[0]] += 1
                active[ns] = args[0].lower()
            elif method in ("active", "setactive") and args:
                t = template_type.get((ns, args[0].lower()))
                if t:
                    active[ns] = t
            elif ns in active:
                inv.props[(ns, active[ns])][method] += 1
                inv.values[(ns, active[ns], method)][tuple(args)] += 1
            elif ns in creatable:
                inv.no_active[low] += 1
            else:
                inv.values[(ns, "", method)][tuple(args)] += 1
    return inv


def spelling(inv: Inventory, low: str) -> str:
    return inv.spellings[low].most_common(1)[0][0] if inv.spellings.get(low) else low


def type_spelling(inv: Inventory, low: str) -> str:
    return inv.type_spellings[low].most_common(1)[0][0] if inv.type_spellings.get(low) else low


def to_json(inv: Inventory) -> dict:
    """A stable, diffable summary (all keys lowercase, sorted)."""
    return {
        "files": inv.files,
        "lines": inv.lines,
        "commands": {k: inv.commands[k] for k in sorted(inv.commands)},
        "creates": {f"{ns}.{t}": n for (ns, t), n in sorted(inv.creates.items())},
        "props": {f"{ns}.{t}": {m: c[m] for m in sorted(c)} for (ns, t), c in sorted(inv.props.items())},
        "shapes": {k: {" ".join(sh) or "-": n for sh, n in inv.shapes[k].most_common(4)} for k in sorted(inv.shapes)},
        "keywords": {k: inv.keywords[k] for k in sorted(inv.keywords)},
        "conditions": {k: inv.conditions[k] for k in sorted(inv.conditions)},
        "run_targets": inv.run_targets,
        "run_resolution": dict(sorted(inv.run_resolution.items())),
        "stray": len(inv.stray),
    }
