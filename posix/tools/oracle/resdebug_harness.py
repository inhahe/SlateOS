"""glibc 2.39's `<resolv.h>` symbols and printers -- `b64_ntop`, `b64_pton`,
`sym_ntos`, `sym_ntop`, `sym_ston` over `__p_class_syms` and
`__p_type_syms`, `p_class`, `p_type`, `p_rcode`, `p_option`, `p_time`,
`loc_aton`, `loc_ntoa` -- as the oracle for posix/src/res_debug.rs:

    python posix/tools/oracle/resdebug_harness.py   # writes posix/src/resdebug_oracle.txt

One line a call:

    B <bytes> <size> = <rc> <48 bytes>           b64_ntop (the target in hex)
    D <text> <size|null> = <rc> <48 bytes|->     b64_pton (a NULL target: `null`)
    N <table> <number> = <name> <success>        sym_ntos (`class` or `type`)
    H <table> <number> = <humanname|NULL> <success>   sym_ntop
    S <table> <text> = <number> <success>        sym_ston
    C <n> = <text>, T <n> = <text>, R <n> = <text>    p_class, p_type, p_rcode
    O <option> = <text>                          p_option (the option in hex)
    M <value> = <text>                           p_time
    A <text> = <rc> <16 bytes>                   loc_aton
    Z <16 bytes> = <text>                        loc_ntoa

`<text>` is written as posix/src/resolv.rs's tests write text: a byte from
`!` to `~` as itself but `\\`, any other as `\\xHH`, the empty text `\\x`.
An answer that is text is written the same way.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "resdebug_oracle.txt"


def c_str(b: bytes) -> str:
    return '"' + "".join(f"\\x{c:02x}" for c in b) + '"'


def token(b: bytes) -> str:
    if not b:
        return "\\x"
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c != 0x5C else f"\\x{c:02x}" for c in b)


def c_escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


NTOP = [(b"", [0, 1, 8]), (b"f", [0, 3, 4, 5, 8]), (b"fo", [4, 5, 8]), (b"foo", [3, 4, 5, 8]),
        (b"foob", [7, 8, 9, 16]), (b"fooba", [8, 9, 16]), (b"foobar", [8, 9, 16]),
        (bytes(range(256))[:32], [48]), (b"\xff\xfe\xfd", [8]), (b"\x00", [8])]

PTON = [b"Zm9vYmFy", b"Zm9vYg==", b"Zm9vYmE=", b"Zg==", b"Zg=", b"Zg", b"Z", b"", b"  Zm 9v\nYmFy ",
        b"Zm9v=", b"=Zm9v", b"Zm9vYg==x", b"Zm9vYg== ", b"Zm9vYg== =", b"Zm9vYh==", b"Zm9vYmF=",
        b"Zm9v!mFy", b"Zm9vYg=\t=", b"Zm9vYg=a", b"////", b"++++", b"AAAA", b"\xffAAA",
        b"Zm9vYmFyYmF6"]
PTON_SIZES = [0, 1, 2, 3, 4, 5, 6, 48]

CLASSES = [1, 3, 4, 254, 255, 0, 2, 65535, -1, 999]
TYPES = [1, 2, 5, 6, 12, 15, 16, 28, 29, 33, 35, 36, 37, 39, 41, 43, 44, 46, 250, 251, 252, 253,
         254, 255, 0, 99, 256, 65280, -5, 99999]
RCODES = list(range(0, 24)) + [-1, 65535]
OPTIONS = [0x1, 0x2, 0x4, 0x8, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400, 0x800, 0x1000,
           0x2000, 0x4000, 0x8000, 0x10000, 0x20000, 0x40000, 0x80000, 0x100000, 0x200000,
           0x400000, 0x800000, 0x1000000, 0x2000000, 0x4000000, 0x8000000, 0x10000000,
           0x3, 0x0, 0xffffffff, 0xffffffffffffffff]
TIMES = [0, 1, 59, 60, 61, 3600, 3661, 86400, 90061, 604800, 694861, 1209600, 0x7fffffff,
         0xffffffff]
STON = [("class", b"IN"), ("class", b"in"), ("class", b"CHAOS"), ("class", b"hs"),
        ("class", b"HESIOD"), ("class", b"ANY"), ("class", b"NONE"), ("class", b"bogus"),
        ("class", b""), ("type", b"A"), ("type", b"mx"), ("type", b"AAAA"), ("type", b"SRV"),
        ("type", b"NSAP_PTR"), ("type", b"ANY"), ("type", b"bogus"), ("type", b""),
        ("type", b"DNAME"), ("type", b"cert")]

LOCS = [b"42 21 54 N 71 06 18 W -24m 30m", b"42 21 43.952 N 71 5 6.344 W -24m 1m 200m",
        b"52 22 23.000 N 4 53 32.000 E -2.00m 0.00m 10000m 10m", b"0 N 0 E 0m",
        b"90 S 180 W 0m", b"1 2 3 N 4 5 6 E 7m 8m 9m 10m", b"45 N", b"45 E 45 N 10m",
        b"45 N 45 N 0m", b"garbage", b"", b"12 34 56.7891 N 12 E 0m", b"45 N 90 E +1000.5m",
        b"45 N 90 E -100000.00m", b"45 n 90 e 5", b"45 N 90 E 1m 99999999m", b"45 N 90 E 1m 0.5m",
        b"45 N 90 E 1m 1.23456m", b"45N 90E 1m", b"45 N 90 E 1m junk", b"181 N 361 W 0m",
        b"45 30 N 90 30 W 1.5m 2m 3m 4m 5m"]

NTOAS = ["0012161300000000000000000098968" + "0",
         "00331316" "89171b40" "70aaf13c" "0098f1f8",
         "0122161380000000800000000098968" + "0",
         "0000000080000000800000000000000" + "0",
         "00ff99ff" "ffffffff" "00000000" "ffffffff",
         "00a5a5a5" "12345678" "87654321" "00989680",
         "00121613" "a0a0a0a0" "5f5f5f5f" "01000000"]


def c_program() -> str:
    L = [r'''#define _GNU_SOURCE
#include <arpa/nameser.h>
#include <errno.h>
#include <resolv.h>
#include <stdio.h>
#include <string.h>

extern const struct res_sym __p_class_syms[];
extern const struct res_sym __p_type_syms[];

static void hex(const unsigned char *b, size_t n)
{
    for (size_t i = 0; i < n; i++)
        printf("%02x", b[i]);
}

/* A text as the lines write it. */
static void text(const char *s)
{
    if (s == NULL) {
        fputs("NULL", stdout);
        return;
    }
    size_t n = strlen(s);
    if (n == 0) {
        fputs("\\x", stdout);
        return;
    }
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char) s[i];
        if (c >= 0x21 && c <= 0x7e && c != '\\')
            putchar(c);
        else
            printf("\\x%02x", c);
    }
}

static void ntop(const char *h, const unsigned char *b, size_t n, size_t size)
{
    unsigned char out[48];
    memset(out, 0xaa, sizeof out);
    int r = b64_ntop(b, n, (char *) out, size);
    printf("B %s %zu = %d ", h, size, r);
    hex(out, sizeof out);
    putchar('\n');
}

static void pton(const char *t, const char *s, long size)
{
    unsigned char out[48];
    memset(out, 0xaa, sizeof out);
    int r = b64_pton(s, size < 0 ? NULL : out, size < 0 ? 0 : (size_t) size);
    printf("D %s ", t);
    if (size < 0)
        printf("null = %d -\n", r);
    else {
        printf("%ld = %d ", size, r);
        hex(out, sizeof out);
        putchar('\n');
    }
}

static const struct res_sym *table(const char *which)
{
    return which[0] == 'c' ? __p_class_syms : __p_type_syms;
}

int main(void)
{
''']
    for b, sizes in NTOP:
        h = b.hex() or "-"
        for size in sizes:
            L.append(f'    ntop("{h}", (const unsigned char *) {c_str(b or b"x")}, {len(b)}, {size});')
    for s in PTON:
        t = c_escape(token(s))
        for size in PTON_SIZES:
            L.append(f'    pton("{t}", {c_str(s)}, {size});')
        L.append(f'    pton("{t}", {c_str(s)}, -1);')
    for which, nums in (("class", CLASSES), ("type", TYPES)):
        for n in nums:
            L.append(f'    {{ int ok = 7; const char *r = sym_ntos(table("{which}"), {n}, &ok); '
                     f'printf("N {which} {n} = "); text(r); printf(" %d\\n", ok); }}')
            L.append(f'    {{ int ok = 7; const char *r = sym_ntop(table("{which}"), {n}, &ok); '
                     f'printf("H {which} {n} = "); text(r); printf(" %d\\n", ok); }}')
    for which, s in STON:
        t = c_escape(token(s))
        L.append(f'    {{ int ok = 7; int r = sym_ston(table("{which}"), {c_str(s)}, &ok); '
                 f'printf("S {which} {t} = %d %d\\n", r, ok); }}')
    for n in CLASSES:
        L.append(f'    printf("C {n} = "); text(p_class({n})); putchar(\'\\n\');')
    for n in TYPES:
        L.append(f'    printf("T {n} = "); text(p_type({n})); putchar(\'\\n\');')
    for n in RCODES:
        L.append(f'    printf("R {n} = "); text(p_rcode({n})); putchar(\'\\n\');')
    for o in OPTIONS:
        L.append(f'    printf("O {o:x} = "); text(p_option(0x{o:x}UL)); putchar(\'\\n\');')
    for t in TIMES:
        L.append(f'    printf("M {t} = "); text(p_time(0x{t:x}u)); putchar(\'\\n\');')
    for s in LOCS:
        t = c_escape(token(s))
        L.append(f'    {{ unsigned char o[16]; memset(o, 0xaa, sizeof o); int r = loc_aton({c_str(s)}, o); '
                 f'printf("A {t} = %d ", r); hex(o, 16); putchar(\'\\n\'); }}')
    for h in NTOAS:
        b = bytes.fromhex(h)
        assert len(b) == 16, h
        L.append(f'    {{ char o[128]; printf("Z {h} = "); '
                 f'text(loc_ntoa((const unsigned char *) {c_str(b)}, o)); putchar(\'\\n\'); }}')
    L.append("    return 0;")
    L.append("}")
    return "\n".join(L) + "\n"


def main() -> int:
    with workdir() as d:
        c = Path(d) / "resdebug.c"
        c.write_text(c_program(), encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "resdebug")
        # -lresolv: libresolv's. -Wno-deprecated-declarations: glibc
        # deprecates most of these, as posix/include does too.
        r = run(f"gcc -O1 -Wall -Werror -Wno-deprecated-declarations -o {exe} {wsl_path(c)} "
                f"-lresolv && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        return 1
    header = ("# glibc 2.39's b64_ntop, b64_pton, sym_ntos, sym_ntop, sym_ston, p_class,\n"
              "# p_type, p_rcode, p_option, p_time, loc_aton and loc_ntoa\n"
              "# (posix/tools/oracle/resdebug_harness.py): one line a call -- see the\n"
              "# harness for the forms.\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} calls)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
