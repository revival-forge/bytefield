"""Does any file we would publish carry text copied from the games' or mods' scripts?

    git ls-files -z -- ':!re/' | python3 -I python/tools/corpus_leak.py <corpus dir>... [--k 3] [--long 80]

The corpus is every `.con`/`.inc` (or `--corpus-ext`) under the given dirs (the scripts as extracted, e.g. data/extracted/bf1942 or
data/mods/gc-src); the files to check come NUL-separated on stdin, relative to the current dir. Both sides go
through the same normalisation: one line per script line (a literal `\\n` in source code splits too), without
indentation, comment markers, quotes, backticks or list bullets, lowercase, blanks collapsed (the first line of a
string literal, glued to the code before its quote, does not match: a block needs `k` more lines).

Lines that appear in many corpus files (`endIf`, `ObjectTemplate.setNetworkableInfo BulletInfo`) are boilerplate
and are dropped on both sides before looking for runs. What is left, two kinds of hit:
- a run of `k` consecutive lines that is also a run in some corpus file (a copied block);
- a single line of `long` characters or more that is in the corpus (a long comment, a long list of values).

Prints one block per file with hits (the line numbers and the start of each hit) and a summary. Exit status 1 if
anything was found, so that a caller may block on it; publish-check only reports.
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

SKIP_DIRS = {".git", "target", "data", "__pycache__", ".venv"}
TEXT_EXT = {".rs", ".py", ".md", ".txt", ".json", ".toml", ".con", ".inc", ".sh", ".c", ".in", ".yaml", ".yml", ".csv", ""}
STRIP = re.compile(r'^(?:\s|//+!?|#+|>|\*|-|\+|rem\b|"|`|\'|\|)+', re.I)
TAIL = re.compile(r'(?:\s|"|`|\'|,|;|\\|\|)+$')
BLANKS = re.compile(r"\s+")


def decode(raw: bytes) -> str:
    if raw[:2] in (b"\xff\xfe", b"\xfe\xff"):
        return raw.decode("utf-16", errors="replace")
    return raw.decode("latin-1")


def norm_lines(text: str) -> list[tuple[int, str]]:
    """(1-based line number, normalised line) for every line that keeps some text."""
    out = []
    for no, line in enumerate(text.splitlines(), 1):
        for part in line.split("\\n"):
            s = BLANKS.sub(" ", TAIL.sub("", STRIP.sub("", part))).lower()
            if len(s) >= 4:
                out.append((no, s))
    return out


def corpus_files(dirs: list[Path], exts: tuple[str, ...] = (".con", ".inc")):
    for d in dirs:
        for p in d.rglob("*"):
            if p.suffix.lower() in exts and p.is_file():
                yield p


class Corpus:
    def __init__(self, files, k: int, common: int):
        self.k = k
        docs = [norm_lines(decode(p.read_bytes())) for p in files]
        df = Counter()
        for lines in docs:
            df.update({s for _, s in lines})
        self.common = {s for s, n in df.items() if n >= common}
        self.lines = set(df) - self.common
        self.runs = set()
        for lines in docs:
            kept = [s for _, s in lines if s not in self.common]
            self.runs.update(hash(tuple(kept[i:i + k])) for i in range(len(kept) - k + 1))
        self.files = len(docs)

    def hits(self, text: str, long: int) -> list[tuple[int, str, str]]:
        """(line number, kind, the normalised text) of each hit, a run reported at its first line."""
        kept = [(no, s) for no, s in norm_lines(text) if s not in self.common]
        found, covered = [], set()
        for i in range(len(kept) - self.k + 1):
            if hash(tuple(s for _, s in kept[i:i + self.k])) in self.runs:
                if i not in covered:
                    found.append((kept[i][0], "run", " / ".join(s for _, s in kept[i:i + self.k])))
                covered.update(range(i, i + self.k))
        for i, (no, s) in enumerate(kept):
            if i not in covered and len(s) >= long and s in self.lines:
                found.append((no, "long", s))
        return sorted(found)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("corpus", nargs="+", type=Path)
    ap.add_argument("--corpus-ext", action="append", help="extension of the corpus files, repeatable (default .con and .inc)")
    ap.add_argument("--k", type=int, default=3, help="consecutive lines that make a copied block")
    ap.add_argument("--long", type=int, default=80, help="characters for a single line to count alone")
    ap.add_argument("--common", type=int, default=20, help="corpus files a line must be in to be boilerplate")
    ap.add_argument("--show", type=int, default=5, help="hits shown per file")
    a = ap.parse_args(argv)
    dirs = [d for d in a.corpus if d.is_dir()]
    if not dirs:
        print("corpus_leak: no corpus dir found; nothing checked", file=sys.stderr)
        return 0
    corpus = Corpus(corpus_files(dirs, tuple(a.corpus_ext or (".con", ".inc"))), a.k, a.common)
    print(f"corpus: {corpus.files} scripts, {len(corpus.lines)} distinct lines, {len(corpus.common)} boilerplate")
    names = [n for n in sys.stdin.read().split("\0") if n]
    total = 0
    for name in names:
        p = Path(name)
        if not p.is_file() or SKIP_DIRS & set(p.parts) or p.suffix.lower() not in TEXT_EXT:
            continue
        hits = corpus.hits(decode(p.read_bytes()), a.long)
        if not hits:
            continue
        total += len(hits)
        print(f"{name}: {len(hits)}")
        for no, kind, s in hits[:a.show]:
            print(f"  {no}: {kind}: {s[:100]}")
    print(f"{total} hits")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main())
