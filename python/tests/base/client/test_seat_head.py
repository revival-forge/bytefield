"""The driver's head and the mouse, through the devices: the same gesture as on the original (a big turn, twice that back, a big turn again) ends near the middle, turned a little the first way, and it stays with the mouse still. About 1 minute."""
import time

from tests.base.client.harness import Client, pytestmark  # noqa: F401


def test_the_gesture(tmp_path):
    with Client(tmp_path, ["bf1942", "El_Alamein", "--team", "1", "--kit", "1", "--no-intro"], env={"BYTEFIELD_BOARD": "Kubelwagen", "BYTEFIELD_BOARD_TICK": "60"}) as c:
        c.focus()
        c.wait_tick(100)
        c.click(400, 300)
        assert c.ticks()[-1]["vehicle"] == "Kubelwagen"

        def turn(total):
            for _ in range(20):
                c.xdo("mousemove_relative", "--", total // 20, 0)
                time.sleep(0.03)
            time.sleep(0.5)
            return float(c.ticks()[-1]["seat_head"])

        assert turn(-300) == 18
        time.sleep(1)
        assert float(c.ticks()[-1]["seat_head"]) == 18
        assert turn(600) == -18
        assert 3 < turn(-300) < 9
