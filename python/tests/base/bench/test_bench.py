"""The headless test bench through its Python API (`import bytefield`, built from crates/py).

    cd python && uv run maturin develop --release -m ../crates/py/Cargo.toml && uv run pytest
"""

import math

import pytest

bytefield = pytest.importorskip("bytefield")
from tests.layers import GAME  # noqa: E402


@pytest.fixture(scope="module")
def alamein():
    return bytefield.World(str(GAME), "bf1942", "El_Alamein")


@pytest.fixture(scope="module")
def midway():
    return bytefield.World(str(GAME), "bf1942", "Midway")


def test_world(alamein):
    assert alamein.world_size == 2048
    assert alamein.height_at(1000, 1000) > alamein.water_level
    assert alamein.height_at(-5, 10) is None
    assert len(alamein.instances()) > 900


def reference_fall(h0, dt, g=14.73):
    """The engine's point-node scheme, written again in plain Python: gravity enters
    the accumulator after each step, 4 substeps of dt/4."""
    y, v, a, t = h0, 0.0, 0.0, 0.0
    while True:
        y_prev = y
        for _ in range(4):
            v += a * dt / 4
            y += v * dt / 4
        t += dt
        a = -g
        if y <= 0:
            return t - dt * (1 - y_prev / (y_prev - y))


def test_gravity_is_the_engines():
    assert bytefield.GRAVITY == 14.73


def test_free_fall_time(alamein):
    """100 m drop, against the reference scheme, and close to sqrt(2h/g) + one step of lag."""
    x, z = 1000.0, 1000.0
    s = alamein.sim(dt=1 / 60)
    i = s.spawn((x, alamein.height_at(x, z) + 100, z), (0, 0, 0))
    s.run(30, i)
    kind, t = s.contact(i)
    assert kind == "Ground"
    assert abs(t - reference_fall(100.0, 1 / 60)) < 0.02  # terrain is not exactly flat
    assert abs(t - (math.sqrt(2 * 100 / bytefield.GRAVITY) + 1 / 60)) < 0.03


def test_linear_drag_terminal_velocity(alamein):
    """With linear drag k = pi r² drag / m, a falling body tends to v = g / k."""
    s = alamein.sim(dt=1 / 60)
    drag, mass, radius = 2.0, 1.0, 0.3
    k = math.pi * radius ** 2 * drag / mass
    i = s.spawn((1000.0, 2000.0, 1000.0), (0, 0, 0), drag=drag, mass=mass, radius=radius)
    s.step(60 * 20)
    assert abs(s.state(i)[1][1] + bytefield.GRAVITY / k) < 0.05


def test_deterministic(alamein):
    def shot():
        s = alamein.sim(dt=1 / 60)
        i = s.spawn((900.0, 80.0, 900.0), (120.0, 60.0, 35.0), gravity_modifier=0.8, drag=0.3, radius=0.2)
        return s.run(60, i), s.contact(i)

    assert shot() == shot()


def test_drag_shortens_range(alamein):
    def landing(drag):
        s = alamein.sim()
        i = s.spawn((500.0, alamein.height_at(500, 1000) + 1, 1000.0), (80.0, 80.0, 0.0), drag=drag, radius=0.3)
        s.run(60, i)
        return s.state(i)[0][0]

    assert landing(0.5) < landing(0.0)




def drive(world, spawns, steps):
    """Spawn (template, pos, rotation, inputs) vehicles in one sim, step it; their final bodies."""
    s = world.sim(dt=1 / 30)
    vs = []
    for template, pos, rot, inputs in spawns:
        v = s.add_vehicle(template, pos, rotation=rot)
        for k, x in inputs.items():
            v.set_input(k, x)
        vs.append(v)
    s.step(steps)
    return [v.body() for v in vs]


def test_vehicles_share_a_sim(alamein):
    """Two vehicles far apart in one sim move exactly as each does alone (the phases interleave,
    but nothing yet couples them), and a driven Willy goes somewhere."""
    def at(x, z, lift=1.0):
        return (x, alamein.height_at(x, z) + lift, z)
    willy = ("Willy", at(1000, 1000), (30, 0, 0), {"c_PIThrottle": 1.0})
    t34 = ("T34", at(1200, 900), (0, 0, 0), {"c_PIThrottle": 1.0, "c_PIYaw": 0.5})
    both = drive(alamein, [willy, t34], 120)
    alone = [drive(alamein, [v], 120)[0] for v in (willy, t34)]
    for b, a in zip(both, alone):
        for k in ("pos", "vel", "rotvel", "rows"):
            assert b[k] == a[k], k
    assert math.dist(both[0]["pos"], willy[1]) > 10
    assert all(math.isfinite(x) for b in both for x in b["pos"])


