#!/usr/bin/env python3
"""What each kind of bot knows how to do, from the lines of `examples/bot_caps`.
    python3 -I python/tools/bot_caps_table.py <dir>
<dir> holds seat_<mind>_<level>.jsonl and round_<mind>_<level>_<seed>.jsonl. Only numbers from outside."""
import collections
import glob
import json
import os
import sys

MINDS = []  # the kinds of bot in the files, filled by main


def lines(pattern):
    for p in sorted(glob.glob(pattern)):
        with open(p) as f:
            for line in f:
                if line.strip():
                    yield os.path.basename(p), json.loads(line)


def main(d):
    MINDS.extend(sorted({os.path.basename(p).split("_")[1] for p in glob.glob(f"{d}/*_*.jsonl")}))
    # seat: by class, how many templates move under the bot's hands, and how far
    seat = collections.defaultdict(list)
    for _, r in lines(f"{d}/seat_*.jsonl"):
        if "class" in r:
            seat[(r["class"], r["mind"])].append(r)
    print("## Synthetic: a bot in the driver's seat, 45 s\n")
    print("Per cell: templates that travel ≥ 20 m / templates; median distance (m); median seconds inside.\n")
    classes = sorted({c for c, _ in seat})
    print("| Class | " + " | ".join(MINDS) + " |")
    print("|---|" + "---|" * len(MINDS))
    for c in classes:
        cells = []
        for m in MINDS:
            rs = seat.get((c, m), [])
            if not rs:
                cells.append("—")
                continue
            moved = sum(r["path_m"] >= 20 for r in rs)
            med = sorted(r["path_m"] for r in rs)[len(rs) // 2]
            kept = sorted(r.get("kept_s", 0) for r in rs)[len(rs) // 2]
            cells.append(f"{moved}/{len(rs)}; {med:.0f} m; {kept:.0f} s")
        print(f"| {c} | " + " | ".join(cells) + " |")

    # planes: do they get up, and stay there?
    print("\nPlanes and submarines, the ones that climb or dive:\n")
    print("| Class | " + " | ".join(MINDS) + " |")
    print("|---|" + "---|" * len(MINDS))
    for c, key, label in [("plane", "top_above_ground", "m above ground"), ("sub", "below_water", "m under water")]:
        cells = []
        for m in MINDS:
            rs = seat.get((c, m), [])
            cells.append(f"median {sorted(r[key] for r in rs)[len(rs) // 2]:.0f}, max {max(r[key] for r in rs):.0f}" if rs else "—")
        print(f"| {c} ({label}) | " + " | ".join(cells) + " |")

    # round: by class, added up over all rounds
    rnd = collections.defaultdict(lambda: collections.Counter())
    rounds = collections.Counter()
    foot = collections.defaultdict(collections.Counter)
    caps = collections.defaultdict(collections.Counter)
    weapons = collections.defaultdict(collections.Counter)
    for name, r in lines(f"{d}/round_*.jsonl"):
        if r.get("foot"):
            rounds[r["mind"]] += 1
            for k in ("kills", "deaths", "captures", "tks", "healed", "repaired"):
                foot[r["mind"]][k] += r[k]
            for w, n in r["shots_by_projectile"].items():
                weapons[r["mind"]][w] += n
            caps[r["mind"]].update(r.get("caps", {}))
        else:
            c = rnd[(r["class"], r["mind"])]
            for k in ("boardings", "driver_s", "gunner_s", "path_m", "shots", "kills", "damage"):
                c[k] += r[k]
            for k in ("gunner_s", "gunner_shots", "gunner_kills"):
                caps[r["mind"]][k] += r.get(k, 0)
    print("\n## Real match: 32 bots, 5 min, both sides\n")
    print("Rounds: " + ", ".join(f"{m} {rounds[m]}" for m in MINDS) + ". Per cell, summed: boardings; s driving + s as gunner; m travelled; shots; kills.\n")
    print("| Class | " + " | ".join(MINDS) + " |")
    print("|---|" + "---|" * len(MINDS))
    for c in sorted({c for c, _ in rnd}):
        cells = []
        for m in MINDS:
            x = rnd.get((c, m))
            cells.append(f"{x['boardings']}; {x['driver_s']:.0f}+{x['gunner_s']:.0f} s; {x['path_m']:.0f} m; {x['shots']} shots; {x['kills']} kills" if x else "—")
        print(f"| {c} | " + " | ".join(cells) + " |")
    print("\nOn foot and in total:\n")
    print("| | " + " | ".join(MINDS) + " |")
    print("|---|" + "---|" * len(MINDS))
    for k in ("kills", "deaths", "captures", "tks", "healed", "repaired"):
        print(f"| {k} | " + " | ".join(f"{foot[m][k]:.0f}" for m in MINDS) + " |")
    print("| ammunition kinds on foot | " + " | ".join(str(len(weapons[m])) for m in MINDS) + " |")
    print("\nOn foot, shots per ammunition (the 14 most used):\n")
    print("| Ammunition | " + " | ".join(MINDS) + " |")
    print("|---|" + "---|" * len(MINDS))
    total = collections.Counter()
    for m in MINDS:
        total.update(weapons[m])
    for w, _ in total.most_common(14):
        print(f"| {w} | " + " | ".join(str(weapons[m][w]) for m in MINDS) + " |")

    # what a level gives: rows where a mind did nothing in a class the level has, by level
    seen = collections.defaultdict(dict)
    for name, r in lines(f"{d}/round_*.jsonl"):
        if not r.get("foot"):
            cell = seen[(r["level"], r["class"])].setdefault(r["mind"], collections.Counter())
            cell["driver_s"] += r["driver_s"]
            cell["gunner_s"] += r["gunner_s"]
            cell["shots"] += r["shots"]
    print("\n## Per level and class: s driving + gunner / shots\n")
    print("| Level | Class | " + " | ".join(MINDS) + " |")
    print("|---|---|" + "---|" * len(MINDS))
    for (lv, c), v in sorted(seen.items()):
        cells = [f"{v[m]['driver_s']:.0f}+{v[m]['gunner_s']:.0f} / {v[m]['shots']}" if m in v else "**0**" for m in MINDS]
        print(f"| {lv} | {c} | " + " | ".join(cells) + " |")

    capabilities(caps)
    gate(rnd, foot, seat)


# Measured, not asked of anyone: not in the gate.
CAPS = [
    ("Mines", "laid; go off; vehicles damaged; destroyed", ("mines_laid", "mines_off", "mines_vehicles_damaged", "mines_vehicles_destroyed")),
    ("C4", "laid; detonated; vehicles damaged; destroyed; damage", ("c4_laid", "c4_off", "c4_vehicles_damaged", "c4_vehicles_destroyed", "c4_damage")),
    ("Parachutes", "leave a flying plane; falls; of those, after leaving; opened; arrive alive; die", ("air_exits", "falls", "jumps", "chutes_open", "fell_alive", "fell_dead")),
    ("Disembarking", "leave a boat; near the shore; go on foot", ("boat_exits", "near_shore", "went_on")),
    ("Ladders", "climb; m gained", ("climbs", "climb_m")),
    ("Gunner", "s; shots (those of `Sim::fired`: a minimum); kills", ("gunner_s", "gunner_shots", "gunner_kills")),
]


def capabilities(caps):
    print("\n## Other capabilities (measured; they do not count for the gate)\n")
    print("| What | Figures | " + " | ".join(MINDS) + " |")
    print("|---|---|" + "---|" * len(MINDS))
    for name, what, keys in CAPS:
        print(f"| {name} | {what} | " + " | ".join("; ".join(f"{caps[m][k]:.0f}" for k in keys) for m in MINDS) + " |")


# What bots2 must do before it can be the default: the same things as the others, however badly. Summed over the rounds of the census.
GATES = [
    ("tank: drives, fires and kills", lambda r, f, s: r["tank"]["path_m"] >= 1000 and r["tank"]["shots"] >= 10 and r["tank"]["kills"] >= 1),
    ("car: drives", lambda r, f, s: r["car"]["path_m"] >= 1000),
    ("fixed weapon: takes it and kills", lambda r, f, s: r["fixed"]["boardings"] >= 50 and r["fixed"]["kills"] >= 1),
    ("plane: flies and fires", lambda r, f, s: r["plane"]["path_m"] >= 50000 and r["plane"]["shots"] >= 100),
    ("plane: takes off (seat, median ≥ 30 m)", lambda r, f, s: bool(s) and sorted(x["top_above_ground"] for x in s)[len(s) // 2] >= 30),
    ("boat: sails, fires and kills", lambda r, f, s: r["ship"]["path_m"] >= 10000 and r["ship"]["shots"] >= 100 and r["ship"]["kills"] >= 1),
    ("medic: heals", lambda r, f, s: f["healed"] > 0),
    ("engineer: repairs", lambda r, f, s: f["repaired"] > 0),
    ("takes flags", lambda r, f, s: f["captures"] >= 18),
]


def gate(rnd, foot, seat):
    print("\n## Gate: what it takes to be the default option\n")
    print("| Capability | " + " | ".join(MINDS) + " |")
    print("|---|" + "---|" * len(MINDS))
    for name, ok in GATES:
        cells = []
        for m in MINDS:
            r = collections.defaultdict(collections.Counter, {c: x for (c, mm), x in rnd.items() if mm == m})
            # (`seat` is a separate run: without its files there is nothing to say)
            if "seat" in name and not any(k[0] == "plane" for k in seat):
                cells.append("no data")
                continue
            cells.append("yes" if ok(r, foot[m], seat.get(("plane", m), [])) else "**no**")
        print(f"| {name} | " + " | ".join(cells) + " |")


if __name__ == "__main__":
    main(sys.argv[1])
