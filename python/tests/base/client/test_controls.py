"""The controls through the devices, as a player's: real clicks and keys (xdotool) on our client in an Xvfb, and what reached
the simulation read back from `BYTEFIELD_STATE`. Skipped without the data, the release build, Xvfb or xdotool. About 3 minutes.
"""
import time

import pytest

from tests.base.client.harness import BIN, GAME, Client, pytestmark  # noqa: F401


def values(rows, name):
    return [float(r[name]) for r in rows]


def play(tmp, board=None, config=None, team="2"):
    env = {"BYTEFIELD_BOARD_TICK": "60", "BYTEFIELD_BOARD": board} if board else {}
    c = Client(tmp, ["bf1942", "El_Alamein", "--kit", "1", "--team", team], env=env, config=config)
    c.focus()
    c.wait_tick(90, timeout=240)
    time.sleep(0.5)
    # (the left button takes the mouse, as a player's first click)
    c.click(400, 300)
    return c


# The places of the page (`controls_page`), in the menu's 800x600
OPTIONS, CONTROLS, SAVE = (168, 19), (492, 45), (492, 71)
TAB_INFANTRY, TAB_AIR, TAB_LAND = (157, 137), (245, 137), (356, 137)


def cell(row, column=0):
    return (27 + 190 + 45 + 100 * column, 125 + 35 + 25 * row + 9)


def slider(tenths, y=40):
    return (450 + 218 + 76 / 11 * (tenths + 0.5), 125 + y + 6)


def test_the_page_defines_and_saves(tmp_path):
    with Client(tmp_path, ["--no-intro", "--lang", "en", "--steps", "20:options;30:controls"]) as c:
        c.focus()
        time.sleep(12)
        c.click(*TAB_INFANTRY)
        c.click(*cell(9))  # RELOAD
        c.key("q")
        c.click(*cell(0, 1))  # FORWARD's second
        c.key("Escape")  # left as it was, and the page stays
        c.click(*slider(8))
        c.click(450 + 218 + 10, 125 + 68 + 6)  # INVERT MOUSE
        assert not c.config.exists(), "only SAVE writes"
        c.click(*SAVE)
        c.click(*TAB_LAND)
        time.sleep(1)
        text = c.config.read_text()
    assert "infantry.CONTROLS_INFANTRY_RELOAD=Q\n" in text
    assert "infantry.CONTROLS_INFANTRY_FORWARD=W\n" in text
    assert "infantry.mouse=0.8\n" in text
    assert "infantry.invert=1\n" in text
    assert "land.mouse=0.25\n" in text


def test_esc_on_a_key_with_the_round_under_it(tmp_path):
    with play(tmp_path) as c:
        c.key("Escape")
        time.sleep(1)
        c.click(*OPTIONS)
        c.click(*CONTROLS)
        c.click(*TAB_INFANTRY)
        c.click(*cell(9))
        c.key("Escape")
        c.click(*cell(9))
        c.key("q")
        c.click(*SAVE)  # (the menu is still up)
        time.sleep(1)
        assert "infantry.CONTROLS_INFANTRY_RELOAD=Q\n" in c.config.read_text()


@pytest.mark.parametrize("sens,gain", [("0.25", 1.0), ("0.5", 2.0)])
def test_the_soldiers_mouse(tmp_path, sens, gain):
    with play(tmp_path, config=f"version=2\ninfantry.mouse={sens}\n") as c:
        rows = c.move(100, 0)
    # (100 of xdotool's units, two pixels each here as in the original's tests; `MOUSE_UNITS` each at 0.25)
    assert sum(values(rows, "mouse_x")) == pytest.approx(4.0 * gain, abs=0.3)


def test_a_whole_turn_as_the_originals(tmp_path):
    """The original's soldier turns round in 3000 of these units at 0.25; a unit of the axis is 3°."""
    with play(tmp_path, config="version=2\ninfantry.mouse=0.25\n") as c:
        # (in steps: a tick's axis stops at ±16)
        turned = sum(sum(values(c.move(100, 0), "mouse_x")) for _ in range(10))
    assert turned * 3 == pytest.approx(120, abs=3)


def test_inverted_on_foot(tmp_path):
    with play(tmp_path, config="version=2\ninfantry.invert=1\n") as c:
        rows = c.move(0, 100)
    # (on foot the original turns up and down 0.84 of sideways, `FOOT_PITCH_GAIN` in play.rs)
    assert sum(values(rows, "mouse_y")) == pytest.approx(-4.0 * 0.84, abs=0.15)


def test_a_key_defined_in_a_tank(tmp_path):
    with play(tmp_path, board="PanzerIV", config="version=2\nland.CONTROLS_LANDSEA_SPEEDUP=Q\nland.keyboard=0.25\n") as c:
        assert c.ticks()[-1]["map"] == "Land"
        assert max(values(c.hold("w"), "throttle")) == 0
        assert max(values(c.hold("q"), "throttle")) == 1
        # (half the sensitivity by default: half the axis)
        assert max(values(c.hold("d"), "yaw_axis")) == pytest.approx(0.5, abs=0.02)
        # (↑ is `DIVE / HATCH DOWN`, the axis's plus; the same sensitivity)
        assert max(values(c.hold("Up"), "pitch_axis")) == pytest.approx(0.5, abs=0.02)


@pytest.mark.parametrize("invert,forward", [("1", 1), ("0", -1)])
def test_the_pilots_mouse(tmp_path, invert, forward):
    with play(tmp_path, board="Bf109", config=f"version=2\nair.invert={invert}\nair.mouse=0.5\n") as c:
        assert c.ticks()[-1]["map"] == "Air"
        # (forward, with `setAirMouseInvert 1`, dives as ↑ does)
        pitch = values(c.move(0, -20), "pitch_axis")
        assert max(pitch, key=abs) * forward > 0
        roll = values(c.move(20, 0), "roll_axis")
        assert max(roll, key=abs) > 0
        assert max(values(c.hold("Up"), "pitch_axis")) == 1
        assert min(values(c.hold("Left"), "roll_axis")) == -1
