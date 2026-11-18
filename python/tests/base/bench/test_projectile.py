"""Projectiles in the test bench: templates, firing, flight, hits. Uses the real
data (skipped without it)."""

from __future__ import annotations


import pytest

bytefield = pytest.importorskip("bytefield")

from tests.layers import GAME  # noqa: E402
T34_AT = (1805.18, 39.62, 1924.71)  # a T34 of Berlin
IDENTITY = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))


@pytest.fixture(scope="module")
def berlin():
    return bytefield.World(str(GAME), "bf1942", "Berlin")


def test_templates_read_like_the_engine(berlin):
    t = berlin.projectile_template("T34Projectile")
    assert t["point_physics"] and t["has_collision_effect"] and t["die_after_coll"]
    assert t["time_to_live"] == {"kind": 0, "a": 10.0, "b": 0.0}
    assert t["drag"] == 0.0 and t["mass"] == 1.0 and t["material"] == 238
    g = berlin.projectile_template("GrenadeAxisProjectile")
    assert not g["point_physics"] and not g["die_after_coll"] and g["has_on_time_effect"]
    w = berlin.weapon("T34GunBarrel")
    assert w["projectile"] == "T34Projectile" and w["velocity"] == 100.0 and w["add_root_speed"]


def test_a_shell_flies_and_ends_where_it_hits(berlin):
    s = berlin.sim(dt=1 / 30)
    x, y, z = T34_AT
    down = ((1.0, 0.0, 0.0), (0.0, 0.98, 0.17), (0.0, -0.17, 0.98))
    p = s.fire("T34GunBarrel", down, (x, y + 3, z), root_vel=(0.0, 0.0, 0.0))
    st = p.state()
    assert st["vel"][2] == pytest.approx(98.0) and st["vel"][1] == pytest.approx(-17 + 0.982, abs=1e-3)
    s.step(60)
    st = p.state()
    assert not p.alive and st["end"] == {"Hit": {"detonated": True}}
    hit = st["impacts"][-1]
    assert hit["object"] is None and hit["ended"]
    h = berlin.height_at(hit["point"][0], hit["point"][2])
    assert hit["point"][1] == pytest.approx(h, abs=1e-3)


def test_terrain_segment_meets_the_surface(berlin):
    x, _, z = T34_AT
    h = berlin.height_at(x, z)
    p, n = berlin.terrain_segment((x, h + 5, z), (0.3, -10, 0.2))
    assert p[1] == pytest.approx(berlin.height_at(p[0], p[2]), abs=1e-3)
    assert n[1] > 0.5
    assert berlin.terrain_segment((x, h + 5, z), (0.3, -2, 0.2)) is None  # stops short


def test_time_to_live(berlin):
    s = berlin.sim(dt=1 / 30)
    x, y, z = T34_AT
    up = ((1.0, 0.0, 0.0), (0.0, 0.0, -1.0), (0.0, 1.0, 0.0))
    p = s.fire("T34GunBarrel", up, (x, y + 3, z))
    s.step(310)
    st = p.state()
    assert st["end"] == {"Expired": {"detonated": False}}
    assert st["age"] == pytest.approx(10.0, abs=0.05)


def test_a_grenade_bounces_until_its_time_runs_out(berlin):
    s = berlin.sim(dt=1 / 30)
    x, y, z = T34_AT
    g = s.add_projectile("GrenadeAxisProjectile", (x, y + 2, z + 4), (0.0, 3.0, 4.0))
    s.step(60)
    st = g.state()
    assert g.alive and st["impacts"]
    s.step(40)
    assert g.state()["end"] == {"Expired": {"detonated": True}}
