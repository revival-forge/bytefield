"""The content layers a test runs on, and one way to ask for them (the Rust side is `crates/testdata`).

| Layer | Folders under `Mods/` | Who has it |
|---|---|---|
| (synthetic) | fixtures in the repo | everyone, also public CI |
| `base` | `bf1942` | whoever owns the game |
| `xpack1`, `xpack2` | `XPack1`, `XPack2` | whoever owns the expansions |
| `dc`, `dc07` | `DC_Final`, `DesertCombat` | whoever installed Desert Combat |
| `gc` | `GCMOD` | whoever installed Galactic Conquest |
| `ref` | `DC_Extended`, `GC_Redux` | reference mods: almost nobody |
| `hd` | the packs of `BYTEFIELD_HD` | whoever installed one |

A test says what it needs by where it lives (`tests/<layer>/...`, see `conftest.py`), with the mark
`@pytest.mark.layer("xpack1", ...)`, or at run time with `require("dc")` / `require_mod("DC_Final")`.

`BYTEFIELD_LAYERS` picks the layers even when more are installed: a comma list of the names above, `xpack` for
both expansions, or `all`. Unset, every installed layer but `ref` and `hd`. `BYTEFIELD_GAME` points at another
install (`data/game` by default).
"""

from __future__ import annotations

import os
from pathlib import Path

import pytest

TESTS = Path(__file__).resolve().parent
PYTHON = TESTS.parent
ROOT = PYTHON.parent
TOOLS = PYTHON / "tools"
FIXTURES = TESTS / "fixtures"
GAME = Path(os.environ.get("BYTEFIELD_GAME") or ROOT / "data" / "game")

MODS = {
    "base": ["bf1942"],
    "xpack1": ["XPack1"],
    "xpack2": ["XPack2"],
    "dc": ["DC_Final"],
    "dc07": ["DesertCombat"],
    "gc": ["GCMOD"],
    "ref": ["DC_Extended", "GC_Redux"],
    "hd": [],
}
# the layers that are not content (no mod folder; not in the install view): their folder
SIDE = {
}
ALL = list(MODS) + list(SIDE)
OFF_BY_DEFAULT = {"ref", "hd"}


def parse(value: str) -> set[str]:
    out: set[str] = set()
    for name in (n.strip().lower() for n in value.split(",")):
        if name == "all":
            out |= set(ALL)
        elif name == "xpack":
            out |= {"xpack1", "xpack2"}
        elif name in ALL:
            out.add(name)
    return out


def wanted(layer: str) -> bool:
    value = os.environ.get("BYTEFIELD_LAYERS")
    if value is None:
        return layer not in OFF_BY_DEFAULT
    # a side layer is not content: it comes along unless the list leaves out the base game
    return layer in parse(value) or (layer in SIDE and "base" in parse(value))


def installed(layer: str) -> bool:
    if layer == "hd":
        return any(Path(p).is_dir() for p in os.environ.get("BYTEFIELD_HD", "").split(os.pathsep) if p)
    if layer in SIDE:
        return SIDE[layer].is_dir()
    mods = GAME / "Mods"
    base = layer == "base" or (mods / "bf1942").is_dir()
    return base and all((mods / m).is_dir() for m in MODS[layer])


def available(layer: str) -> bool:
    return wanted(layer) and installed(layer)


def layer_of_mod(mod: str) -> str | None:
    return next((layer for layer, mods in MODS.items() if any(m.lower() == mod.lower() for m in mods)), None)


def missing(*layers: str) -> str | None:
    """The first of `layers` that is not available, if any."""
    return next((layer for layer in layers if not available(layer)), None)


def require(*layers: str) -> Path:
    """The install, or skip the test when a layer is not available."""
    if (layer := missing(*layers)) is not None:
        pytest.skip(f"needs layer {layer}")
    return GAME


def require_mod(mod: str) -> Path:
    """The install, or skip the test when the mod's layer is not available (a mod outside the layers: its folder)."""
    layer = layer_of_mod(mod)
    if layer is None:
        if not (GAME / "Mods" / mod).is_dir():
            pytest.skip(f"needs mod {mod}")
        return GAME
    return require("base", layer)


def install_view() -> Path:
    """The install as a machine with only the available layers would have it (`crates/testdata`'s `install_view`): a
    folder under `target/testdata/` linking to the install's entries, with a `Mods/` of the available layers' mods and
    those outside the layers. For what scans `Mods/` (the client's menu): it never reaches a reference mod on another
    disk. The same folder as the Rust tests' for the same layers. The engine's cache beside the install (`<game>/../out`)
    is linked beside the view (`target/testdata/out`), so levels load from it as from the install."""
    keep = [m for m in (GAME / "Mods").iterdir() if (layer := layer_of_mod(m.name)) is None or available(layer)]
    tag = "-".join(sorted(layer for layer in MODS if available(layer))) or "none"
    view = ROOT / "target" / "testdata" / tag
    out = GAME.resolve().parent / "out"
    if out.is_dir():
        view.parent.mkdir(parents=True, exist_ok=True)
        try:
            (view.parent / "out").symlink_to(out)
        except FileExistsError:
            pass
    if (view / "Mods").is_dir():
        return view
    tmp = view.with_name(f"{tag}.tmp{os.getpid()}")
    (tmp / "Mods").mkdir(parents=True)
    real = GAME.resolve()
    for e in real.iterdir():
        if e.name != "Mods":
            (tmp / e.name).symlink_to(e)
    for m in keep:
        (tmp / "Mods" / m.name).symlink_to(real / "Mods" / m.name)
    try:
        tmp.rename(view)
    except OSError:
        import shutil
        shutil.rmtree(tmp, ignore_errors=True)
    return view
