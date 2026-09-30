"""glibc 2.39's network numbers and NSAP addresses (`<arpa/inet.h>`:
`inet_net_pton`, `inet_net_ntop`, `inet_neta`, `inet_nsap_addr`,
`inet_nsap_ntoa` -- libresolv's in glibc, libc.a's here), as the oracle for
posix/src/inet.rs's:

    python posix/tools/oracle/inetnet_harness.py   # writes posix/src/inetnet_oracle.txt

One line a call. Every output buffer starts as 0xaa bytes and is printed
whole, in hex, so that what a failing call wrote before it failed is
compared too:

    P <af> <text> <size> = <rc> <errno> <16 bytes>       inet_net_pton
    N <af> <4 bytes> <bits> <size> = <ok|NULL> <errno> <24 bytes>
                                                         inet_net_ntop
    A <net> <size> = <ok|NULL> <errno> <24 bytes>        inet_neta (net in hex)
    S <text> <maxlen> = <rc> <40 bytes>                  inet_nsap_addr
    T <bytes> <binlen> <buf|null> = <ok|other> <text>    inet_nsap_ntoa

`<text>` is written as posix/src/resolv.rs's tests write it: a byte from
`!` to `~` as itself but `\\`, any other as `\\xHH`, the empty text `\\x`.
`<bytes>` is hex, or `seq<n>` for n bytes `i*7+3`. `ok` for inet_nsap_ntoa
is its answer being the buffer it was given, or with none its own.

The sizes stop short of what the destination holds: every write these
calls make is bounded by the size they are given.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "inetnet_oracle.txt"


def c_str(b: bytes) -> str:
    """`b` as a C string literal, every byte a hex escape (as nsname_harness.py)."""
    return '"' + "".join(f"\\x{c:02x}" for c in b) + '"'


def token(b: bytes) -> str:
    """How a text is written in a line."""
    if not b:
        return "\\x"
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c != 0x5C else f"\\x{c:02x}" for c in b)


def c_escape(s: str) -> str:
    """`s` as the inside of a C string literal, for a `%s` argument."""
    return s.replace("\\", "\\\\").replace('"', '\\"')


PTON = [
    b"10", b"10.0", b"10.1.2.3", b"192.168.1", b"192.168.1.0/24", b"192.168.1.0/16",
    b"192.168.1.0/33", b"192.168.1.0/32", b"10/8", b"10/", b"10/x", b"10/08", b"10/-1",
    b"1.2.3.4/32", b"1.2.3.4/0", b"1.2.3.4/0x10", b"128.1", b"128", b"192", b"224",
    b"224.1.2.3", b"239.255", b"240", b"240.1", b"255.255.255.255", b"256", b"1.256",
    b"01.02", b"0", b"0.0.0.0/0", b"0/0", b"0x0a", b"0x0a0b", b"0xa", b"0x", b"0X1",
    b"0x1g", b"0x123", b"0x12345678", b"0x123456789a", b"0x0a/8", b"0x0a/7", b"0x0a/",
    b"0xA0/3", b"1.2.3.4.5", b"1..2", b"1.", b".1", b"", b"a", b" 1", b"1 ", b"1.2 ",
    b"12345", b"1/4294967296", b"1/2147483648", b"1/4294967295", b"1/99", b"10.0.0.0/8/8",
    b"\xff", b"1\xff",
]

# (text, sizes) for the destination-size edges.
PTON_SIZES = [
    (b"10.1.2.3", [0, 1, 2, 3, 4]),
    (b"192.168.1.0/24", [2, 3]),
    (b"10/32", [1, 2, 3, 4]),
    (b"0x0a0b0c", [0, 1, 2, 3]),
    (b"0x0a0", [1, 2]),
    (b"0", [0, 1]),
    (b"1.2.3.4.5", [4, 5]),
    (b"224.1.2.3", [4]),
]

NTOP_BITS = [-1, 0, 1, 4, 7, 8, 9, 15, 16, 17, 23, 24, 25, 31, 32, 33]
NTOP_SIZES = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 16, 17, 18, 20]

NETA = [0x0, 0xA, 0xA00, 0xA0000, 0xA000000, 0xA000001, 0xC0A80100, 0xFFFFFFFF, 0x1020304,
        0x80000000, 0x1, 0x10001, 0x7F000001]
NETA_SIZES = [0, 1, 4, 5, 6, 7, 8, 9, 10, 12, 16]

NSAP = [
    (b"47.0005.80ffff00", 40), (b"4700", 40), (b"47", 40), (b"4", 40), (b"", 40), (b"4x", 40),
    (b"0x47", 40), (b"47+00/11", 40), (b"a1B2", 40), (b"\xff", 40), (b"47 ", 40),
    (b" 47", 40), (b"4.7", 40), (b"..", 40), (b"+/.", 40), (b"abcdef0123456789", 40),
    (b"ABCDEFG1", 40), (b"47.00", 1), (b"47.00", 0), (b"47.00.11", 2), (b"47.00.11", -1),
    (b"4700", -5), (b"47\xe9", 40), (b"g0", 40), (b"0g", 40),
]

NTOA = [
    ("", 0), ("47", 1), ("4700", 2), ("470005", 3), ("47000580", 4), ("47000580ff", 5),
    ("0a1b2c3d4e5f", 6), ("seq255", 255), ("seq256", 256), ("seq300", 300), ("47", -1),
]


def c_program() -> str:
    L = [r'''#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>

static const char *name(int e)
{
    switch (e) {
    case 0: return "0";
    case ENOENT: return "ENOENT";
    case EMSGSIZE: return "EMSGSIZE";
    case EINVAL: return "EINVAL";
    case EAFNOSUPPORT: return "EAFNOSUPPORT";
    default: return "other";
    }
}

static void hex(const unsigned char *b, size_t n)
{
    for (size_t i = 0; i < n; i++)
        printf("%02x", b[i]);
}

static void pton(int af, const char *text, const char *s, size_t size)
{
    unsigned char buf[16];
    memset(buf, 0xaa, sizeof buf);
    errno = 0;
    int r = inet_net_pton(af, s, buf, size);
    printf("P %d %s %zu = %d %s ", af, text, size, r, name(r == -1 ? errno : 0));
    hex(buf, sizeof buf);
    putchar('\n');
}

static void ntop(int af, const unsigned char *src, int bits, size_t size)
{
    char buf[24];
    memset(buf, 0xaa, sizeof buf);
    errno = 0;
    char *r = inet_net_ntop(af, src, bits, buf, size);
    printf("N %d ", af);
    hex(src, 4);
    printf(" %d %zu = %s %s ", bits, size, r == NULL ? "NULL" : r == buf ? "ok" : "other",
           name(r == NULL ? errno : 0));
    hex((unsigned char *) buf, sizeof buf);
    putchar('\n');
}

static void neta(unsigned int net, size_t size)
{
    char buf[24];
    memset(buf, 0xaa, sizeof buf);
    errno = 0;
    char *r = inet_neta(net, buf, size);
    printf("A %x %zu = %s %s ", net, size, r == NULL ? "NULL" : r == buf ? "ok" : "other",
           name(r == NULL ? errno : 0));
    hex((unsigned char *) buf, sizeof buf);
    putchar('\n');
}

static void nsap(const char *text, const char *s, int maxlen)
{
    unsigned char buf[40];
    memset(buf, 0xaa, sizeof buf);
    unsigned int r = inet_nsap_addr(s, buf, maxlen);
    printf("S %s %d = %u ", text, maxlen, r);
    hex(buf, sizeof buf);
    putchar('\n');
}

static void ntoa(const char *what, const unsigned char *b, int binlen, int own)
{
    static char buf[1024];
    memset(buf, 0xaa, sizeof buf);
    char *r = inet_nsap_ntoa(binlen, b, own ? buf : NULL);
    const char *ok = own ? (r == buf ? "ok" : "other") : (r != NULL && r != buf ? "ok" : "other");
    printf("T %s %d %s = %s %s\n", what, binlen, own ? "buf" : "null", ok, r);
}

int main(void)
{
    static unsigned char seq[300];
    for (int i = 0; i < 300; i++)
        seq[i] = (unsigned char) (i * 7 + 3);
''']
    for b in PTON:
        L.append(f'    pton(AF_INET, "{c_escape(token(b))}", {c_str(b)}, 16);')
    for b, sizes in PTON_SIZES:
        for size in sizes:
            L.append(f'    pton(AF_INET, "{c_escape(token(b))}", {c_str(b)}, {size});')
    L.append(f'    pton(AF_INET6, "{c_escape(token(b"::1"))}", {c_str(b"::1")}, 16);')
    L.append(f'    pton(0, "{c_escape(token(b"10"))}", {c_str(b"10")}, 16);')
    L.append('    {')
    L.append('        static const unsigned char a[4] = {192, 168, 1, 128};')
    L.append('        static const unsigned char z[4] = {0, 0, 0, 0};')
    L.append('        static const unsigned char f[4] = {255, 255, 255, 255};')
    L.append('        static const unsigned char t[4] = {10, 0, 0, 1};')
    for bits in NTOP_BITS:
        L.append(f'        ntop(AF_INET, a, {bits}, 24);')
    for bits in (0, 8, 20, 32):
        for size in NTOP_SIZES:
            L.append(f'        ntop(AF_INET, a, {bits}, {size});')
    for src in ("z", "f", "t"):
        for bits in (8, 24, 32):
            L.append(f'        ntop(AF_INET, {src}, {bits}, 24);')
    L.append('        ntop(AF_INET6, a, 24, 24);')
    L.append('        ntop(0, a, 24, 24);')
    L.append('    }')
    for net in NETA:
        L.append(f'    neta(0x{net:x}u, 24);')
    for net in (0x0, 0xA000001, 0xFFFFFFFF, 0xC0A80100):
        for size in NETA_SIZES:
            L.append(f'    neta(0x{net:x}u, {size});')
    for b, maxlen in NSAP:
        L.append(f'    nsap("{c_escape(token(b))}", {c_str(b)}, {maxlen});')
    for what, binlen in NTOA:
        if what.startswith("seq"):
            src = "seq"
        else:
            src = c_str(bytes.fromhex(what) or b"\x00")
            src = f"(const unsigned char *) {src}"
        shown = what or "-"
        for own in (1, 0):
            L.append(f'    ntoa("{shown}", {src}, {binlen}, {own});')
    L.append("    return 0;")
    L.append("}")
    return "\n".join(L) + "\n"


def main() -> int:
    with workdir() as d:
        c = Path(d) / "inetnet.c"
        c.write_text(c_program(), encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "inetnet")
        # -lresolv: glibc's inet_net_pton, inet_net_ntop and inet_neta are
        # libresolv's. -Wno-deprecated-declarations: its inet_neta is
        # deprecated ("Use inet_ntop instead"), as the overlay's is too.
        r = run(f"gcc -O1 -Wall -Werror -Wno-deprecated-declarations -o {exe} {wsl_path(c)} "
                f"-lresolv && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        return 1
    header = ("# glibc 2.39's inet_net_pton, inet_net_ntop, inet_neta, inet_nsap_addr and\n"
              "# inet_nsap_ntoa (posix/tools/oracle/inetnet_harness.py): one line a call, its\n"
              "# answer, errno, and the output buffer in hex -- see the harness for the forms.\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} calls)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
