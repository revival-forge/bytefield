"""List or extract files from a MODE1/2352 raw CD image (ISO9660), without mounting."""
import struct, sys
from pathlib import Path

RAW, HDR, DATA = 2352, 16, 2048

class Img:
    def __init__(self, path):
        self.f = open(path, "rb")
    def sector(self, lba, n=1):
        out = bytearray()
        for i in range(n):
            self.f.seek((lba + i) * RAW + HDR)
            out += self.f.read(DATA)
        return bytes(out)
    def read(self, lba, size):
        n = (size + DATA - 1) // DATA
        return self.sector(lba, n)[:size]
    def walk(self, lba=None, size=None, prefix=""):
        if lba is None:
            pvd = self.sector(16)
            assert pvd[1:6] == b"CD001", pvd[:8]
            root = pvd[156:190]
            lba, size = struct.unpack_from("<I", root, 2)[0], struct.unpack_from("<I", root, 10)[0]
        data = self.read(lba, size)
        i = 0
        while i < len(data):
            ln = data[i]
            if ln == 0:
                i = (i // DATA + 1) * DATA
                continue
            rec = data[i:i + ln]
            ext, sz, flags, nl = struct.unpack_from("<I", rec, 2)[0], struct.unpack_from("<I", rec, 10)[0], rec[25], rec[32]
            name = rec[33:33 + nl]
            i += ln
            if name in (b"\x00", b"\x01"):
                continue
            name = name.decode("latin-1").split(";")[0]
            path = prefix + name
            if flags & 2:
                yield from self.walk(ext, sz, path + "/")
            else:
                yield path, ext, sz

if __name__ == "__main__":
    img = Img(sys.argv[1])
    if len(sys.argv) == 2:
        for p, e, s in img.walk():
            print(f"{s:>12} {p}")
    else:
        out = Path(sys.argv[2]); want = [w.lower() for w in sys.argv[3:]]
        for p, e, s in img.walk():
            if not want or any(p.lower().startswith(w) for w in want):
                dst = out / p; dst.parent.mkdir(parents=True, exist_ok=True)
                with open(dst, "wb") as f:
                    left, lba = s, e
                    while left > 0:
                        chunk = img.read(lba, min(left, 512 * DATA)); f.write(chunk)
                        left -= len(chunk); lba += 512
                print(p)
