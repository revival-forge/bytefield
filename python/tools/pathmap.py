"""Decode a level's pathfinding map (`Pathfinding/<Type>Level<n>Map.raw`) into a PNG.

The format:
five int32 (patches in x and in y as bits, a patch's side as bits, the scale shift, log2 of the
bits per pixel), the number of fill words and those words (patches all of one value), then one
int32 per patch, row by row: >= 0, the fill word it repeats; -1, followed by its words. A patch
covers 2^side m in pixels of 2^scale m ((side >> scale)² · bits per pixel / 32 words). Inside a
patch the pixels go row by row, 32/bpp to a word, the first in the lowest bits.

    python3 -I python/tools/pathmap.py <file.raw> <out.png> [--flip]
"""

import struct
import sys
import zlib


def decode(data: bytes):
    """(width, height, bits per pixel, rows of pixel values)."""
    xb, yb, side_b, scale, bpp_b = struct.unpack_from("<5i", data, 0)
    at = 20
    (n,) = struct.unpack_from("<i", data, at)
    at += 4
    fills = list(struct.unpack_from(f"<{n}I", data, at))
    at += 4 * n
    bpp = 1 << bpp_b
    side = 1 << (side_b - scale)  # pixels per patch row
    side_b -= scale
    per_word = 32 // bpp
    words = max(1, side * side // per_word)
    px, py = 1 << xb, 1 << yb
    patches = []
    for _ in range(px * py):
        (k,) = struct.unpack_from("<i", data, at)
        at += 4
        if k < 0:
            patches.append(list(struct.unpack_from(f"<{words}I", data, at)))
            at += 4 * words
        else:
            patches.append([fills[k]] * words)
    if at != len(data):
        raise ValueError(f"{len(data) - at} bytes left over")
    w, h = px * side, py * side
    mask = (1 << bpp) - 1
    rows = []
    for y in range(h):
        row = bytearray(w)
        prow, iy = y >> side_b, y & (side - 1)
        for x in range(w):
            patch = patches[(x >> side_b) + (prow << xb)]
            i = iy * side + (x & (side - 1))
            row[x] = (patch[i // per_word] >> ((i % per_word) * bpp)) & mask
        rows.append(row)
    return w, h, bpp, rows


def png(path: str, w: int, h: int, rows) -> None:
    raw = b"".join(b"\x00" + bytes(r) for r in rows)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        f.write(chunk(b"IEND", b""))


def main(argv):
    if len(argv) < 3:
        print(__doc__)
        return 2
    data = open(argv[1], "rb").read()
    w, h, bpp, rows = decode(data)
    scale = 255 // ((1 << bpp) - 1)
    out = [bytes(v * scale for v in r) for r in rows]
    if "--flip" in argv:
        out.reverse()
    png(argv[2], w, h, out)
    counts = {}
    for r in rows:
        for v in r:
            counts[v] = counts.get(v, 0) + 1
    print(f"{w}x{h}, {bpp} bit(s) per pixel, values {dict(sorted(counts.items()))}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
