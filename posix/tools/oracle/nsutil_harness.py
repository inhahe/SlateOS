"""glibc 2.39's `<arpa/nameser.h>` utilities -- names compared and made
canonical, labels lowered, the compression table rolled back, the header's
flags, TTLs and dates -- as the oracle for posix/src/nameser.rs and
posix/src/resolv.rs:

    python posix/tools/oracle/nsutil_harness.py   # writes posix/src/nsutil_oracle.txt

One line a call:

    K <text> <size> = <rc> <errno> <text written>     ns_makecanon
    M <a> <b> = <rc>                                   ns_samename
    D <a> <b> = <rc>                                   ns_samedomain
    U <a> <b> = <rc>                                   ns_subdomain
    G <flags> <flag> = <value>                         ns_msg_getflag (flags in hex)
    L <wire> <size> = <rc> <errno> <32 bytes>          ns_name_ntol
    R <entries> <src> <last> = <entries>               ns_name_rollback
    F <ttl> <size> = <rc> <errno> <24 bytes>           ns_format_ttl
    P <text> = <rc> <errno> <ttl>                      ns_parse_ttl
    S <text> = <secs> <err>                            ns_datetosecs

`<text>`, `<a>` and `<b>` are written as posix/src/resolv.rs's tests write
text: a byte from `!` to `~` as itself but `\\`, any other as `\\xHH`, the
empty text `\\x`; a text of `x*N` is N `x`s. `<wire>` is hex. Output buffers
start as 0xaa bytes and are printed whole, so that what a failing call wrote
is compared too. For ns_name_rollback, `<entries>` are the pointer table's,
as offsets into one message, `-` for NULL, comma-separated; `<src>` is an
offset and `<last>` how many entries the table's end is past its start.
errno is `0` where a call succeeded or does not set it.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "nsutil_oracle.txt"


def c_str(b: bytes) -> str:
    """`b` as a C string literal, every byte a hex escape."""
    return '"' + "".join(f"\\x{c:02x}" for c in b) + '"'


def token(b: bytes) -> str:
    """How a text is written in a line."""
    if not b:
        return "\\x"
    if len(b) > 40 and b == b"x" * len(b):
        return f"x*{len(b)}"
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c != 0x5C else f"\\x{c:02x}" for c in b)


def c_escape(s: str) -> str:
    """`s` as the inside of a C string literal, for a `%s` argument."""
    return s.replace("\\", "\\\\").replace('"', '\\"')


BS = b"\\"

CANON = [b"foo", b"foo.", b"foo..", b"foo...", b"foo" + BS + b".", b"foo" + BS + BS + b".",
         b"foo" + BS + BS + BS + b".", b".", b"..", b"", b"a" + BS, BS, BS + b".",
         BS + BS + b".", b"a.b.c", b"a.b.c.", b"x" * 300, b"x" * 1024, b"x" * 1023]
CANON_SIZES = [0, 1, 2, 3, 4, 5, 6, 256, 1025, 2048]

SAME = [
    (b"foo", b"FOO."), (b"foo.", b"foo.."), (b"foo" + BS + b".", b"foo."),
    (b"foo" + BS + b".", b"foo" + BS + b".."), (b"", b"."), (b"", b""), (b".", b".."),
    (b"a", b"b"), (b"a.b", b"A.B."), (b"a.b", b"a.b.c"), (b"x" * 1024, b"x" * 1024),
    (b"x" * 1023, b"x" * 1023), (b"x" * 1023, b"X" * 1023), (b"\xe9", b"\xc9"), (b"a\xc9", b"A\xc9"),
]

DOMAIN = [
    (b"host.foobar.top", b"foobar.top"), (b"host.foobar.top", b"bar.top"),
    (b"host.foobar.top", b"top"), (b"host.foobar.top", b""), (b"host.foobar.top", b"."),
    (b"host.foobar.top.", b"foobar.top."), (b"foobar.top", b"foobar.top"),
    (b"FooBar.Top", b"foobar.top."), (b"a.b", b"B"), (b"b", b"b."), (b"b.", b"b"),
    (b"ab", b"b"), (b".b", b"b"), (b"a", b"a.b"), (b"", b""), (b"", b"a"), (b".", b""),
    (b".", b"."), (b"..", b"."), (b"x" + BS + b".b", b"b"), (b"x" + BS + BS + b".b", b"b"),
    (b"x" + BS + BS + BS + b".b", b"b"), (b"a" + BS + b".", b"a" + BS + b"."),
    (b"a" + BS + b".", b"a"), (b"b" + BS + b".", b""), (b"a.b" + BS + BS + b".", b"b" + BS + BS),
    (b"q.a.b" + BS + b".", b"b" + BS + b"."), (b"\xe9.b", b"b"), (b"a.\xe9", b"\xc9"),
]

SUB = DOMAIN + [(b"foo.", b"foo"), (b"foo", b"foo"), (b"a.foo", b"FOO."), (b"x" * 1024, b"x")]

FLAGS = [0x0000, 0x8180, 0xFFFF, 0x7800, 0x0105, 0x8583]

NTOL = [
    ("03464f4f03634f6d00", [0, 1, 4, 5, 8, 9, 10, 32]), ("00", [0, 1, 2]),
    ("014100", [2, 3, 4]), ("03666f6f00", [32]), ("03464f4fc00c", [32]), ("c00c", [32]),
    ("40" + "41" * 64 + "00", [32]), ("3f" + "41" * 63 + "00", [32]),
    ("0441c9e97a00", [32]), ("0341424380", [32]), ("02414203626300", [4, 5, 6, 7]),
]

# (entries, src, last): entries as offsets or None.
ROLLBACK = [
    ([0, 12, 30, 50, None], 30, 5), ([0, 12, 30, 50, None], 31, 5), ([0, 12, 30, 50, None], 0, 5),
    ([0, 12, 30, 50, None], 60, 5), ([0, 12, 30, 50, None], 30, 2), ([0, 12, 30, 50, None], 30, 0),
    ([None, 12, 30], 12, 3), ([40, 12, 30, None], 20, 4), ([0, 12, 30, 50, 60], 55, 5),
]

TTLS = [0, 1, 59, 60, 61, 3599, 3600, 3661, 86400, 86401, 90061, 604800, 604801, 694861,
        1209600, 0xFFFFFFFF, 1 << 40, (1 << 63) - 1, 1 << 63, (1 << 64) - 1]
TTL_SIZES = [0, 1, 2, 3, 4, 5, 6, 8, 10, 24]

PARSE = [b"1W", b"1w2d3h4m5s", b"3600", b"1H30", b"30M1", b"", b"W", b"1X", b"1 ", b"0",
         b"99999999999999999999", b"18446744073709551615", b"18446744073709551616", b"1h1h",
         b"5s", b"1W1W", b"2d", b"1W2", b"12m", b"1\x7f", b"1\xe9", b"1s2", b"s1", b"007H",
         b"4294967296S", b"30500000000000000000w"]

DATES = [b"19900101000000", b"20260930123456", b"99991231235959", b"19891231235959",
         b"2026093012345", b"202609301234567", b"20261301000000", b"20260001000000",
         b"20260100000000", b"20260132000000", b"20260101240000", b"20260101006000",
         b"20260101000060", b"2026a930123456", b"20240229000000", b"21000301000000",
         b"20000301000000", b"20380119031408", b"21060207062816", b"21060207062815",
         b"20260230000000", b"", b"2026-9-30 1234"]


def c_program() -> str:
    L = [r'''#define _GNU_SOURCE
#include <arpa/nameser.h>
#include <errno.h>
#include <resolv.h>
#include <stdio.h>
#include <string.h>

static const char *name(int e)
{
    switch (e) {
    case 0: return "0";
    case EMSGSIZE: return "EMSGSIZE";
    case EINVAL: return "EINVAL";
    default: return "other";
    }
}

static void hex(const unsigned char *b, size_t n)
{
    for (size_t i = 0; i < n; i++)
        printf("%02x", b[i]);
}

/* A text as the lines write it. */
static void text(const char *s)
{
    size_t n = strlen(s), i;
    if (n == 0) {
        fputs("\\x", stdout);
        return;
    }
    for (i = 0; i < n && s[i] == 'x'; i++)
        ;
    if (n > 40 && i == n) {
        printf("x*%zu", n);
        return;
    }
    for (i = 0; i < n; i++) {
        unsigned char c = (unsigned char) s[i];
        if (c >= 0x21 && c <= 0x7e && c != '\\')
            putchar(c);
        else
            printf("\\x%02x", c);
    }
}

static char big[4096];

static const char *xs(size_t n, char c)
{
    static char b[2][2048];
    static int k;
    char *p = b[k ^= 1];
    memset(p, c, n);
    p[n] = '\0';
    return p;
}

static void canon(const char *s, size_t size)
{
    memset(big, 0xaa, sizeof big);
    errno = 0;
    int r = ns_makecanon(s, big, size);
    printf("K ");
    text(s);
    printf(" %zu = %d %s ", size, r, name(r < 0 ? errno : 0));
    if (r == 0)
        text(big);
    else
        fputs("-", stdout);
    putchar('\n');
}

static void pair(const char *what, int r, const char *a, const char *b)
{
    printf("%s ", what);
    text(a);
    putchar(' ');
    text(b);
    printf(" = %d\n", r);
}

static void ntol(const char *wirehex, const unsigned char *wire, size_t size)
{
    unsigned char out[32];
    memset(out, 0xaa, sizeof out);
    errno = 0;
    int r = ns_name_ntol(wire, out, size);
    printf("L %s %zu = %d %s ", wirehex, size, r, name(r < 0 ? errno : 0));
    hex(out, sizeof out);
    putchar('\n');
}

static unsigned char msg[64];

static void rollback(const char *entries, const int *offs, int n, int src, int last)
{
    const unsigned char *t[8];
    int i;
    for (i = 0; i < n; i++)
        t[i] = offs[i] < 0 ? NULL : msg + offs[i];
    ns_name_rollback(msg + src, t, t + last);
    printf("R %s %d %d = ", entries, src, last);
    for (i = 0; i < n; i++) {
        if (i)
            putchar(',');
        if (t[i] == NULL)
            putchar('-');
        else
            printf("%d", (int) (t[i] - msg));
    }
    putchar('\n');
}

static void fttl(unsigned long ttl, size_t size)
{
    char out[24];
    memset(out, 0xaa, sizeof out);
    errno = 0;
    int r = ns_format_ttl(ttl, out, size);
    printf("F %lu %zu = %d %s ", ttl, size, r, name(r < 0 ? errno : 0));
    hex((unsigned char *) out, sizeof out);
    putchar('\n');
}

static void pttl(const char *s)
{
    unsigned long v = 0xaaaaaaaaaaaaaaaaUL;
    errno = 0;
    int r = ns_parse_ttl(s, &v);
    printf("P ");
    text(s);
    printf(" = %d %s %lu\n", r, name(r < 0 ? errno : 0), v);
}

static void date(const char *s)
{
    int err = 7;
    unsigned int v = ns_datetosecs(s, &err);
    printf("S ");
    text(s);
    printf(" = %u %d\n", v, err);
}

int main(void)
{
''']

    def arg(b: bytes) -> str:
        if len(b) > 40 and set(b) <= {ord("x")}:
            return f"xs({len(b)}, 'x')"
        if len(b) > 40 and set(b) <= {ord("X")}:
            return f"xs({len(b)}, 'X')"
        return c_str(b)

    for s in CANON:
        for size in CANON_SIZES:
            L.append(f"    canon({arg(s)}, {size});")
    for a, b in SAME:
        L.append(f'    {{ const char *a = {arg(a)}; const char *b = {arg(b)}; pair("M", ns_samename(a, b), a, b); }}')
    for a, b in DOMAIN:
        L.append(f'    {{ const char *a = {arg(a)}; const char *b = {arg(b)}; pair("D", ns_samedomain(a, b), a, b); }}')
    for a, b in SUB:
        L.append(f'    {{ const char *a = {arg(a)}; const char *b = {arg(b)}; pair("U", ns_subdomain(a, b), a, b); }}')
    L.append("    {")
    L.append("        ns_msg h;")
    L.append("        memset(&h, 0, sizeof h);")
    for fl in FLAGS:
        for flag in range(16):
            L.append(f"        h._flags = 0x{fl:04x}; "
                     f"printf(\"G {fl:04x} {flag} = %d\\n\", (ns_msg_getflag)(h, {flag}));")
    L.append("    }")
    for wire, sizes in NTOL:
        for size in sizes:
            L.append(f'    ntol("{wire}", (const unsigned char *) {c_str(bytes.fromhex(wire))}, '
                     f'{size});')
    for entries, src, last in ROLLBACK:
        shown = ",".join("-" if e is None else str(e) for e in entries)
        offs = ", ".join("-1" if e is None else str(e) for e in entries)
        L.append(f'    {{ static const int o[] = {{{offs}}}; '
                 f'rollback("{shown}", o, {len(entries)}, {src}, {last}); }}')
    for ttl in TTLS:
        for size in TTL_SIZES:
            L.append(f"    fttl({ttl}UL, {size});")
    for s in PARSE:
        L.append(f"    pttl({arg(s)});")
    for s in DATES:
        L.append(f"    date({arg(s)});")
    L.append("    return 0;")
    L.append("}")
    return "\n".join(L) + "\n"


def main() -> int:
    with workdir() as d:
        c = Path(d) / "nsutil.c"
        c.write_text(c_program(), encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "nsutil")
        # -lresolv: libresolv's, all of them. -Wno-deprecated-declarations:
        # glibc deprecates all but ns_msg_getflag, ns_name_ntol and
        # ns_name_rollback, as posix/include does too.
        r = run(f"gcc -O1 -Wall -Werror -Wno-deprecated-declarations -o {exe} {wsl_path(c)} "
                f"-lresolv && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        return 1
    header = ("# glibc 2.39's ns_makecanon, ns_samename, ns_samedomain, ns_subdomain,\n"
              "# ns_msg_getflag, ns_name_ntol, ns_name_rollback, ns_format_ttl, ns_parse_ttl\n"
              "# and ns_datetosecs (posix/tools/oracle/nsutil_harness.py): one line a call --\n"
              "# see the harness for the forms.\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} calls)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
