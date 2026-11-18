"""Content layers for the whole suite (`tests/layers.py`).

The folder a test lives in says the layers it needs: any path part under `tests/` that names a layer (`base`,
`dc`, `gc`, `xpack1`...) adds it (and the expansions and mods add `base`). `synthetic` needs nothing.
On top, `@pytest.mark.layer("xpack1", ...)` adds more. A test whose layers are
not all available is skipped with the reason `needs layer <name>`.

Tests under a `client` folder drive the release client on Xvfb (seconds to a minute each) and are marked `slow`:
`scripts/tests quick` leaves them out (`-m "not slow"`), `full` runs them.
"""

from __future__ import annotations

import pytest

from tests import layers


def pytest_configure(config):
    config.addinivalue_line("markers", "layer(*names): the content layers the test needs (tests/layers.py)")
    config.addinivalue_line("markers", "slow: out of the quick run (scripts/tests quick); the client tests are, by their folder")


def layers_of_path(parts: tuple[str, ...]) -> list[str]:
    out = [p for p in parts if p in layers.MODS]
    if {"xpack1", "xpack2", "dc", "dc07", "gc", "ref"} & set(out) and "base" not in out:
        out.insert(0, "base")
    return out


def pytest_collection_modifyitems(config, items):
    for item in items:
        try:
            parts = item.path.resolve().relative_to(layers.TESTS).parts[:-1]
        except ValueError:
            continue
        if "client" in parts:
            item.add_marker(pytest.mark.slow)
        need = layers_of_path(parts)
        for mark in item.iter_markers("layer"):
            need.extend(mark.args)
        if (layer := layers.missing(*need)) is not None:
            item.add_marker(pytest.mark.skip(reason=f"needs layer {layer}"))
