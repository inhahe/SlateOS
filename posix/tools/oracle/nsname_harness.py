"""glibc 2.39's domain-name conversions (`<arpa/nameser.h>`) and name checks
(`<resolv.h>`), as the oracle for posix/src/resolv.rs's `ns_name_ntop`,
`ns_name_pton`, `ns_name_unpack`, `ns_name_pack`, `ns_name_compress`,
`ns_name_skip`, `res_hnok`, `res_ownok`, `res_mailok` and `res_dnok`:

    python posix/tools/oracle/nsname_harness.py   # writes posix/src/nsname_oracle.txt

One call a line; `errno` is 1234 before each call. Text is written with
`\\xHH` for bytes outside `!`..`~` and for `\\` itself, and `\\x` alone for
the empty string; bytes are hex, `-` for none.

    T <text> <dstsiz> = <rc> <errno> <wire>             ns_name_pton
    N <wire> <dstsiz> = <rc> <errno> <text>             ns_name_ntop
    U <message> <offset> <dstsiz> = <rc> <errno> <wire> ns_name_unpack
    S <message> <offset> <eom> = <rc> <errno> <offset>  ns_name_skip
    P <seq> <i> <how> <name> <dstsiz> = <rc> <errno> <bytes> ; <dnptrs>
                                                        ns_name_pack (how
                                                        `wire`) or
                                                        ns_name_compress (how
                                                        `text`), the calls of
                                                        one sequence writing
                                                        one message in turn
    O <text> = <hnok> <ownok> <mailok> <dnok>           the four checks

The messages are ns_harness.py's. In `P` lines the name packed goes at the
message's end so far, and `dnptrs` is the table after the call as offsets
into the message, `-` ending it; a sequence starts with the table holding
the message (the header) alone, or -- sequence `nocomp` -- with no table.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import ns_harness as nh  # noqa: E402
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "nsname_oracle.txt"

LONG63 = b"a" * 63
LONG64 = b"a" * 64
# The longest name there can be: four labels of 62 and one of 1 -- 253
# characters of text, and 4 * 63 + 2 + 1 = 255 wire bytes with the root's.
# One more character is one byte too many.
NAME253 = b".".join([b"b" * 62] * 4) + b".c"
NAME254 = NAME253 + b"d"
NAME255 = NAME253 + b"de"

PTON = [
    b"www.example.com", b"www.example.com.", b".", b"", b"a..b", b"..", b".a",
    b"a.", b"a\\.b.c", b"\\046", b"\\065bc", b"\\256", b"\\25", b"\\2", b"a\\",
    b"a\\;b", b"a;b", b"a b", b"\\032x", LONG63, LONG64, LONG63 + b".com",
    LONG64 + b".com", NAME253, NAME254, NAME255, b"\xc3\xa9.com", b"A.B.C",
    b"a.b.c.d.e.f.g.h.i.j", b"\\", b"\\\\", b"\\.", b"x\\000y",
]
PTON_SIZES = [255, 17, 16, 5, 1, 0]

NTOP = [
    nh.name(b"www", b"example", b"com"), b"\0", nh.name(b"a.b"), nh.name(b"a;b"),
    nh.name(b"a\\b"), nh.name(b"a b"), nh.name(b"\x00"), nh.name(b"\xff"),
    nh.name(b'"q"'), nh.name(b"(x)"), nh.name(b"@"), nh.name(b"$"), nh.name(b"~"),
    nh.name(LONG63), b"\x40" + b"a" * 64 + b"\0", b"\xc0\x0c", b"\x41abc\0",
    nh.name(b"A", b"B"),
]
NTOP_SIZES = [1025, 16, 4, 2, 1]

UNPACK = [("full", 12, 1025), ("full", 33, 1025), ("full", 45, 1025), ("full", 12, 17),
          ("full", 12, 16), ("full", 12, 1), ("special", 12, 1025), ("root", 12, 1025),
          ("root", 12, 1), ("loop", 12, 1025), ("badlabel", 12, 1025),
          ("forward", 12, 1025), ("full", 60, 1025), ("cut20", 12, 1025)]

SKIP = [("full", 12, None), ("full", 33, None), ("full", 45, None), ("root", 12, None),
        ("loop", 12, None), ("badlabel", 12, None), ("full", 12, 20), ("full", 12, 29),
        ("full", 12, 28), ("forward", 12, None), ("cut20", 12, None)]

# Sequences of packs into one message: (sequence, [(how, name, dstsiz)]).
W = nh.name
PACKS = [
    ("basic", [("wire", W(b"www", b"example", b"com"), 255),
               ("wire", W(b"mail", b"example", b"com"), 255),
               ("wire", W(b"example", b"com"), 255),
               ("wire", W(b"EXAMPLE", b"COM"), 255),
               ("wire", W(b"www", b"example", b"com"), 255),
               ("wire", W(b"other", b"org"), 255),
               ("wire", b"\0", 255),
               ("wire", W(b"x", b"www", b"example", b"com"), 255)]),
    ("small", [("wire", W(b"www", b"example", b"com"), 255),
               ("wire", W(b"mail", b"example", b"com"), 6),
               ("wire", W(b"mail", b"example", b"com"), 7),
               ("wire", W(b"www", b"example", b"com"), 1),
               ("wire", W(b"www", b"example", b"com"), 2)]),
    ("text", [("text", b"www.example.com", 255),
              ("text", b"ftp.example.com.", 255),
              ("text", b"example.com", 255),
              ("text", b"a\\.b.example.com", 255),
              ("text", b".", 255),
              ("text", b"a..b", 255),
              ("text", LONG64, 255)]),
    ("full", [("wire", W(b"a", b"b"), 255), ("wire", W(b"c", b"b"), 255),
              ("wire", W(b"d", b"b"), 255), ("wire", W(b"e", b"f"), 255),
              ("wire", W(b"e", b"f"), 255), ("wire", W(b"d", b"b"), 255)]),
    ("nocomp", [("wire", W(b"www", b"example", b"com"), 255),
                ("wire", W(b"www", b"example", b"com"), 255)]),
    ("bad", [("wire", b"\x41abc\0", 255), ("wire", b"\xc0\x0c", 255),
             ("wire", W(LONG63), 255)]),
]
# The table has room for this many pointers besides the message and the NULL
# that ends it; `full` has two, to fill it.
PACK_TABLE = {"full": 2}

OK_NAMES = [
    b"www.example.com", b"-a.b", b"a-.b", b"a-b.c", b"a_b.c", b"*.example.com", b"*",
    b"a.*.b", b"*a.b", b"user.example.com", b"us er.example.com", b"a\\.b.c", b".", b"",
    b"example", b"exam ple", b"ex~ample.com", b"a..b", b"\\065.com", LONG63 + b".com",
    LONG64 + b".com", b"tab\tx", b"\xc3\xa9.com", b"a!b.c", b"a+b.c", b"first.last@x",
    b"*.", b"a.b.", b"-", b"_", b"0.9", b"a\\032b.c", b"a\\.",
]


def c_bytes(b: bytes) -> str:
    return nh.c_bytes(b)


def c_str(b: bytes) -> str:
    """`b` as a C string literal, every byte a hex escape: an escape ends at
    the next backslash, so none can swallow its neighbour."""
    return '"' + "".join(f"\\x{c:02x}" for c in b) + '"'


def token(b: bytes) -> str:
    """How a text is written in a line: the C program's `text` for bytes."""
    if not b:
        return "\\x"
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c != 0x5C else f"\\x{c:02x}" for c in b)


