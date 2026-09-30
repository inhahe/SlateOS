"""glibc 2.39's small `<resolv.h>` helpers -- `dn_count_labels`,
`putlong` and `putshort`, `res_isourserver`, `res_nameinquery`,
`res_queriesmatch`, `hostalias` and `res_hostalias`, `res_close` and
`res_randomid` -- as the oracle for posix/src/resolv.rs's:

    python posix/tools/oracle/resutil_harness.py   # writes posix/src/resutil_oracle.txt

One line a call:

    C <name> = <count>                                   dn_count_labels
    L <value> = <4 bytes>, H <value> = <2 bytes>         putlong, putshort (hex)
    N <address> <port>                                   one of _res's nameservers
    I <family> <address> <port> = <rc>                   res_isourserver, with those
    Q <message> <length> <name> <type> <class> = <rc>    res_nameinquery
    M <message> <length> <message> <length> = <rc>       res_queriesmatch
    F <file>                                             the HOSTALIASES file's bytes
    A <mode> <name> <size> = <alias or NULL>             hostalias (h), res_hostalias
                                                         on a state from res_ninit (r) or
                                                         one with RES_NOALIASES (n), and
                                                         hostalias with no HOSTALIASES (u)
                                                         or one naming no file (m)
    X <what> = <value>                                   res_close and res_randomid

`<name>` is written as posix/src/resolv.rs's tests write text: a byte from
`!` to `~` as itself but `\\`, any other as `\\xHH`, the empty text `\\x`.
`<message>`, `<file>` and the byte answers are hex; `<length>` is where the
message is cut, its `eom`.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "resutil_oracle.txt"


def c_str(b: bytes) -> str:
    """`b` as a C string literal, every byte a hex escape."""
    return '"' + "".join(f"\\x{c:02x}" for c in b) + '"'


def token(b: bytes) -> str:
    """How a text is written in a line."""
    if not b:
        return "\\x"
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c != 0x5C else f"\\x{c:02x}" for c in b)


def c_escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def wire(name: str) -> bytes:
    out = b""
    for label in name.split(".") if name else []:
        out += bytes([len(label)]) + label.encode()
    return out + b"\x00"


def header(ident: int, flags: int, qd: int) -> bytes:
    return ident.to_bytes(2, "big") + flags.to_bytes(2, "big") + qd.to_bytes(2, "big") + bytes(6)


def q(name: str, qtype: int, qclass: int = 1) -> bytes:
    return wire(name) + qtype.to_bytes(2, "big") + qclass.to_bytes(2, "big")


EX = header(0x1234, 0x0100, 1) + q("Example.COM", 1)
EX_ID = header(0x9999, 0x0100, 1) + q("example.com", 1)
NET = header(0x1234, 0x0100, 1) + q("example.net", 1)
TWO = header(0x1234, 0x0100, 2) + q("example.com", 1) + q("example.org", 15)
TWO_REV = header(0x4321, 0x0100, 2) + q("example.org", 15) + q("EXAMPLE.com", 1)
TWO_ONE = header(0x4321, 0x0100, 2) + q("example.org", 15) + q("example.org", 15)
# The second question's name is `www` and a pointer to the first's.
COMP = header(0x1234, 0x0100, 2) + q("example.com", 1) + b"\x03www\xc0\x0c" + b"\x00\x01\x00\x01"
UPD1 = header(0x1234, 0x2800, 1) + q("example.com", 6)
UPD2 = header(0x5678, 0x2800, 1) + q("other.org", 6)
UPD_Q = header(0x5678, 0x2800, 1) + q("example.com", 1)
ZERO_Q = header(0x1234, 0x0100, 0)

MESSAGES = {"EX": EX, "EX_ID": EX_ID, "NET": NET, "TWO": TWO, "TWO_REV": TWO_REV,
            "TWO_ONE": TWO_ONE, "COMP": COMP, "UPD1": UPD1, "UPD2": UPD2, "UPD_Q": UPD_Q,
            "ZERO_Q": ZERO_Q}

NAMEINQUERY = [
    ("EX", None, "example.com", 1, 1), ("EX", None, "example.com.", 1, 1),
    ("EX", None, "EXAMPLE.COM", 1, 1), ("EX", None, "example.net", 1, 1),
    ("EX", None, "example.com", 28, 1), ("EX", None, "example.com", 1, 3),
    ("EX", None, "example", 1, 1), ("TWO", None, "example.org", 15, 1),
    ("TWO", None, "example.org", 1, 1), ("COMP", None, "www.example.com", 1, 1),
    ("COMP", None, "www", 1, 1), ("EX", 20, "example.com", 1, 1), ("EX", 12, "example.com", 1, 1),
    ("EX", 27, "example.com", 1, 1), ("EX", 28, "example.com", 1, 1), ("ZERO_Q", None, "a", 1, 1),
    ("EX", None, "", 1, 1),
]

QUERIESMATCH = [
    ("EX", None, "EX", None), ("EX", None, "EX_ID", None), ("EX", None, "NET", None),
    ("TWO", None, "TWO_REV", None), ("TWO_REV", None, "TWO", None), ("TWO", None, "TWO_ONE", None),
    ("TWO_ONE", None, "TWO", None), ("EX", None, "TWO", None), ("UPD1", None, "UPD2", None),
    ("UPD1", None, "UPD_Q", None), ("UPD_Q", None, "EX", None), ("EX", None, "UPD_Q", None),
    ("EX", 11, "EX", None), ("EX", None, "EX", 11), ("EX", 20, "EX", None),
    ("EX", None, "EX", 20), ("EX", None, "EX", 12), ("ZERO_Q", None, "ZERO_Q", None),
    ("ZERO_Q", None, "EX", None), ("COMP", None, "COMP", None),
]

# _res's nameservers for res_isourserver.
SERVERS = [("127.0.0.1", 53), ("10.0.0.53", 5353), ("0.0.0.0", 54)]
ISOURS = [(2, "127.0.0.1", 53), (2, "127.0.0.1", 54), (2, "127.0.0.2", 53),
          (2, "10.0.0.53", 5353), (2, "10.0.0.53", 53), (2, "8.8.8.8", 54),
          (2, "8.8.8.8", 55), (10, "127.0.0.1", 53), (0, "127.0.0.1", 53)]

ALIASES = (b"# comment lines are words too\n"
           b"ftp ftp.example.com\n"
           b"www\twww.example.org  extra words\n"
           b"Mail   mail.example.com\n"
           b"alone \n"
           b"long a-very-long-alias.example.com\n"
           b"dot. dotted.example.com\n"
           b"nospace\n"
           b"after after.example.com\n"
           # Longer than glibc's BUFSIZ: its first fgets has no white space,
           # which ends the scan -- `beyond` below is never reached.
           + b"x" * 9000 + b" y\n"
           b"beyond beyond.example.com\n")

ALIAS_CALLS = [
    ("h", b"ftp", 0), ("h", b"FTP", 0), ("h", b"ftp.", 0), ("h", b"www", 0), ("h", b"mail", 0),
    ("h", b"MAIL.", 0), ("h", b"alone", 0), ("h", b"dot", 0), ("h", b"dot.", 0),
    ("h", b"nope", 0), ("h", b"after", 0), ("h", b"beyond", 0), ("h", b"#", 0), ("h", b"", 0),
    ("r", b"ftp", 64), ("r", b"long", 64), ("r", b"long", 8), ("r", b"long", 2), ("r", b"long", 1),
    ("r", b"www", 16), ("r", b"nope", 64),
    ("n", b"ftp", 64), ("u", b"ftp", 0), ("m", b"ftp", 0),
]

LONGS = [0, 1, 0x12345678, 0xFFFFFFFF, 0x80000000]
SHORTS = [0, 1, 0x1234, 0xFFFF, 0x8000]
LABELS = [b"", b".", b"a", b"a.", b"a.b", b"a.b.", b"*.a.b", b"*", b"*.", b"*.a", b"a..b",
          b"a\\.b", b"..", b"*a.b", b".a", b"www.example.com."]


def c_program(alias_path: str) -> str:
    L = [r'''#define _GNU_SOURCE
#include <arpa/inet.h>
#include <arpa/nameser.h>
#include <errno.h>
#include <netinet/in.h>
#include <resolv.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void hex(const unsigned char *b, size_t n)
{
    for (size_t i = 0; i < n; i++)
        printf("%02x", b[i]);
}

static void server(int i, const char *a, int port)
{
    memset(&_res.nsaddr_list[i], 0, sizeof _res.nsaddr_list[i]);
    _res.nsaddr_list[i].sin_family = AF_INET;
    _res.nsaddr_list[i].sin_port = htons(port);
    inet_pton(AF_INET, a, &_res.nsaddr_list[i].sin_addr);
}

static void isours(int family, const char *a, int port)
{
    struct sockaddr_in6 s6;
    memset(&s6, 0, sizeof s6);
    struct sockaddr_in *s = (struct sockaddr_in *) &s6;
    s->sin_family = family;
    s->sin_port = htons(port);
    inet_pton(AF_INET, a, &s->sin_addr);
    printf("I %d %s %d = %d\n", family, a, port, res_isourserver(s));
}

static void alias(const char *mode, const char *text, const char *name, size_t size,
                  const char *got)
{
    printf("A %s %s %zu = %s\n", mode, text, size, got ? got : "NULL");
}

int main(void)
{
    char dst[64];
    struct __res_state st, noal;
    memset(&st, 0, sizeof st);
    memset(&noal, 0, sizeof noal);
''']
    for b in LABELS:
        L.append(f'    printf("C {c_escape(token(b))} = %d\\n", dn_count_labels({c_str(b)}));')
    for v in LONGS:
        L.append(f'    {{ unsigned char o[4]; putlong(0x{v:x}u, o); printf("L {v:x} = "); '
                 f'hex(o, 4); putchar(\'\\n\'); }}')
    for v in SHORTS:
        L.append(f'    {{ unsigned char o[2]; putshort(0x{v:x}u, o); printf("H {v:x} = "); '
                 f'hex(o, 2); putchar(\'\\n\'); }}')
    # _res before res_init, then after.
    L.append('    printf("X init-before = %d\\n", (int) (_res.options & RES_INIT) != 0);')
    L.append('    res_close();')
    L.append('    printf("X init-after-close-uninit = %d\\n", (int) (_res.options & RES_INIT) != 0);')
    L.append('    res_init();')
    L.append(f'    _res.nscount = {len(SERVERS)};')
    for i, (a, port) in enumerate(SERVERS):
        L.append(f'    server({i}, "{a}", {port});')
        L.append(f'    printf("N {a} {port}\\n");')
    for fam, a, port in ISOURS:
        L.append(f'    isours({fam}, "{a}", {port});')
    L.append('    res_close();')
    L.append('    printf("X init-after-close = %d\\n", (int) (_res.options & RES_INIT) != 0);')
    L.append(f'    printf("X nscount-after-close = %d\\n", _res.nscount);')
    L.append('    {')
    L.append('        unsigned int a = res_randomid(), same = 1, big = 0;')
    L.append('        for (int i = 0; i < 1000; i++) {')
    L.append('            unsigned int b = res_randomid();')
    L.append('            same &= b == a;')
    L.append('            big |= b > 0xffff;')
    L.append('        }')
    L.append('        printf("X randomid = %s\\n", !same && !big ? "ok" : "bad");')
    L.append('    }')
    for name, cut, qname, qtype, qclass in NAMEINQUERY:
        m = MESSAGES[name]
        n = len(m) if cut is None else cut
        L.append(f'    {{ static const unsigned char m[] = {c_str(m)}; '
                 f'printf("Q {m.hex()} {n} {c_escape(token(qname.encode()))} {qtype} {qclass} = %d\\n", '
                 f'res_nameinquery({c_str(qname.encode())}, {qtype}, {qclass}, m, m + {n})); }}')
    for a, cut_a, b, cut_b in QUERIESMATCH:
        ma, mb = MESSAGES[a], MESSAGES[b]
        na = len(ma) if cut_a is None else cut_a
        nb = len(mb) if cut_b is None else cut_b
        L.append(f'    {{ static const unsigned char a[] = {c_str(ma)}, b[] = {c_str(mb)}; '
                 f'printf("M {ma.hex()} {na} {mb.hex()} {nb} = %d\\n", '
                 f'res_queriesmatch(a, a + {na}, b, b + {nb})); }}')
    L.append(f'    printf("F {ALIASES.hex()}\\n");')
    L.append(f'    setenv("HOSTALIASES", "{alias_path}", 1);')
    L.append('    res_ninit(&st);')
    L.append('    res_ninit(&noal);')
    L.append('    noal.options |= RES_NOALIASES;')
    for mode, name, size in ALIAS_CALLS:
        t = c_escape(token(name))
        n = c_str(name)
        if mode == "h":
            L.append(f'    alias("h", "{t}", {n}, 0, hostalias({n}));')
        elif mode in "rn":
            state = "&st" if mode == "r" else "&noal"
            L.append(f'    memset(dst, 0xaa, sizeof dst); '
                     f'alias("{mode}", "{t}", {n}, {size}, res_hostalias({state}, {n}, dst, {size}));')
        elif mode == "u":
            L.append('    unsetenv("HOSTALIASES");')
            L.append(f'    alias("u", "{t}", {n}, 0, hostalias({n}));')
        elif mode == "m":
            L.append(f'    setenv("HOSTALIASES", "{alias_path}.missing", 1);')
            L.append(f'    alias("m", "{t}", {n}, 0, hostalias({n}));')
    L.append("    return 0;")
    L.append("}")
    return "\n".join(L) + "\n"


def main() -> int:
    with workdir() as d:
        aliases = Path(d) / "aliases"
        aliases.write_bytes(ALIASES)
        c = Path(d) / "resutil.c"
        c.write_text(c_program(wsl_path(aliases)), encoding="utf-8", newline="\n")
        exe = wsl_path(Path(d) / "resutil")
        # -lresolv: libresolv's. -Wno-deprecated-declarations: glibc
        # deprecates most of these, as posix/include does too.
        r = run(f"gcc -O1 -Wall -Werror -Wno-deprecated-declarations -o {exe} {wsl_path(c)} "
                f"-lresolv && {exe}")
    if r.returncode != 0:
        sys.stderr.write(r.stdout + r.stderr)
        return 1
    header_text = ("# glibc 2.39's dn_count_labels, putlong, putshort, res_isourserver,\n"
                   "# res_nameinquery, res_queriesmatch, hostalias, res_hostalias, res_close and\n"
                   "# res_randomid (posix/tools/oracle/resutil_harness.py): one line a call --\n"
                   "# see the harness for the forms.\n")
    OUT.write_text(header_text + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} lines)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