def test_a_t34_rams_a_parked_t34(alamein):
    """Two movers collide: a T34 at full throttle runs into a parked one 45 m ahead,
    on flat sand. The hulls' vertices against each other's faces push them apart by their
    masses: the parked one (asleep by then) wakes and is shoved forward, the momentum is shared
    (equal masses), and the hulls never go through each other."""
    def at(x, z, lift=0.5):
        return (x, alamein.height_at(x, z) + lift, z)
    s = alamein.sim(dt=1 / 30)
    a = s.add_vehicle("T34", at(325, 1100))
    b = s.add_vehicle("T34", at(325, 1145))
    b.set_engines_on(False)
    a.set_input("c_PIThrottle", 1.0)
    start_b = b.body()["pos"]
    closest, first, asleep_then = 1e9, None, None
    for n in range(300):
        before = (a.body()["vel"][2], b.body()["vel"][2])
        s.step()
        pa, pb = a.body()["pos"], b.body()["pos"]
        closest = min(closest, math.dist(pa, pb))
        hit = any(i["with"] == "T34" for c in b.contacts() for i in c["impulses"])
        if hit and first is None:
            first, asleep_then = n, before
        if first is not None and n == first + 10:
            after = (a.body()["vel"][2], b.body()["vel"][2])
        assert pa[2] < pb[2]
    assert first is not None and asleep_then[1] == 0.0
    assert asleep_then[0] > 8  # it came fast
    assert after[1] > 3 and after[0] < asleep_then[0] - 3  # shoved / slowed down
    assert abs(sum(after) - asleep_then[0]) < 0.15 * asleep_then[0]  # momentum, roughly
    assert closest > 5.5, closest  # a T34 is 6.6 m long: nose to tail, not through
    assert b.body()["pos"][2] - start_b[2] > 3.0


def test_two_t34s_head_on_stop_each_other(alamein):
    """Head-on at full throttle: they meet halfway and stop each other; the momentum along the
    street (zero, by symmetry) stays near zero through the crash."""
    def at(x, z, lift=0.5):
        return (x, alamein.height_at(x, z) + lift, z)
    s = alamein.sim(dt=1 / 30)
    a = s.add_vehicle("T34", at(325, 1100), rotation=(0, 0, 0))
    b = s.add_vehicle("T34", at(325, 1140), rotation=(180, 0, 0))
    for v in (a, b):
        v.set_input("c_PIThrottle", 1.0)
    first = None
    for n in range(120):
        before = (a.body()["vel"][2], b.body()["vel"][2])
        s.step()
        if first is None and any(i["with"] == "T34" for c in a.contacts() for i in c["impulses"]):
            first, speed = n, before
            pa, pb = a.body()["pos"], b.body()["pos"]
        if first is not None and n == first + 3:
            break
    assert first is not None
    assert speed[0] > 8 and speed[1] < -8 and abs(sum(speed)) < 0.5
    assert abs((pa[2] + pb[2]) / 2 - 1120) < 1  # halfway
    after = (a.body()["vel"][2], b.body()["vel"][2])
    assert after[0] < 0.7 * speed[0] and after[1] > 0.7 * speed[1]  # both slowed hard
    assert abs(sum(after)) < 1.0


def test_team_kills_cost_spawn_cycles(alamein):
    """A human killing a human of its own team: one team kill (bots and
    other teams do not count). With spawn waves, the killer's next death adds round(its team
    kills × 1) whole cycles (20 + 5 s) to its wait, and the count starts again; at 5 team kills
    in all it is out of the game."""
    s = alamein.sim(dt=1 / 30)
    s.start_conquest()
    a = s.add_player(2, "BritishSoldier", ai=False)
    b = s.add_player(2, "BritishSoldier", ai=False)
    bot = s.add_player(2, "BritishSoldier")
    enemy = s.add_player(1, "GermanDesertSoldier", ai=False)
    s.waves = (20.0, 5.0, 5.0, 20.0)  # in the long part of the cycle: 20 s to wait
    s.step(10)  # (one spawns every third frame, `gameStatusPlaying`)
    sol = lambda p: s.players()[p]["soldier"]
    s.give_damage(sol(b), 1000.0, by=sol(a))
    s.give_damage(sol(bot), 1000.0, by=sol(a))
    s.give_damage(sol(enemy), 1000.0, by=sol(a))
    s.step(1)
    assert s.players()[a]["team_kills"] == 1 and s.players()[a]["team_kills_total"] == 1
    plain = s.players()[b]["spawn_time"]
    s.give_damage(sol(a), 1000.0)
    s.step(1)
    p = s.players()[a]
    assert p["team_kills"] == 0 and p["spawn_time"] == pytest.approx(plain - 1 / 30 + 25.0, abs=1e-3)
    assert [e for (_, j, e) in s.player_log() if j == a and "TeamKill" in str(e)] == [{"TeamKill": b}]