def c_escape(s: str) -> str:
    """`s` as the inside of a C string literal -- backslashes and quotes
    escaped -- for a line's text, which is passed as a `%s` argument and so
    needs no `%` doubled."""
    return s.replace("\\", "\\\\").replace('"', '\\"')


def c_program() -> str:
    L = [r'''#define _GNU_SOURCE
#include <arpa/nameser.h>
#include <errno.h>
#include <resolv.h>
#include <stdio.h>
#include <string.h>

static void text(const char *s) {
  putchar(' ');
  if (!*s) { printf("\\x"); return; }
  for (; *s; s++) {
    unsigned char c = (unsigned char)*s;
    if (c < 0x21 || c > 0x7e || c == '\\') printf("\\x%02x", c); else putchar(c);
  }
}
static void hex(const unsigned char *p, int n) {
  putchar(' ');
  if (n <= 0) { putchar('-'); return; }
  for (int i = 0; i < n; i++) printf("%02x", p[i]);
}
static int wirelen(const unsigned char *p, int max) {
  int i = 0;
  while (i < max) { if (p[i] == 0) return i + 1; i += 1 + p[i]; }
  return max;
}
''']
    L.append("int main(void) {")
    L.append("  unsigned char w[2048]; char t[2048]; int rc, en;")
    # pton
    for s in PTON:
        for n in PTON_SIZES:
            L.append(f"  memset(w, 0xee, sizeof w); errno = 1234; rc = ns_name_pton({c_str(s)}, w, {n}); en = errno;")
            L.append(f'  printf("T %s {n} = %d %d", "{c_escape(token(s))}", rc, en);')
            L.append(f"  hex(w, rc < 0 ? 0 : wirelen(w, {n})); putchar('\\n');")
    # ntop
    for b in NTOP:
        for n in NTOP_SIZES:
            L.append(f"  {{ static const unsigned char s[] = {c_bytes(b)};")
            L.append(f"    memset(t, 0, sizeof t); errno = 1234; rc = ns_name_ntop(s, t, {n}); en = errno;")
            L.append(f'    printf("N {b.hex()} {n} = %d %d", rc, en);')
            L.append("    if (rc < 0) printf(\" -\"); else text(t); putchar('\\n'); }")
    # unpack
    for m, o, n in UNPACK:
        b = nh.MESSAGES[m]
        L.append(f"  {{ static const unsigned char m[] = {c_bytes(b)};")
        L.append(f"    memset(w, 0xee, sizeof w); errno = 1234; rc = ns_name_unpack(m, m + {len(b)}, m + {o}, w, {n}); en = errno;")
        L.append(f'    printf("U {m} {o} {n} = %d %d", rc, en);')
        L.append(f"    hex(w, rc < 0 ? 0 : wirelen(w, {n})); putchar('\\n'); }}")
    # skip
    for m, o, e in SKIP:
        b = nh.MESSAGES[m]
        eom = len(b) if e is None else e
        L.append(f"  {{ static const unsigned char m[] = {c_bytes(b)};")
        L.append(f"    const unsigned char *p = m + {o}; errno = 1234; rc = ns_name_skip(&p, m + {eom}); en = errno;")
        L.append(f'    printf("S {m} {o} {eom} = %d %d %ld\\n", rc, en, (long)(p - m)); }}')
    # pack / compress sequences
    for seq, calls in PACKS:
        room = PACK_TABLE.get(seq, 8)
        L.append("  { unsigned char msg[1024]; memset(msg, 0, sizeof msg); int at = 12;")
        L.append(f"    const unsigned char *dn[{room + 2}]; memset(dn, 0, sizeof dn); dn[0] = msg;")
        tab = "NULL" if seq == "nocomp" else "dn"
        last = "NULL" if seq == "nocomp" else f"dn + {room + 1}"
        for i, (how, nm, n) in enumerate(calls):
            if how == "wire":
                L.append(f"    {{ static const unsigned char s[] = {c_bytes(nm)};")
                L.append(f"      errno = 1234; rc = ns_name_pack(s, msg + at, {n}, {tab}, {last}); en = errno;")
                shown = nm.hex()
            else:
                L.append(f"    {{ errno = 1234; rc = ns_name_compress({c_str(nm)}, msg + at, {n}, {tab}, {last}); en = errno;")
                shown = token(nm)
            L.append(f'      printf("P {seq} {i} {how} %s {n} = %d %d", "{c_escape(shown)}", rc, en);')
            L.append("      hex(msg + at, rc); printf(\" ;\");")
            if seq == "nocomp":
                L.append("      printf(\" -\");")
            else:
                L.append(f"      for (int k = 0; k < {room + 2} && dn[k]; k++) {{ printf(\" %ld\", (long)(dn[k] - msg)); }}")
                L.append("      printf(\" -\");")
            L.append("      putchar('\\n'); if (rc > 0) at += rc; }")
        L.append("  }")
    # the four checks
    for s in OK_NAMES:
        L.append(f'  printf("O %s = %d %d %d %d\\n", "{c_escape(token(s))}", res_hnok({c_str(s)}), '
                 f"res_ownok({c_str(s)}), res_mailok({c_str(s)}), res_dnok({c_str(s)}));")
    L.append("  return 0;\n}")
    return "\n".join(L) + "\n"


def main() -> int:
    with workdir() as d:
        c = Path(d) / "nsname.c"
        c.write_text(c_program(), encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "nsname")
        r = run(f"gcc -O1 -Wall -Werror -Wno-format-overflow -o {exe} {wsl_path(c)} && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        return 1
    header = ("# glibc 2.39's ns_name_* and res_*ok (posix/tools/oracle/nsname_harness.py)\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} lines)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
