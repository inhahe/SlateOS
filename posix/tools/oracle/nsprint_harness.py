"""glibc's record printers, `ns_sprintrrf` and `ns_sprintrr`
(`<arpa/nameser.h>`), as the oracle for posix/src/nameser.rs's:

    python posix/tools/oracle/nsprint_harness.py   # writes posix/src/nsprint_oracle.txt

The glibc answering is the WSL oracle's, Ubuntu's 2.39-0ubuntu8.9, which
carries glibc's June 2026 fixes to ns_print.c -- bug 34289 (CLASSn and TYPEn
for a class or type with no name), CVE-2026-5435 (CERT, TKEY, TSIG and OPT
printed as unknown records) and CVE-2026-6238 (the LOC and A6 overreads) --
so these are the fixed printer's answers, not the 2.39 tarball's.

One line a call:

    F <type> <class> <ttl> <name> <rdata> <ctx> <origin> <size> = <rc> <errno> <text>
          ns_sprintrrf, the record's data its own message (so its names
          expand within it); `<ctx>` and `<origin>` `-` for NULL. `<rdata>`
          may be `<data>+<more>`: the message is both, the data the first
          (so a name in it can run on into the rest of the message)
    R <message> <section> <n> <ctx> <origin> <size> = <rc> <errno> <text>
          ns_sprintrr on the message's record n of the section, as
          ns_parserr gives it
    S <type> <rdata> = <rc> <errno> <text> | <at each size> | <each truncation>
          glibc's own test of the fixes, resolv/tst-ns_sprintrr.c: the
          record the one answer of a response to www.example.org/IN/ANY,
          printed by ns_sprintrr (no ctx, no origin) into 4096 bytes; then
          into each size from 1 to the text's length + 16; then with its
          data cut to each length from 0 to all of it (the message ending
          where the data does), into 4096 bytes. Each of the latter is
          `<rc>/<errno>/<k>`, `<k>` the bytes the buffer's text shares with
          the whole text, and `/<rest>` after it when the text goes on

`<name>`, `<ctx>`, `<origin>`, `<text>` and `<rest>` are written as
posix/src/resolv.rs's tests write text (a byte from `!` to `~` as itself but
`\\`, any other as `\\xHH`, the empty text `\\x`); `<rdata>` and `<message>`
are hex (`-` for none). errno is `0` where the call left it so. Every
buffer is zeroed first, so the text is what the call wrote, up to a NUL.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "nsprint_oracle.txt"


def c_str(b: bytes) -> str:
    return '"' + "".join(f"\\x{c:02x}" for c in b) + '"'


def token(b: bytes) -> str:
    if not b:
        return "\\x"
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c != 0x5C else f"\\x{c:02x}" for c in b)


def c_escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def wire(name: str) -> bytes:
    out = b""
    for label in [x for x in name.split(".") if x]:
        out += bytes([len(label)]) + label.encode()
    return out + b"\x00"


def cs(s: bytes) -> bytes:
    """A <character-string>."""
    return bytes([len(s)]) + s


def u16(v: int) -> bytes:
    return v.to_bytes(2, "big")


def u32(v: int) -> bytes:
    return v.to_bytes(4, "big")


# (type, rdata)
RECORDS = [
    (1, bytes([192, 0, 2, 1])), (1, bytes([192, 0, 2])),
    (28, bytes.fromhex("20010db8000000000000000000000001")), (28, bytes(15)),
    (5, wire("target.example.com")), (2, wire("ns1.example.com")), (12, wire("host.example.com")),
    (39, wire("new.example.net")), (7, wire("mb.example.com")), (8, wire("mg.example.com")),
    (9, wire("mr.example.com")), (5, wire("")),
    (13, cs(b"PDP-11") + cs(b"UNIX")), (13, cs(b"PDP-11")), (13, cs(b"x") + cs(b"yz") + b"!"),
    (13, b"\x05ab"), (20, cs(b"150862028003217")), (20, cs(b"150862028003217") + cs(b"004")),
    (6, wire("ns.example.com") + wire("admin.example.com") + u32(2026093001) + u32(7200)
     + u32(3600) + u32(1209600) + u32(86400)),
    (6, wire("ns.example.com") + wire("admin.example.com") + u32(1)),
    (15, u16(10) + wire("mail.example.com")), (15, b"\x00"), (18, u16(1) + wire("afs.example.com")),
    (21, u16(20) + wire("rt.example.com")), (26, u16(5) + wire("a.example") + wire("b.example")),
    (19, cs(b"311061700956")), (16, cs(b"hello world")),
    (16, cs(b"a \"quoted\" \\ back") + cs(b"line\nbreak") + cs(b"")), (16, b""), (16, b"\x03ab"),
    (22, bytes.fromhex("47000580ffff000000")), (29, bytes.fromhex("00121613800000008000000000989680")),
    (29, bytes.fromhex("0012161380000000800000000098968000")), (29, bytes.fromhex("001216138000")),
    (35, u16(100) + u16(10) + cs(b"u") + cs(b"E2U+sip") + cs(b"!^.*$!sip:x@example.com!") + wire("")),
    (35, u16(100) + u16(10) + cs(b"")), (35, u16(1)),
    (33, u16(10) + u16(60) + u16(5060) + wire("sip.example.com")), (33, u16(1) + u16(2)),
    (14, wire("rmail.example.com") + wire("email.example.com")),
    (17, wire("admin.example.com") + wire("txt.example.com")),
    (11, bytes([192, 0, 2, 1]) + bytes([6]) + bytes([0x40, 0x00, 0x00, 0x01])),
    (11, bytes([192, 0, 2, 1]) + bytes([17]) + bytes([0xff] * 3)), (11, bytes([192, 0, 2, 1])),
    (37, u16(1) + u16(12345) + bytes([5]) + bytes(range(12))),
    (37, u16(1) + u16(12345) + bytes([5]) + bytes(range(60))),
    (37, u16(2) + u16(1) + bytes([8])),
    (249, wire("hmac-md5.sig-alg.reg.int") + u32(1000) + u32(2000) + u16(3) + u16(0) + u16(16)),
    (250, wire("hmac-sha256") + bytes(6) + u16(300) + u16(4) + b"MACX" + u16(0x1234) + u16(17)),
    (38, bytes([0]) + bytes.fromhex("20010db8000000000000000000000001")),
    (38, bytes([64]) + bytes(8) + wire("prefix.example")), (38, bytes([128]) + wire("p.example")),
    (38, bytes([129])), (38, b""), (38, bytes([64]) + bytes(8)), (38, bytes([64]) + bytes(7)),
    (38, bytes([0]) + bytes(15)), (38, bytes([1]) + bytes(16)),
    (41, b""), (99, bytes(range(40))), (99, b""), (99, b"ab\x00\xff ~"), (10, b"\x01"),
]

# (type, rdata, the message after it): names that run on past the data.
RUNNING_ON = [
    # SOA: the second name ends 16 bytes past the data, so the data left
    # is -16 -- a format error, its count as C's (unsigned) shows it.
    (6, wire("ns.example.com") + b"\x05ad",
     b"min" + wire("example.com") + u32(1) + u32(2) + u32(3) + u32(4) + u32(5)),
    # PX: the first name's end is past the data, the second all of it.
    (26, u16(10) + b"\x01a", b"\x00" + wire("b.example")),
    (14, b"\x05rmail", wire("example.com") + wire("x")),
    (5, b"\x06target", wire("example.com")),
]

OWNERS = [
    ("www.example.com", None, None), ("www.example.com.", None, None),
    ("www.example.com", "www.example.com", None), ("www.example.com", None, "example.com"),
    ("example.com", None, "example.com"), ("", None, None), ("", None, "example.com"),
    ("a", None, "."), ("www.example.com", None, "other.org"), ("x\\.y.example.com", None, "example.com"),
    ("www.example.com", "WWW.EXAMPLE.COM.", "example.com"),
]

SIZES = [1, 4, 8, 16, 24, 30, 40, 60, 100]

# glibc's own test's records (resolv/tst-ns_sprintrr.c, from the June 2026
# fixes), each printed at every size and cut at every length; then a few
# of this harness's own.
SWEEP = [
    (1, b"\xc0\x00\x02\x01"),
    (5, b"\x04www1\x04prod\xc0\x10"),
    (13, b"\x05first\x06second"),
    (20, b"\x05first\x06second"),
    (20, b"\x05first"),
    (6, b"\x02ns\xc0\x10\x0ahostmaster\xc0\x10" + u32(1) + u32(2) + u32(3) + u32(4) + u32(5)),
    (15, b"\x00\x0a\x02mx\xc0\x10"),
    (26, b"\x00\x0a\x03px1\xc0\x10\x03px2\xc0\x10"),
    (19, b"\x04X.25"),
    (16, b"\x01A\x02BC\x03DEF"),
    (22, b""), (22, b"\x01"), (22, b"\x01\x02"), (22, b"\x01\x02\x03"), (22, b"\x01\x02\x03\x04"),
    (22, bytes(range(1, 256))),
    (28, bytes.fromhex("20010db8000000000000000000001234")),
    (29, bytes.fromhex("0033161389172dd070be15f000988d20")),
    (35, b"\x00\x01\x00\x02\x05flags\x07service\x02.*\x05naptr\xc0\x10"),
    (33, b"\x00\x01\x00\x02\x00\x50\x04www1\xc0\x10"),
    (17, b"\x03rp1\xc0\x10\x03rp2\xc0\x10"),
    (11, b"\xc0\x00\x02\x01\x06" + bytes(10) + b"\x80"),
    (37, b"\x00\x01\x04\xd2\x00blob"),
    (249, b"\x04algo\x00" + u32(1) + u32(2) + u16(3) + u16(4) + u16(5) + b"\xa1\xa2\xa3\xa4\xa5"
     + u16(3) + b"\xb1\xb2\xb3"),
    (250, b"\x04algo\x00" + u16(16) + bytes.fromhex("ddcd6410e921341a8ee0a19a30fc3bd1")
     + u16(2) + u16(3) + u16(5) + b"other"),
    (38, b"\x00" + bytes.fromhex("20010db8000000000000000000001234") + b"\x06prefix\xc0\x10"),
    (38, b"\x00" + bytes.fromhex("20010db8000000000000000000001235")),
    (38, b"\x80\x06prefix\xc0\x10"),
    (38, b"\x20" + bytes(10) + b"\x12\x36\x06prefix\xc0\x10"),
    # This harness's: names escaped and relative, a TXT's escapes, an
    # unknown type's rows.
    (14, b"\x05a.b\\c\xc0\x10\x00"),
    (16, cs(b"a \"q\" \\ b") + cs(b"line\nbreak")),
    (99, bytes(range(40))),
]

# The packet tst-ns_sprintrr.c prints its records from: a response with one
# question, www.example.org/IN/ANY, and one answer, its owner a pointer to
# the question's name.
PACKET_PREFIX = (b"AA\x81\x80\x00\x01\x00\x01\x00\x00\x00\x00"
                 b"\x03www\x07example\x03org\x00\x00\xff\x00\x01" b"\xc0\x0c")


def sweep_packet(t: int, rdata: bytes) -> bytes:
    return PACKET_PREFIX + u16(t) + u16(1) + u32(86400) + u16(len(rdata)) + rdata


def message() -> bytes:
    """A response: a question and four answers, their names compressed."""
    m = bytearray(u16(0x1234) + u16(0x8180) + u16(1) + u16(4) + u16(0) + u16(0))
    q = len(m)
    m += wire("www.example.com") + u16(1) + u16(1)
    ptr = u16(0xC000 | q)          # www.example.com
    ex = u16(0xC000 | (q + 4))     # example.com
    m += ptr + u16(5) + u16(1) + u32(300) + u16(len(b"\x03web" + ex)) + b"\x03web" + ex
    m += ptr + u16(1) + u16(1) + u32(300) + u16(4) + bytes([192, 0, 2, 7])
    rd = u16(10) + b"\x04mail" + ex
    m += ex + u16(15) + u16(1) + u32(3600) + u16(len(rd)) + rd
    rd = b"\x02ns" + ex + b"\x05admin" + ex + u32(1) + u32(2) + u32(3) + u32(4) + u32(5)
    m += ex + u16(6) + u16(1) + u32(90061) + u16(len(rd)) + rd
    return bytes(m)


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
    case ENOSPC: return "ENOSPC";
    case EMSGSIZE: return "EMSGSIZE";
    default: return "other";
    }
}

/* A text as the lines write it. */
static void text(const char *s, size_t n)
{
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

static void rrf(const char *head, int type, int class, unsigned long ttl, const char *owner,
                const unsigned char *msg, size_t msglen, size_t rdlen, const char *ctx,
                const char *origin, size_t size)
{
    char buf[4096];
    memset(buf, 0, sizeof buf);
    errno = 0;
    int r = ns_sprintrrf(msg, msglen, owner, (ns_class) class, (ns_type) type, ttl, msg, rdlen,
                         ctx, origin, buf, size);
    printf("%s = %d %s ", head, r, name(errno));
    text(buf, strnlen(buf, sizeof buf));
    putchar('\n');
}

static void rr(const char *head, const unsigned char *m, int len, int sect, int n,
               const char *ctx, const char *origin, size_t size)
{
    ns_msg h;
    ns_rr rec;
    char buf[4096];
    memset(buf, 0, sizeof buf);
    if (ns_initparse(m, len, &h) != 0 || ns_parserr(&h, (ns_sect) sect, n, &rec) != 0) {
        printf("%s = parse-failed\n", head);
        return;
    }
    errno = 0;
    int r = ns_sprintrr(&h, &rec, ctx, origin, buf, size);
    printf("%s = %d %s ", head, r, name(errno));
    text(buf, strnlen(buf, sizeof buf));
    putchar('\n');
}

/* One of a sweep's answers: rc/errno/k, and /rest when the text goes on
   past the k bytes it shares with the whole text. */
static void entry(int r, int e, const char *full, size_t fl, const char *buf)
{
    size_t n = strnlen(buf, 4096), k = 0;
    while (k < n && k < fl && buf[k] == full[k])
        k++;
    printf(" %d/%s/%zu", r, name(e), k);
    if (k < n) {
        putchar('/');
        text(buf + k, n - k);
    }
}

/* The packet's answer printed by ns_sprintrr into `size` of 4096 zeroed
   bytes; 0 and the rc, or -1 when the packet does not parse. */
static int answer(const unsigned char *p, size_t plen, char *buf, size_t size, int *rc, int *e)
{
    ns_msg h;
    ns_rr rec;
    memset(buf, 0, 4096);
    if (ns_initparse(p, (int) plen, &h) != 0 || ns_parserr(&h, ns_s_an, 0, &rec) != 0)
        return -1;
    errno = 0;
    *rc = ns_sprintrr(&h, &rec, NULL, NULL, buf, size);
    *e = errno;
    return 0;
}

static void sweep(const char *head, const unsigned char *packet, size_t plen, size_t rdlen)
{
    static char full[4096], buf[4096];
    static unsigned char p[4096];
    int r, e;
    if (answer(packet, plen, full, 4096, &r, &e) != 0) {
        printf("%s = parse-failed\n", head);
        return;
    }
    size_t fl = strnlen(full, sizeof full);
    printf("%s = %d %s ", head, r, name(e));
    text(full, fl);
    fputs(" |", stdout);
    for (size_t size = 1; size <= fl + 16; size++) {
        answer(packet, plen, buf, size, &r, &e);
        entry(r, e, full, fl, buf);
    }
    fputs(" |", stdout);
    for (size_t k = 0; k <= rdlen; k++) {
        size_t tsize = plen - rdlen + k;
        memcpy(p, packet, tsize);
        p[tsize - k - 2] = (unsigned char) (k >> 8);
        p[tsize - k - 1] = (unsigned char) k;
        if (answer(p, tsize, buf, 4096, &r, &e) != 0) {
            fputs(" parse-failed", stdout);
            continue;
        }
        entry(r, e, full, fl, buf);
    }
    putchar('\n');
}

int main(void)
{
''']

    def opt(s):
        return "NULL" if s is None else f'"{c_escape(s)}"'

    def tok(s):
        return "-" if s is None else token(s.encode())

    for t, rdata in RECORDS:
        for owner, ctx, origin in OWNERS[:1]:
            head = f"F {t} 1 86400 {tok(owner)} {rdata.hex() or '-'} {tok(ctx)} {tok(origin)} 4096"
            L.append(f'    rrf("{c_escape(head)}", {t}, 1, 86400, {opt(owner)}, '
                     f'(const unsigned char *) {c_str(rdata or b"x")}, {len(rdata)}, {len(rdata)}, '
                     f'{opt(ctx)}, {opt(origin)}, 4096);')
    for t, rdata, more in RUNNING_ON:
        head = f"F {t} 1 86400 www.example.com {rdata.hex()}+{more.hex()} - - 4096"
        L.append(f'    rrf("{c_escape(head)}", {t}, 1, 86400, "www.example.com", '
                 f'(const unsigned char *) {c_str(rdata + more)}, {len(rdata + more)}, {len(rdata)}, '
                 f'NULL, NULL, 4096);')
    # Owners, TTLs and classes, on an A record.
    a = bytes([192, 0, 2, 1])
    for owner, ctx, origin in OWNERS:
        for ttl, cls in ((86400, 1), (0, 3), (90061, 99)):
            head = f"F 1 {cls} {ttl} {tok(owner)} {a.hex()} {tok(ctx)} {tok(origin)} 4096"
            L.append(f'    rrf("{c_escape(head)}", 1, {cls}, {ttl}, {opt(owner)}, '
                     f'(const unsigned char *) {c_str(a)}, 4, 4, {opt(ctx)}, {opt(origin)}, 4096);')
    # Classes and types by name and by number, and A6 by name.
    for t, cls in ((1, 4), (1, 254), (1, 255), (1, 2), (1, 65535), (38, 1), (41, 1), (255, 1),
                   (252, 1), (65535, 1), (0, 0)):
        head = f"F {t} {cls} 60 www.example.com {a.hex()} - - 4096"
        L.append(f'    rrf("{c_escape(head)}", {t}, {cls}, 60, "www.example.com", '
                 f'(const unsigned char *) {c_str(a)}, 4, 4, NULL, NULL, 4096);')
    # Names in the data under an origin.
    for t, rdata in [(5, wire("target.example.com")), (5, wire("example.com")),
                     (15, u16(10) + wire("mail.example.com")), (2, wire(""))]:
        for origin in ("example.com", "example.com.", ".", "com"):
            head = f"F {t} 1 3600 www.example.com {rdata.hex()} - {token(origin.encode())} 4096"
            L.append(f'    rrf("{c_escape(head)}", {t}, 1, 3600, "www.example.com", '
                     f'(const unsigned char *) {c_str(rdata)}, {len(rdata)}, {len(rdata)}, NULL, '
                     f'"{c_escape(origin)}", 4096);')
    # Buffer sizes, where each part does and does not fit.
    for t, rdata in [(1, a), (6, RECORDS[18][1]), (16, cs(b"hello world")), (99, bytes(range(20))),
                     (11, bytes([192, 0, 2, 1, 6, 0x40, 0, 0, 1])), (37, u16(1) + u16(2) + bytes([3]) + bytes(20))]:
        for size in SIZES:
            head = f"F {t} 1 86400 www.example.com {rdata.hex()} - - {size}"
            L.append(f'    rrf("{c_escape(head)}", {t}, 1, 86400, "www.example.com", '
                     f'(const unsigned char *) {c_str(rdata)}, {len(rdata)}, {len(rdata)}, NULL, NULL, '
                     f'{size});')
    m = message()
    L.append(f'    static const unsigned char msg[] = {c_str(m)};')
    for n in range(4):
        for ctx, origin, size in ((None, None, 4096), ("www.example.com", "example.com", 4096),
                                  (None, None, 20)):
            head = f"R {m.hex()} 1 {n} {tok(ctx)} {tok(origin)} {size}"
            L.append(f'    rr("{c_escape(head)}", msg, {len(m)}, 1, {n}, {opt(ctx)}, {opt(origin)}, '
                     f'{size});')
    for i, (t, rdata) in enumerate(SWEEP):
        p = sweep_packet(t, rdata)
        head = f"S {t} {rdata.hex() or '-'}"
        L.append(f'    static const unsigned char sweep{i}[] = {c_str(p)};')
        L.append(f'    sweep("{c_escape(head)}", sweep{i}, {len(p)}, {len(rdata)});')
    L.append("    return 0;")
    L.append("}")
    return "\n".join(L) + "\n"


def main() -> int:
    with workdir() as d:
        c = Path(d) / "nsprint.c"
        c.write_text(c_program(), encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "nsprint")
        # -lresolv: libresolv's. -Wno-deprecated-declarations: glibc
        # deprecates both, as posix/include does too.
        r = run(f"gcc -O1 -Wall -Werror -Wno-deprecated-declarations -o {exe} {wsl_path(c)} "
                f"-lresolv && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        return 1
    header = ("# glibc's ns_sprintrrf and ns_sprintrr, with ns_print.c's June 2026 fixes "
              "(posix/tools/oracle/nsprint_harness.py):\n"
              "# one line a call -- see the harness for the forms.\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} calls)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
