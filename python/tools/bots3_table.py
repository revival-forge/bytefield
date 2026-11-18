"""The table of a tournament of minds (`examples/tournament3`).

    python3 -I python/tools/bots3_table.py data/out/bots3/0b/self [--md table.md]

Reads the JSON files of a folder (one per round). For each pair of minds (A against B):
  - wins of A, of B and draws (the winner if it ended; otherwise whoever has more tickets, a draw within `DRAW`), ticket margin
    of A (mean and range) and flags and kills of each, per map and in total. The sides are swapped with the same
    seed, so the map's bias cancels out in the mean of each pair.
  - If the pair is a mind against itself (`--a X --b X`), the table gives the bias (mean margin of side 1, which should be 0)
    and the noise (deviation of the margin between seeds): what A against B has to exceed to count. For the other pairs the
    mind-against-itself one is used, if it is in the folder.
Then, per mind and map, each `B##` of the meter (median and range across rounds, in either side) and the `dumb` (B16) numbers
per side: events per bot-minute.
Markdown output; `--md` also saves it as well as printing it. A field the meter could not measure is not shown (it is not 0).

For a scoreboard that reads one figure: `--field B02_pct_with_2_teammates` (or `--total "stuck foot %"`, from B16) prints
only `figure <name> <median>` over all rounds and both sides, without tables. `--cards B14,B15` gives only those cards per mind and map.
"""

import argparse
import collections
import glob
import json
import os
import statistics

DRAW = 2


def result(r):
    """1 or 2: the side that won; 0 a draw."""
    w = r["summary"].get("winner")
    if w in (1, 2):
        return w
    a, b = r["summary"]["tickets"] or [0, 0]
    return 0 if abs(a - b) <= DRAW else (1 if a > b else 2)


def load(folder):
    out = []
    for f in sorted(glob.glob(os.path.join(folder, "*.json"))):
        with open(f) as fh:
            r = json.load(fh)
        if "meta" in r and "meter" in r:
            out.append(r)
    return out


def level_key(r):
    return f"{r['meta']['mod']}:{r['meta']['level']}"


def mean(xs):
    return statistics.fmean(xs) if xs else float("nan")


def sd(xs):
    return statistics.stdev(xs) if len(xs) > 1 else 0.0


def med(xs):
    return statistics.median(xs)


def margin_a(r):
    t = r["summary"]["tickets"] or [0, 0]
    sa = r["meta"]["side_a"]
    return t[sa - 1] - t[2 - sa]


def stats_pair(rounds):
    """The figures of A against B for a set of rounds."""
    w = {"A": 0, "B": 0, "=": 0}
    marg, fa, fb, ka, kb = [], [], [], [], []
    for r in rounds:
        sa = r["meta"]["side_a"]
        res = result(r)
        w["=" if res == 0 else ("A" if res == sa else "B")] += 1
        marg.append(margin_a(r))
        fl, ki = r["summary"]["flags"], r["summary"]["kills"]
        fa.append(fl[sa])
        fb.append(fl[3 - sa])
        ka.append(ki[sa])
        kb.append(ki[3 - sa])
    return w, marg, fa, fb, ka, kb


def row(name, rounds, noise):
    w, marg, fa, fb, ka, kb = stats_pair(rounds)
    n = len(rounds)
    m = mean(marg)
    note = ""
    if noise is not None and n:
        note = " (within the noise)" if abs(m) < noise else ""
    return f"| {name} | {n} | {w['A']} / {w['B']} / {w['=']} | {m:+.0f}{note} | {min(marg):+d} .. {max(marg):+d} | {mean(fa):.1f} - {mean(fb):.1f} | {mean(ka):.0f} - {mean(kb):.0f} |"


def bias_noise(rounds):
    """From rounds of a mind against itself: the mean margin (side 1 minus side 2) and its deviation between seeds."""
    per_seed = collections.defaultdict(list)
    for r in rounds:
        t = r["summary"]["tickets"] or [0, 0]
        per_seed[(level_key(r), r["meta"]["seed"])].append(t[0] - t[1])
    ms = [mean(v) for v in per_seed.values()]
    return mean(ms), sd(ms)


def pair_tables(rounds, md):
    pairs = collections.defaultdict(list)
    for r in rounds:
        pairs[(r["meta"]["a"], r["meta"]["b"])].append(r)
    own = {a: bias_noise(rs)[1] for (a, b), rs in pairs.items() if a == b}
    for (a, b), rs in sorted(pairs.items()):
        maps = sorted({level_key(r) for r in rs})
        md.append(f"## {a} against {b} ({len(rs)} rounds)\n")
        if a == b:
            md.append("Self-play: the margin is that of side 1 minus that of side 2 (the side bias); its deviation between seeds, the noise.\n")
            md.append("| map | rounds | side 1 wins / side 2 / = | bias (tickets) | noise (sd) | flags s1 - s2 | kills s1 - s2 |")
            md.append("|---|---|---|---|---|---|---|")
            for name, group in [(m, [r for r in rs if level_key(r) == m]) for m in maps] + [("**total**", rs)]:
                w = {1: 0, 2: 0, 0: 0}
                for r in group:
                    w[result(r)] += 1
                s, n = bias_noise(group)
                fl = [r["summary"]["flags"] for r in group]
                ki = [r["summary"]["kills"] for r in group]
                md.append(f"| {name} | {len(group)} | {w[1]} / {w[2]} / {w[0]} | {s:+.0f} | {n:.0f} | {mean([f[1] for f in fl]):.1f} - {mean([f[2] for f in fl]):.1f} | {mean([k[1] for k in ki]):.0f} - {mean([k[2] for k in ki]):.0f} |")
            md.append("")
        else:
            noise = own.get(a, own.get(b))
            if noise is not None:
                md.append(f"Reference noise (self-play): sd {noise:.0f} tickets; a mean margin smaller than that is flagged.\n")
            md.append("| map | rounds | wins A / B / = | margin A (tickets, mean) | range | flags A - B | kills A - B |")
            md.append("|---|---|---|---|---|---|---|")
            for m in maps:
                md.append(row(m, [r for r in rs if level_key(r) == m], noise))
            md.append(row("**total**", rs, noise))
            md.append("")
        disp = [r["meter"]["B01_contested_flags_max"] for r in rs]
        md.append(f"B01 flags contested at once (maximum per round): median {med(disp):g}, range {min(disp)}..{max(disp)}.\n")


