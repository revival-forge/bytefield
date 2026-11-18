"""Assemble a BF1942 install tree from the unshield-extracted groups of the retail discs.

    unshield -d data/discs/ex-bf42 x data/discs/bf42/data1.cab       (discs 1+2)
    unshield -d data/discs/ex-xpack1 x data/discs/d3-cab/data1.cab   (Road to Rome)
    unshield -d data/discs/ex-xpack2 x data/discs/d4-cab/data1.cab   (Secret Weapons)
    python3 -I python/tools/assemble_install.py data/discs data/game

Windows paths are case-insensitive and the discs mix `objects.rfa` and `Objects.rfa`, so later
layers replace earlier files whatever their case (the first spelling seen is kept). Files are
hard-linked, not copied. Layer order mirrors the installers: the expansion discs first, then the
base disc's own (newer) expansion archives on top.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

LAYERS = [  # (source relative to the discs dir, destination relative to the game dir)
    ("ex-bf42/Battlefield_1942", ""),
    ("ex-bf42/Language_Specific_English_US", ""),
    ("ex-bf42/Archives", "Mods/bf1942/Archives"),
    ("ex-bf42/bf1942", "Mods/bf1942/Archives/bf1942"),
    ("ex-xpack1/XPack_Files", "Mods/XPack1"),
    ("ex-xpack2/XPack_Files", "Mods/XPack2"),
    ("ex-bf42/Archives_2/XPack1", "Mods/XPack1"),
    ("ex-bf42/Archives_2/XPack2", "Mods/XPack2"),
]


def main() -> None:
    discs, game = Path(sys.argv[1]), Path(sys.argv[2])
    seen: dict[str, Path] = {}   # lowercase relative path -> path as first created
    replaced = 0
    for src, dst in LAYERS:
        root = discs / src
        if not root.is_dir():
            print(f"missing layer {root}, skipped")
            continue
        for p in sorted(root.rglob("*")):
            if not p.is_file():
                continue
            rel = (Path(dst) / p.relative_to(root)).as_posix()
            key = rel.lower()
            # reuse the first spelling of every directory and file
            parts, cur = rel.split("/"), ""
            fixed = []
            for part in parts:
                cur = f"{cur}/{part}".lstrip("/").lower()
                fixed.append(seen[cur].name if cur in seen else part)
                seen.setdefault(cur, Path(*fixed))
            target = game / Path(*fixed)
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.exists():
                target.unlink()
                replaced += 1
            os.link(p, target)
            seen[key] = Path(*fixed)
    print(f"{sum(1 for _ in game.rglob('*') if _.is_file())} files in {game}, {replaced} replaced by a later layer")


if __name__ == "__main__":
    main()
