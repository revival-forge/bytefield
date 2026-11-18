"""The briefing after a level loads, through the devices: READY or Enter takes it away, without also saying «done choosing» on the spawn
screen under it; the soldier only appears after that second answer (`BYTEFIELD_STATE` has no line until there is one). About 2 minutes."""
import time

from tests.base.client.harness import Client, pytestmark  # noqa: F401

READY = (390, 437)


def start(tmp):
    c = Client(tmp, ["bf1942", "El_Alamein", "--team", "2"])
    c.focus()
    end = time.time() + 120
    while "mode conquest" not in (c.tmp / "client.log").read_text():
        assert time.time() < end and c.client.poll() is None, (c.tmp / "client.log").read_text()[-2000:]
        time.sleep(0.5)
    time.sleep(10)
    return c


def nobody(c, seconds=20):
    """Longer than the spawn's wait: had it been let go, there would be a soldier."""
    time.sleep(seconds)
    assert not c.ticks(), c.ticks()[:1]


def test_enter_is_ready_and_then_done(tmp_path):
    with start(tmp_path) as c:
        nobody(c, 5)
        c.key("Return")
        nobody(c)
        c.key("Return")
        c.wait_tick(1, timeout=60)


def test_a_click_off_the_button_does_nothing(tmp_path):
    with start(tmp_path) as c:
        c.click(390, 300)
        c.key("Return")
        nobody(c)
        c.key("Return")
        c.wait_tick(1, timeout=60)


def test_the_button_itself(tmp_path):
    with start(tmp_path) as c:
        c.click(*READY)
        nobody(c)
        c.key("Return")
        c.wait_tick(1, timeout=60)
