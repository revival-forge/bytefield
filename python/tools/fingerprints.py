#!/usr/bin/env python3
"""The table of `bytefield-content verify` (crates/content/fingerprints.json): size and MD5 of the files of a game's folders, by version.
Only names, sizes and hashes: nothing of the game. What depends on the language or on the player stays out (`*_000.rfa`, `Settings`).

    python3 -I python/tools/fingerprints.py <install> <version> <table.json>     (replaces that version's rows)
"""
import hashlib
import json
import pathlib
import re
import sys

FOLDERS = ("bf1942", "XPack1", "XPack2")


def md5(path):
    h = hashlib.md5()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def rows(folder):
    files = [p for p in folder.rglob("*") if p.is_file() and (p.name.lower() == "init.con" and p.parent == folder or (p.suffix.lower() == ".rfa" and not re.search(r"_000\.rfa$", p.name, re.I)))]
    return [[p.relative_to(folder).as_posix(), p.stat().st_size, md5(p)] for p in sorted(files)]


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    install, version, out = pathlib.Path(sys.argv[1]), sys.argv[2], pathlib.Path(sys.argv[3])
    tables = json.loads(out.read_text()) if out.exists() else []
    tables = [t for t in tables if t["version"] != version]
    for name in FOLDERS:
        tables.append({"folder": name, "version": version, "files": rows(install / "Mods" / name)})
    tables.sort(key=lambda t: (t["folder"].lower(), t["version"]))
    with open(out, "w") as f:
        f.write("[\n")
        f.write(",\n".join('{"folder": %s, "version": %s, "files": [\n%s\n]}' % (json.dumps(t["folder"]), json.dumps(t["version"]), ",\n".join(json.dumps(r) for r in t["files"])) for t in tables))
        f.write("\n]\n")


if __name__ == "__main__":
    main()
