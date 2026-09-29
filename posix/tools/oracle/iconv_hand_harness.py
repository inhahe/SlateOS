"""glibc 2.39's single-byte character sets that are not generated from a
charmap -- iso646.c's 23 national variants of ISO 646, ISO_11548-1 and
ARMSCII-8, and the 55 modules glibc builds on 8bit-gap.c from table headers
of their own (IBM's EBCDIC and PC code pages past the generated ones, CP737,
CP775, ISIRI-3342) -- as the oracle for the tables
posix/tools/gen_iconv_8bit.py makes of them.

    python posix/tools/oracle/iconv_hand_harness.py   # writes posix/src/iconv_hand_oracle.txt

For each set, glibc's converter decodes every byte on its own and encodes
every code point (U+0000 to U+10FFFF, surrogates aside). The table records
it in up to three lines a set:

    <name> <256 entries>                   each byte's code point in hex, or
                                           `-` where glibc refused the byte
    <name> decode-only <byte> ...          the bytes that decode but whose
                                           code point is written otherwise
    <name> encode-only <cp>:<byte> ...     the code points no byte decodes to
                                           that are written, and as what

the last two only where there are any. Together they are the whole of each
encoder, and the harness refuses to write a table unless they are: every
code point glibc writes is one byte, the byte that decodes to it unless
that byte is decode-only, else its encode-only byte -- and nothing else is
written but the Unicode tag characters (U+E0000-U+E007F), which every glibc
converter drops without a word. That is the one rule iconv.rs applies to
every 8-bit table, so the table's being there is the claim that it can.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402
from gen_iconv_8bit import DEFAULT_GLIBC, module_lists, table_header_modules  # noqa: E402

OUT = POSIX_SRC / "iconv_hand_oracle.txt"

# gconv-modules' canonical names, in its order (as gen_iconv_8bit.py reads
# them): the hand-written sets whose converters are their charmaps.
CHARMAP_SETS = [
    "BS_4730", "CSA_Z243.4-1985-1", "CSA_Z243.4-1985-2", "DIN_66003", "DS_2089", "ES", "ES2",
    "GB_1988-80", "IT", "JIS_C6220-1969-RO", "JIS_C6229-1984-B", "JUS_I.B1.002", "KSC5636",
    "MSZ_7795.3", "NC_NC00-10", "NF_Z_62-010", "NF_Z_62-010_1973", "NS_4551-1", "NS_4551-2",
    "PT", "PT2", "SEN_850200_B", "SEN_850200_C", "ISO_11548-1", "ARMSCII-8"]


def header_sets():
    """The table-header modules' names, by file name, as the generator finds them."""
    lists = module_lists(DEFAULT_GLIBC / "iconvdata/Makefile")
    generated = set(lists["gen-8bit-modules"]) | set(lists["gen-8bit-gap-modules"])
    return [m.upper() for m, *_ in table_header_modules(DEFAULT_GLIBC / "iconvdata", generated)]


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


def main():
    sets_wanted = CHARMAP_SETS + header_sets()
    with workdir() as tmp:
        (Path(tmp) / "hand.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O1 -w -o hand hand.c && ./hand " + " ".join(sets_wanted))
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-2000:]}")
    name = None
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
    lines = []
    tags = set(range(0xE0000, 0xE0080))
    for name in sets_wanted:
        s = sets[name]
        decode = s["decode"]
        assert len(decode) == 256, name
        dec = {b: int(v, 16) for b, v in enumerate(decode) if v != "-"}
        written = {}
        for cp, bs in s["encode"].items():
            if cp in tags and bs == []:
                continue
            if len(bs) != 1:
                sys.exit(f"{name}: U+{cp:04X} is written as {bs}, not one byte")
            if cp > 0xFFFF:
                sys.exit(f"{name}: U+{cp:04X}, beyond the BMP, is written")
            written[cp] = bs[0]
        decode_only = sorted(b for b, cp in dec.items() if written.get(cp) != b)
        encode_only = sorted((cp, b) for cp, b in written.items() if dec.get(b) != cp)
        reverse = {cp: b for b, cp in dec.items() if b not in decode_only}
        for cp, b in encode_only:
            if cp in reverse:
                sys.exit(f"{name}: U+{cp:04X} is read from 0x{reverse[cp]:02x} and written 0x{b:02x}")
        lines.append(f"{name} " + " ".join(decode))
        if decode_only:
            lines.append(f"{name} decode-only " + " ".join(f"{b:02x}" for b in decode_only))
        if encode_only:
            lines.append(f"{name} encode-only " + " ".join(f"{cp:04x}:{b:02x}" for cp, b in encode_only))
    header = ("# glibc 2.39's single-byte sets not generated from a charmap, under WSL\n"
              "# (posix/tools/oracle/iconv_hand_harness.py): <name> <each byte's code point,\n"
              "# or - where glibc refuses it>; <name> decode-only <bytes decoded and never\n"
              "# written>; <name> encode-only <code point:byte written with no byte decoding\n"
              "# to it>. Every encoder is checked there to be exactly those.\n")
    OUT.write_text(header + "\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(sets_wanted)} sets -> {OUT}")


if __name__ == "__main__":
    main()
