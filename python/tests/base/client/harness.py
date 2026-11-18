"""The release client on its own Xvfb display, driven with xdotool: what the client tests share."""

import fcntl
import os
import shutil
import subprocess
import time
from pathlib import Path

import pytest

from tests.layers import GAME, ROOT, install_view

BIN = ROOT / "target/release/bytefield"

pytestmark = pytest.mark.skipif(
    not (BIN.exists() and (GAME / "Mods/bf1942").exists() and shutil.which("Xvfb") and shutil.which("xdotool")),
    reason="needs the release client, the game's data, Xvfb and xdotool",
)


# A client with a level loaded takes ~1.3 GB of video memory; eight side by side (pytest's eight workers) exhaust an 8 GB card and
# wgpu quits with «Out of Memory». The slots are lock files shared by every worker: at most this many clients at once.
SLOT_MIB = 1600
SLOTS_DIR = ROOT / "target/client-test-slots"


def slot_count():
    if n := os.environ.get("BYTEFIELD_CLIENT_SLOTS"):
        return max(1, int(n))
    try:
        total = subprocess.run(["nvidia-smi", "--query-gpu=memory.total", "--format=csv,noheader,nounits"], capture_output=True, text=True, timeout=10).stdout.split()
        return max(1, int(total[0]) // SLOT_MIB) if total else 4
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return 4


def take_slot(timeout=900):
    """Blocks until one of the slots is free; the open file holds it until it is closed."""
    SLOTS_DIR.mkdir(parents=True, exist_ok=True)
    n = slot_count()
    end = time.time() + timeout
    while time.time() < end:
        for i in range(n):
            f = open(SLOTS_DIR / f"{i}.lock", "w")
            try:
                fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
                return f
            except BlockingIOError:
                f.close()
        time.sleep(0.5)
    raise AssertionError(f"no free client slot of {n} in {timeout} s")


class Client:
    """The client on its own display. Coordinates are the menu's 800x600, as the original's."""

    def __init__(self, tmp, args, env=None, config=None):
        self.tmp = Path(tmp)
        self.state = self.tmp / "state.txt"
        self.config = self.tmp / "settings.controls"
        if config is not None:
            self.config.write_text(config)
        self.slot = take_slot()
        r, w = os.pipe()
        self.xvfb = subprocess.Popen(["Xvfb", "-displayfd", str(w), "-screen", "0", "1280x720x24"], pass_fds=[w], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        os.close(w)
        self.display = ":" + os.read(r, 16).decode().strip()
        os.close(r)
        self.env = {**os.environ, "DISPLAY": self.display, "BYTEFIELD_CONFIG": str(self.tmp / "settings.cfg"), "BYTEFIELD_STATE": str(self.state), "LD_LIBRARY_PATH": str(ROOT / "data/runtime-libs/root/usr/lib/x86_64-linux-gnu"), **(env or {})}
        # (no pointer acceleration: a pixel is a pixel)
        subprocess.run(["xset", "m", "1", "1"], env=self.env, check=False)
        self.log = open(self.tmp / "client.log", "w")
        self.client = subprocess.Popen([str(BIN), str(install_view()), *args], cwd=ROOT, env=self.env, stdout=self.log, stderr=subprocess.STDOUT)

    def focus(self):
        """Without a window manager nobody gives the window the keyboard."""
        end = time.time() + 60
        while time.time() < end:
            found = subprocess.run(["xdotool", "search", "--name", "."], env=self.env, capture_output=True, text=True).stdout.split()
            if found:
                self.xdo("windowfocus", found[-1])
                return
            time.sleep(0.5)
        raise AssertionError("no window")

    def close(self):
        self.client.kill()
        self.client.wait()
        self.xvfb.kill()
        self.xvfb.wait()
        self.log.close()
        self.slot.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    def xdo(self, *args):
        subprocess.run(["xdotool", *map(str, args)], env=self.env, check=True)

    def click(self, x, y):
        self.xdo("mousemove", round(160 + x * 1.2), round(y * 1.2))
        time.sleep(0.3)
        self.xdo("mousedown", 1)
        time.sleep(0.15)
        self.xdo("mouseup", 1)
        time.sleep(0.5)

    def key(self, name):
        self.xdo("key", "--delay", 120, name)
        time.sleep(0.4)

    def ticks(self):
        rows = []
        if self.state.exists():
            for line in self.state.read_text().splitlines()[:-1]:
                rows.append(dict(w.split("=", 1) for w in line.split()))
        return rows

    def wait_tick(self, n, timeout=120):
        end = time.time() + timeout
        while time.time() < end:
            rows = self.ticks()
            if rows and int(rows[-1]["tick"]) >= n:
                return rows[-1]
            assert self.client.poll() is None, (self.tmp / "client.log").read_text()[-2000:]
            time.sleep(0.2)
        raise AssertionError(f"no tick {n}: {(self.tmp / 'client.log').read_text()[-2000:]}")

    def during(self, what):
        """The ticks while `what` is done, once the device has reached the game."""
        start = int(self.ticks()[-1]["tick"])
        what()
        time.sleep(0.3)
        return [r for r in self.ticks() if int(r["tick"]) > start]

    def hold(self, key, seconds=1.0):
        def go():
            self.xdo("keydown", key)
            time.sleep(seconds)
            self.xdo("keyup", key)

        return self.during(go)

    def move(self, dx, dy):
        return self.during(lambda: self.xdo("mousemove_relative", "--", dx, dy))


