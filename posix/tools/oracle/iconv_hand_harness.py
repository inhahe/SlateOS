"""glibc 2.39's hand-written single-byte character sets -- iso646.c's 23
national variants of ISO 646, ISO_11548-1 and ARMSCII-8 -- as the oracle for
the tables posix/tools/gen_iconv_8bit.py makes of them from their charmaps.

    python posix/tools/oracle/iconv_hand_harness.py   # writes posix/src/iconv_hand_oracle.txt

For each set, glibc's converter decodes every byte on its own and encodes
every code point (U+0000 to U+10FFFF, surrogates aside). The table records
what decoding gave, one line a set:

    <name> <256 entries>

each entry a byte's code point in hex, or `-` where glibc refused the byte.
Encoding is not recorded but checked, here, against the one rule iconv.rs
applies to every 8-bit table: a code point is written as the byte that
decodes to it, and nothing else is written -- but for the bytes a set's
`.irreversible` file names, which decode and are never written, and the
Unicode tag characters (U+E0000-U+E007F), which every glibc converter drops
without a word. The harness refuses to write a table if glibc's encoder
answers any code point otherwise, so the table's being there is the claim.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "iconv_hand_oracle.txt"
GLIBC = Path("D:/refsrc/glibc-2.39")

# gconv-modules' canonical names, in its order (as gen_iconv_8bit.py reads
# them), and each set's decode-only bytes from its .irreversible file.
SETS = ["BS_4730", "CSA_Z243.4-1985-1", "CSA_Z243.4-1985-2", "DIN_66003", "DS_2089", "ES", "ES2",
        "GB_1988-80", "IT", "JIS_C6220-1969-RO", "JIS_C6229-1984-B", "JUS_I.B1.002", "KSC5636",
        "MSZ_7795.3", "NC_NC00-10", "NF_Z_62-010", "NF_Z_62-010_1973", "NS_4551-1", "NS_4551-2",
        "PT", "PT2", "SEN_850200_B", "SEN_850200_C", "ISO_11548-1", "ARMSCII-8"]

PROGRAM = r"""
#include <errno.h>
#include <iconv.h>
#include <stdint.h>
#include <stdio.h>

int main(int argc, char **argv) {
    for (int k = 1; k < argc; k++) {
        iconv_t dec = iconv_open("UCS-4LE", argv[k]);
        iconv_t enc = iconv_open(argv[k], "UCS-4LE");
        if (dec == (iconv_t) -1 || enc == (iconv_t) -1) {
            printf("open %s failed\n", argv[k]);
            return 1;
        }
        printf("set %s\n", argv[k]);
        printf("d");
        for (int b = 0; b < 256; b++) {
            unsigned char in = (unsigned char) b;
            uint32_t out = 0;
            char *ip = (char *) &in, *op = (char *) &out;
            size_t il = 1, ol = 4;
            iconv(dec, NULL, NULL, NULL, NULL);
            size_t r = iconv(dec, &ip, &il, &op, &ol);
            if (r != (size_t) -1 && il == 0 && ol == 0)
                printf(" %04x", out);
            else
                printf(" -");
        }
        printf("\n");
        for (uint32_t cp = 0; cp <= 0x10FFFF; cp++) {
            if (cp >= 0xD800 && cp <= 0xDFFF) continue;
            uint32_t in = cp;
            unsigned char out[8];
            char *ip = (char *) &in, *op = (char *) out;
            size_t il = 4, ol = sizeof out;
            iconv(enc, NULL, NULL, NULL, NULL);
            size_t r = iconv(enc, &ip, &il, &op, &ol);
            if (r == (size_t) -1 || il != 0) continue;
            printf("e %x", cp);
            for (size_t i = 0; i < sizeof out - ol; i++) printf(" %02x", out[i]);
            printf("\n");
        }
        iconv_close(dec);
        iconv_close(enc);
    }
    return 0;
}
"""


def irreversible(name):
    p = GLIBC / "iconvdata" / f"{name}.irreversible"
    if not p.exists():
        return set()
    return {int(line.split()[0], 16) for line in p.read_text().splitlines() if line.strip()}


def main():
    with workdir() as tmp:
        (Path(tmp) / "hand.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O1 -w -o hand hand.c && ./hand " + " ".join(SETS))
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-2000:]}")
    lines, name, decode = [], None, None
    sets = {}
    for line in r.stdout.splitlines():
        f = line.split()
        if f[0] == "set":
            name = f[1]
            sets[name] = {"decode": None, "encode": {}}
        elif f[0] == "d":
            sets[name]["decode"] = f[1:]
        elif f[0] == "e":
            sets[name]["encode"][int(f[1], 16)] = [int(b, 16) for b in f[2:]]
    for name in SETS:
        s = sets[name]
        decode = s["decode"]
        assert len(decode) == 256, name
        only = irreversible(name)
        want = {}
        for b, v in enumerate(decode):
            if v != "-" and b not in only:
                cp = int(v, 16)
                assert cp not in want, (name, hex(cp), "decoded from two reversible bytes")
                want[cp] = [b]
        tags = set(range(0xE0000, 0xE0080))
        got = {cp: bs for cp, bs in s["encode"].items() if not (cp in tags and bs == [])}
        if got != want:
            extra = {hex(k): v for k, v in got.items() if want.get(k) != v}
            missing = [hex(k) for k in want if k not in got]
            sys.exit(f"{name}: glibc's encoder is not the decoder's reverse: "
                     f"extra {list(extra.items())[:8]} missing {missing[:8]}")
        for b in only:
            assert decode[b] != "-", (name, b)
        lines.append(f"{name} " + " ".join(decode))
    header = ("# glibc 2.39's hand-written single-byte sets under WSL, every byte decoded\n"
              "# (posix/tools/oracle/iconv_hand_harness.py): <name> <each byte's code point,\n"
              "# or - where glibc refuses it>. Each encoder checked there to be the reverse.\n")
    OUT.write_text(header + "\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(lines)} sets -> {OUT}")


if __name__ == "__main__":
    main()
