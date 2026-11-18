"""Esc on the loading screen, through the devices: before the level starts loading it stays on the menu, and pressed while the level loads (one frame,
deaf to the keys) it takes the round down when that frame ends. Both end on CREATE GAME, as the original's. About 1 minute."""
import subprocess
import time

from tests.base.client.harness import Client, pytestmark  # noqa: F401

START = ["--no-intro", "--steps", "20:multiplayer;25:create;30:start"]


def shot(c, name):
    path = c.tmp / name
    subprocess.run(["import", "-window", "root", str(path)], env=c.env, check=True)
    return path


def wait_log(c, text, timeout=90, every=0.02):
    end = time.time() + timeout
    while text not in (c.tmp / "client.log").read_text():
        assert time.time() < end and c.client.poll() is None, (c.tmp / "client.log").read_text()[-2000:]
        time.sleep(every)


def same(a, b, c):
    """How far two captures are (0 the same)."""
    out = subprocess.run(["compare", "-metric", "RMSE", str(a), str(b), "null:"], env=c.env, capture_output=True, text=True).stderr
    return float(out.split("(")[1].split(")")[0])


def test_before_the_level_loads(tmp_path):
    with Client(tmp_path, START, env={"BYTEFIELD_LOAD_FRAMES": "100000"}) as c:
        c.focus()
        time.sleep(15)
        loading = shot(c, "loading.png")
        c.key("Escape")
        time.sleep(3)
        back = shot(c, "back.png")
        assert same(loading, back, c) > 0.02, "still on the loading screen"
        assert "vehicles; the player" not in (c.tmp / "client.log").read_text()


def test_while_the_level_loads(tmp_path):
    with Client(tmp_path, START) as c:
        c.focus()
        # (a line from the middle of the load, about 0.6 s before it ends: `launch`)
        wait_log(c, "textures: alternative paths")
        c.xdo("key", "--delay", 40, "Escape")
        log = c.tmp / "client.log"
        wait_log(c, "vehicles; the player")
        time.sleep(8)
        assert "vehicles; the player" in log.read_text() and c.client.poll() is None, log.read_text()[-2000:]
        # (no round behind: the status line is the round's; the capture is to look at, the menu's background moves)
        shot(c, "back.png")
        lines = log.read_text()
        assert "state:" not in lines, lines[-1500:]
        # (and the menu still starts a round after that: START)
        c.click(718, 562)
        wait_log(c, "state:", every=0.5)