def test_players_die_cost_tickets_and_spawn_again(alamein):
    """Players: a bot of each team spawns in its base's group; the British one's
    soldier is killed: in conquest the team loses a ticket (`subTicket`), and 3 s later
    (`serverSpawnDelay`) the player is back, as a fresh soldier, in its group; a human's death
    costs `ticketLosePerDeath` (1) too."""
    s = alamein.sim(dt=1 / 30)
    s.waves = None  # `serverSpawnDelay` alone (the waves are tested separately)
    s.start_conquest()
    a = s.add_player(2, "BritishSoldier")
    b = s.add_player(1, "GermanDesertSoldier", ai=False)
    s.step(4)  # (one spawns every third frame, `gameStatusPlaying`)
    ps = s.players()
    assert all(p["alive"] for p in ps)
    first = ps[a]["soldier"]
    groups = {int(g): v for g, v in s.conquest()["groups"].items()}
    (_, _, ev), = [e for e in s.player_log() if e[1] == a]
    vehicle, group, point = ev["Spawned"]
    sp = groups[group]["spawns"][point]
    pos = s.vehicle(vehicle).body()["pos"]
    assert groups[group]["team"] == 2 and pos[0] == pytest.approx(sp["pos"][0], abs=0.5)
    # a metre over the point (`spawnPlayer`), as the team's soldier (`game.setTeamSkin`) with the
    # first kit (`game.setKit 2 0 GB_Scout`): its first weapon in hand, the others stowed
    assert pos[1] == pytest.approx(sp["pos"][1] + 1.0, abs=1e-4)
    assert s.vehicle(vehicle).template == "BritishSoldier"
    teams = s.conquest()["teams"]
    assert teams["skins"] == ["GermanDesertSoldier", "BritishSoldier"] and teams["kits"][1][0] == "GB_Scout"
    assert s.vehicle(vehicle).weapon()["template"] == alamein.kit_weapons("GB_Scout")[0]
    tickets = list(s.conquest()["tickets"])
    s.give_damage(first, 1000.0)
    s.step(1)
    assert s.conquest()["tickets"] == [tickets[0], tickets[1] - 1]
    assert not s.players()[a]["alive"]
    s.step(89)
    assert not s.players()[a]["alive"]
    s.step(4)  # (its time is up after 90 frames; the next spawn pass is within 3)
    p = s.players()[a]
    assert p["alive"] and p["soldier"] != first and p["deaths"] == 1
    s.give_damage(s.players()[b]["soldier"], 1000.0)
    s.step(1)
    assert s.conquest()["tickets"][0] == tickets[0] - 1


def test_submarine_oxygen_and_depth(midway):
    """A Gato: held 30 m under the sea its
    oxygen goes at 0.009 a second (`PlayerControlObject::handleFrameUpdate`, every half second)
    and, once out, whoever is inside loses 1 a second (`damageAllAttachedSoldiers`); 45 m down,
    past `maxDepth` 40, the boat itself loses 5 a second."""
    s = midway.sim(dt=1 / 30)
    sea = midway.water_level
    sub = s.add_vehicle("Gato", (900.0, sea - 30.0, 900.0), (0.0, 0.0, 0.0))
    sailor = s.add_vehicle("USSoldier", (0.0, 500.0, 0.0), (0.0, 0.0, 0.0))
    sailor.board(sub.index, 0)
    hold = lambda y: sub.set_body(pos=(900.0, y, 900.0), vel=(0.0, 0.0, 0.0), rotvel=(0.0, 0.0, 0.0))
    for _ in range(30 * 110):
        hold(sea - 30.0)
        s.step()
    assert not [d for d in s.damage_log() if d["vehicle"] == sailor.index]
    for _ in range(30 * 5):
        hold(sea - 30.0)
        s.step()
    hurt = [d for d in s.damage_log() if d["vehicle"] == sailor.index]
    assert hurt and all(d["source"] == "Water" for d in hurt)
    assert sum(d["amount"] for d in hurt) == pytest.approx(3.5, abs=0.6)
    assert not [d for d in s.damage_log() if d["vehicle"] == sub.index]
    for _ in range(30 * 2):
        hold(sea - 45.0)
        s.step()
    hull = [d["amount"] for d in s.damage_log() if d["vehicle"] == sub.index]
    assert sum(hull) == pytest.approx(10.0, abs=2.6)