def fields(rounds):
    by = collections.defaultdict(lambda: collections.defaultdict(list))
    for r in rounds:
        m = level_key(r)
        for side, mind in ((r["meta"]["side_a"], r["meta"]["a"]), (3 - r["meta"]["side_a"], r["meta"]["b"])):
            for k, v in r["meter"]["sides"][str(side)].items():
                by[(mind, m)][k].append(v)
    return by


def card_tables(rounds, md, cards=None):
    by = fields(rounds)
    minds = sorted({k[0] for k in by})
    maps = sorted({k[1] for k in by})
    md.append("## B## cards per mind and map\n")
    md.append("Median (min..max across rounds) of what the meter measured for each side; a `-` means it could never be measured (not 0). The rounds count both sides.\n")
    names = sorted({k for v in by.values() for k in v if not cards or k.split("_")[0] in cards})
    for mind in minds:
        md.append(f"### {mind}\n")
        md.append("| card | " + " | ".join(maps) + " |")
        md.append("|---|" + "---|" * len(maps))
        for k in names:
            cells = []
            for m in maps:
                v = by.get((mind, m), {}).get(k)
                cells.append(f"{med(v):g} ({min(v):g}..{max(v):g})" if v else "-")
            md.append(f"| {k} | " + " | ".join(cells) + " |")
        md.append("")


def dumb_tables(rounds, md):
    acc = collections.defaultdict(lambda: collections.defaultdict(float))
    for r in rounds:
        m = level_key(r)
        for side, mind in ((r["meta"]["side_a"], r["meta"]["a"]), (3 - r["meta"]["side_a"], r["meta"]["b"])):
            d = r["meter"]["B16"]["by_side"][str(side)]
            a = acc[(mind, m)]
            a["alive_s"] += d["alive_s"]
            a["shots"] += d["shots"]
            a["deaths"] += d["deaths"]
            a["without_firing"] += d["died_without_firing"]
            for k, e in d["events"].items():
                a[f"ev:{k}"] += e["n"]
                a[f"dur:{k}"] += e["dur"]
    md.append("## B16 (the dumb things, `dumb`) per side\n")
    md.append("Events per bot-minute (sum of the rounds); `without firing` is the % of bot deaths that had not fired in that life; in parentheses, total seconds of the event per bot-minute.\n")
    minds = sorted({k[0] for k in acc})
    maps = sorted({k[1] for k in acc})
    kinds = sorted({k[3:] for v in acc.values() for k in v if k.startswith("ev:")})
    for mind in minds:
        md.append(f"### {mind}\n")
        md.append("| map | bot-min | shots/bot-min | without firing % | " + " | ".join(kinds) + " |")
        md.append("|---|---|---|---|" + "---|" * len(kinds))
        for m in maps:
            a = acc.get((mind, m))
            if not a:
                continue
            bm = max(a["alive_s"] / 60, 1e-9)
            cel = [f"{a[f'ev:{k}'] / bm:.2f} ({a[f'dur:{k}'] / bm:.1f})" for k in kinds]
            md.append(f"| {m} | {bm:.0f} | {a['shots'] / bm:.1f} | {100 * a['without_firing'] / max(a['deaths'], 1):.0f} | " + " | ".join(cel) + " |")
        md.append("")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("dir")
    ap.add_argument("--md", help="save the table in this file")
    ap.add_argument("--cards", dest="cards", help="only these cards in the B## table (e.g. B14,B15); no pair or B16 tables")
    ap.add_argument("--field", dest="field", help="only the median of a meter field (B##_...) across rounds and sides")
    ap.add_argument("--total", help="only the median of a B16 (dumb) total across rounds")
    a = ap.parse_args()
    rounds = load(a.dir)
    if not rounds:
        raise SystemExit(f"{a.dir}: no rounds")
    if a.field or a.total:
        if a.field:
            v = [x["meter"]["sides"][k][a.field] for x in rounds for k in "12" if a.field in x["meter"]["sides"][k]]
        else:
            v = [x["meter"]["B16"]["totals"][a.total] for x in rounds if a.total in x["meter"]["B16"]["totals"]]
        if not v:
            raise SystemExit(f"figure not measured: {a.field or a.total}")
        print(f"figure {a.field or a.total} {med(v):.2f} ({len(v)} measurements)")
        return
    md = [f"# Tournament of {a.dir}\n"]
    if a.cards:
        card_tables(rounds, md, set(a.cards.split(",")))
    else:
        pair_tables(rounds, md)
        card_tables(rounds, md)
        dumb_tables(rounds, md)
    text = "\n".join(md)
    print(text)
    if a.md:
        with open(a.md, "w") as fh:
            fh.write(text + "\n")


if __name__ == "__main__":
    main()
