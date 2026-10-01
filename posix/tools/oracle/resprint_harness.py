r"""glibc 2.39's message and name printers in `<resolv.h>` -- `fp_nquery`,
`fp_query`, `p_query`, `p_cdnname`, `p_cdname`, `p_fqnname`, `p_fqname`,
`fp_resstat` -- as the oracle for posix/src/res_print.rs:

    python posix/tools/oracle/resprint_harness.py   # writes posix/src/resprint_oracle.txt

One line a call:

    Q <pfcode> <message> <len> = <text>       fp_nquery(msg, len, f), `_res.pfcode` set
    Y <pfcode> <message> = <text>             fp_query(msg, f): the message at the
                                              start of 512 zeroed bytes, PACKETSZ read
    N <message> <offset> <len> = <ret> <text>      p_cdnname(msg + offset, msg, len, f)
    D <message> <offset> = <ret> <text>            p_cdname, in 512 zeroed bytes
    F <message> <offset> <msglen> <namelen> = <ret> <name>
                                              p_fqnname into `namelen` bytes, zeroed
    G <message> <offset> = <ret> <text>            p_fqname, in 512 zeroed bytes
    O <options> = <text>                      fp_resstat of a state with those options

`<ret>` is the returned pointer's offset in the message, `-` for NULL. Every
`<text>` is what the call wrote to its stream (an `open_memstream`), and is
written as posix/src/resolv.rs's tests write text (a byte from `!` to `~` as
itself but `\\`, any other as `\\xHH`, the empty text `\\x`); `<message>` is
hex, `<options>` and `<pfcode>` hex. `p_query` is not here: it is
`fp_query` on standard output, and the tests check that its text arrives
there whole.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "resprint_oracle.txt"


def token(b: bytes) -> str:
    if not b:
        return "\\x"
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c != 0x5C else f"\\x{c:02x}" for c in b)


def c_bytes(b: bytes) -> str:
    return '"' + "".join(f"\\x{c:02x}" for c in b) + '"'


def u16(v: int) -> bytes:
    return v.to_bytes(2, "big")


def u32(v: int) -> bytes:
    return v.to_bytes(4, "big")


def wire(name: str) -> bytes:
    out = b""
    for label in [x for x in name.split(".") if x]:
        out += bytes([len(label)]) + label.encode()
    return out + b"\x00"


def header(id_: int, flags: int, qd: int, an: int, ns: int, ar: int) -> bytes:
    return u16(id_) + u16(flags) + u16(qd) + u16(an) + u16(ns) + u16(ar)


def flags(qr=0, opcode=0, aa=0, tc=0, rd=0, ra=0, z=0, ad=0, cd=0, rcode=0) -> int:
    return (qr << 15 | opcode << 11 | aa << 10 | tc << 9 | rd << 8 | ra << 7 | z << 6
            | ad << 5 | cd << 4 | rcode)


def rr(name: bytes, type_: int, class_: int, ttl: int, rdata: bytes) -> bytes:
    return name + u16(type_) + u16(class_) + u32(ttl) + u16(len(rdata)) + rdata


def question(name: bytes, type_: int = 1, class_: int = 1) -> bytes:
    return name + u16(type_) + u16(class_)


QNAME = wire("www.example.org")   # at offset 12 in every message below
PTR12 = b"\xc0\x0c"               # a pointer to it


def messages() -> list:
    """(label, message bytes)."""
    out = []
    a1 = rr(PTR12, 1, 1, 3600, bytes([93, 184, 216, 34]))
    cname = rr(PTR12, 5, 1, 300, wire("example.org"))
    ns = rr(b"\xc0\x10", 2, 1, 86400, wire("ns1.example.org"))
    glue = rr(wire("ns1.example.org"), 1, 1, 86400, bytes([192, 0, 2, 1]))
    mx = rr(PTR12, 15, 1, 60, u16(10) + wire("mail.example.org"))
    txt = rr(PTR12, 16, 1, 60, b"\x05hello\x05world")
    aaaa = rr(PTR12, 28, 1, 60, bytes(range(16)))
    soa = rr(b"\xc0\x10", 6, 1, 3600, wire("ns1.example.org") + wire("host.example.org")
             + u32(2026093001) + u32(7200) + u32(3600) + u32(1209600) + u32(300))
    unknown = rr(PTR12, 999, 1, 60, b"\x01\x02\x03")
    out.append(("response", header(0x1234, flags(qr=1, rd=1, ra=1), 1, 2, 1, 1)
                + question(QNAME) + a1 + cname + ns + glue))
    out.append(("query", header(0xBEEF, flags(rd=1), 1, 0, 0, 0) + question(QNAME)))
    out.append(("nxdomain", header(7, flags(qr=1, aa=1, rd=1, ra=1, rcode=3), 1, 0, 1, 0)
                + question(QNAME) + soa))
    out.append(("every flag", header(65535, flags(qr=1, aa=1, tc=1, rd=1, ra=1, z=1, ad=1, cd=1),
                                     1, 1, 0, 0) + question(QNAME) + a1))
    out.append(("types", header(1, flags(qr=1), 1, 5, 0, 0) + question(QNAME, 255)
                + mx + txt + aaaa + unknown + a1))
    out.append(("empty", header(0, 0, 0, 0, 0, 0)))
    for op in [1, 2, 3, 4, 5, 6, 13, 14, 15]:
        out.append((f"opcode {op}", header(op, flags(opcode=op), 1, 1, 1, 1) + question(QNAME)
                    + a1 + ns + glue))
    for rc in [1, 2, 4, 5, 6, 9, 10, 11, 15]:
        out.append((f"rcode {rc}", header(rc, flags(qr=1, rcode=rc), 1, 0, 0, 0)
                    + question(QNAME)))
    out.append(("answers only", header(2, flags(qr=1), 0, 1, 0, 0) + rr(wire("a.example"), 1, 1, 5,
                                                                         b"\x7f\x00\x00\x01")))
    full = header(0x1234, flags(qr=1, rd=1, ra=1), 1, 2, 1, 1) + question(QNAME) + a1 + cname + ns
    out.append(("counts past the data", full))
    bad = header(3, flags(qr=1), 1, 1, 0, 0) + question(QNAME) + rr(b"\xc0\xff", 1, 1, 5,
                                                                      b"\x01\x02\x03\x04")
    out.append(("bad pointer", bad))
    long_txt = b"".join(b"\xff" + bytes([0x41 + (k % 26)]) * 255 for k in range(16))
    out.append(("long text", header(4, flags(qr=1), 1, 1, 0, 0) + question(QNAME)
                + rr(PTR12, 16, 1, 1, long_txt)))
    out.append(("header only", header(5, flags(qr=1), 0, 0, 0, 0)[:12]))
    # Exactly PACKETSZ bytes, the only length fp_query and p_query print a
    # message of: they take every message to be 512 bytes long, and
    # ns_initparse refuses bytes after the last record.
    room = 512 - len(header(0, 0, 0, 0, 0, 0)) - len(question(QNAME)) - len(rr(PTR12, 16, 1, 60, b""))
    txt = b"\xff" + b"x" * 255 + bytes([room - 257]) + b"y" * (room - 257)
    full = header(0xBEEF, flags(qr=1, rd=1, ra=1), 1, 1, 0, 0) + question(QNAME) + rr(PTR12, 16, 1,
                                                                                    60, txt)
    assert len(full) == 512, len(full)
    out.append(("full packet", full))
    return out


PFCODES = [0, 0x1, 0x2, 0x4, 0x8, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400, 0x800,
           0x1000, 0x2000, 0x4000, 0x100 | 0x200 | 0x800, 0x10 | 0x20, 0x100 | 0x20, 0x7FFF,
           0x30 | 0x100]

OPTIONS = [0, 1, 0x2C1, 0xFFFFFFFF, 0xFFFFFFFFFFFFFFFF] + [1 << k for k in range(64)]


def cases():
    q, y, n, d, f, g = [], [], [], [], [], []
    for label, msg in messages():
        for pf in PFCODES:
            q.append((pf, msg, len(msg)))
        if label in ("response", "query", "empty", "header only", "types"):
            for cut in sorted({0, 1, 11, 12, 13, 20, len(msg) - 1, len(msg) + 1}):
                if cut >= 0:
                    q.append((0, msg, cut))
        if label in ("response", "query", "empty", "full packet"):
            for pf in [0, 0x100 | 0x200 | 0x800, 0x30]:
                y.append((pf, msg))
    msg = messages()[0][1]
    names = [12, 33, 45]
    for off in [12, 29, 33, 45, 55, 0, len(msg) - 2, len(msg)]:
        for ln in [len(msg), 40, 14, 12, 0]:
            n.append((msg, off, ln))
        d.append((msg, off))
        g.append((msg, off))
        for msglen in [len(msg), 255, 5, 0]:
            for namelen in [1025, 17, 16, 15, 2, 1]:
                f.append((msg, off, msglen, namelen))
    root = b"\x00" + b"\xc0\x00"
    for off, ln in [(0, 3), (1, 3), (0, 1)]:
        n.append((root, off, ln))
        f.append((root, off, ln, 4))
    return q, y, n, d, f, g


C_MAIN = r'''
static void text(char *buf, size_t len)
{
    if (len == 0) {
        fputs("\\x", stdout);
        return;
    }
    for (size_t i = 0; i < len; i++) {
        unsigned char c = (unsigned char) buf[i];
        if (c >= 0x21 && c <= 0x7e && c != '\\')
            putchar(c);
        else
            printf("\\x%02x", c);
    }
}

#define CAPTURE(call)                                             \
    do {                                                          \
        char *buf = NULL;                                         \
        size_t len = 0;                                           \
        FILE *f = open_memstream(&buf, &len);                     \
        call;                                                     \
        fclose(f);                                                \
        text(buf, len);                                           \
        free(buf);                                                \
    } while (0)

int main(void)
{
    static unsigned char pad[512];
    res_init();
    for (unsigned i = 0; i < sizeof QP / sizeof *QP; i++) {
        _res.pfcode = QP[i];
        printf("Q %u = ", i);
        CAPTURE(fp_nquery(QM[i], QL[i], f));
        putchar('\n');
    }
    for (unsigned i = 0; i < sizeof YP / sizeof *YP; i++) {
        _res.pfcode = YP[i];
        memset(pad, 0, sizeof pad);
        memcpy(pad, YM[i], YML[i]);
        printf("Y %u = ", i);
        CAPTURE(fp_query(pad, f));
        putchar('\n');
    }
    _res.pfcode = 0;
    for (unsigned i = 0; i < sizeof NO / sizeof *NO; i++) {
        const unsigned char *r = NULL;
        printf("N %u = ", i);
        char *buf = NULL;
        size_t len = 0;
        FILE *f = open_memstream(&buf, &len);
        r = p_cdnname(NM[i] + NO[i], NM[i], NL[i], f);
        fclose(f);
        if (r)
            printf("%ld ", (long) (r - NM[i]));
        else
            fputs("- ", stdout);
        text(buf, len);
        free(buf);
        putchar('\n');
    }
    for (unsigned i = 0; i < sizeof DO / sizeof *DO; i++) {
        memset(pad, 0, sizeof pad);
        memcpy(pad, DM[i], DML[i]);
        printf("D %u = ", i);
        char *buf = NULL;
        size_t len = 0;
        FILE *f = open_memstream(&buf, &len);
        const unsigned char *r = p_cdname(pad + DO[i], pad, f);
        fclose(f);
        if (r)
            printf("%ld ", (long) (r - pad));
        else
            fputs("- ", stdout);
        text(buf, len);
        free(buf);
        putchar('\n');
    }
    for (unsigned i = 0; i < sizeof FO / sizeof *FO; i++) {
        /* p_fqnname's message ends msglen past cp, which can be past the
         * message: it is read from 1024 zeroed bytes. */
        static unsigned char fbuf[1024];
        char name[2048];
        memset(fbuf, 0, sizeof fbuf);
        memcpy(fbuf, FM[i], FML[i]);
        memset(name, 0, sizeof name);
        printf("F %u = ", i);
        const unsigned char *r = p_fqnname(fbuf + FO[i], fbuf, FL[i], name, FN[i]);
        if (r)
            printf("%ld ", (long) (r - fbuf));
        else
            fputs("- ", stdout);
        text(name, strnlen(name, sizeof name));
        putchar('\n');
    }
    for (unsigned i = 0; i < sizeof GO / sizeof *GO; i++) {
        memset(pad, 0, sizeof pad);
        memcpy(pad, GM[i], GML[i]);
        printf("G %u = ", i);
        char *buf = NULL;
        size_t len = 0;
        FILE *f = open_memstream(&buf, &len);
        const unsigned char *r = p_fqname(pad + GO[i], pad, f);
        fclose(f);
        if (r)
            printf("%ld ", (long) (r - pad));
        else
            fputs("- ", stdout);
        text(buf, len);
        free(buf);
        putchar('\n');
    }
    for (unsigned i = 0; i < sizeof OPT / sizeof *OPT; i++) {
        struct __res_state st;
        memset(&st, 0, sizeof st);
        st.options = OPT[i];
        printf("O %u = ", i);
        CAPTURE(fp_resstat(&st, f));
        putchar('\n');
    }
    return 0;
}
'''


def main() -> None:
    q, y, n, d, f, g = cases()
    lines = ["#define _GNU_SOURCE", "#include <resolv.h>", "#include <stdio.h>",
             "#include <stdlib.h>", "#include <string.h>", ""]

    def arr(name, ctype, vals):
        lines.append(f"static const {ctype} {name}[] = {{" + ", ".join(vals) + "};")

    arr("QP", "unsigned long", [f"0x{pf:x}UL" for pf, _m, _l in q])
    arr("QM", "unsigned char *const", [f"(unsigned char *) {c_bytes(m)}" for _p, m, _l in q])
    arr("QL", "int", [str(ln) for _p, _m, ln in q])
    arr("YP", "unsigned long", [f"0x{pf:x}UL" for pf, _m in y])
    arr("YM", "unsigned char *const", [f"(unsigned char *) {c_bytes(m)}" for _p, m in y])
    arr("YML", "unsigned", [str(len(m)) for _p, m in y])
    arr("NM", "unsigned char *const", [f"(unsigned char *) {c_bytes(m)}" for m, _o, _l in n])
    arr("NO", "int", [str(o) for _m, o, _l in n])
    arr("NL", "int", [str(ln) for _m, _o, ln in n])
    arr("DM", "unsigned char *const", [f"(unsigned char *) {c_bytes(m)}" for m, _o in d])
    arr("DO", "int", [str(o) for _m, o in d])
    arr("DML", "unsigned", [str(len(m)) for m, _o in d])
    arr("FM", "unsigned char *const", [f"(unsigned char *) {c_bytes(m)}" for m, _o, _l, _n in f])
    arr("FO", "int", [str(o) for _m, o, _l, _n in f])
    arr("FL", "int", [str(ln) for _m, _o, ln, _n in f])
    arr("FN", "int", [str(nl) for _m, _o, _l, nl in f])
    arr("FML", "unsigned", [str(len(m)) for m, _o, _l, _n in f])
    arr("GM", "unsigned char *const", [f"(unsigned char *) {c_bytes(m)}" for m, _o in g])
    arr("GO", "int", [str(o) for _m, o in g])
    arr("GML", "unsigned", [str(len(m)) for m, _o in g])
    arr("OPT", "unsigned long", [f"0x{o:x}UL" for o in OPTIONS])
    lines.append(C_MAIN)
    with workdir() as t:
        dd = Path(t)
        (dd / "rp.c").write_text("\n".join(lines), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(dd)} && gcc -O1 -w -o rp rp.c -lresolv && LC_ALL=C ./rp")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr[-3000:]}\n{r.stdout[-2000:]}")
    got = {}
    for line in r.stdout.split("\n"):
        if not line:
            continue
        kind, idx, eq, rest = line.split(" ", 3)
        assert eq == "=", line
        got[(kind, int(idx))] = rest
    out = [
        "# glibc 2.39's <resolv.h> message and name printers, for posix/src/res_print.rs.",
        "# Generated by posix/tools/oracle/resprint_harness.py; do not edit.",
    ]
    for i, (pf, m, ln) in enumerate(q):
        out.append(f"Q {pf:x} {m.hex() or '-'} {ln} = {got[('Q', i)]}")
    for i, (pf, m) in enumerate(y):
        out.append(f"Y {pf:x} {m.hex() or '-'} = {got[('Y', i)]}")
    for i, (m, o, ln) in enumerate(n):
        out.append(f"N {m.hex() or '-'} {o} {ln} = {got[('N', i)]}")
    for i, (m, o) in enumerate(d):
        out.append(f"D {m.hex() or '-'} {o} = {got[('D', i)]}")
    for i, (m, o, ln, nl) in enumerate(f):
        out.append(f"F {m.hex() or '-'} {o} {ln} {nl} = {got[('F', i)]}")
    for i, (m, o) in enumerate(g):
        out.append(f"G {m.hex() or '-'} {o} = {got[('G', i)]}")
    for i, o in enumerate(OPTIONS):
        out.append(f"O {o:x} = {got[('O', i)]}")
    OUT.write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(out) - 2} calls")


if __name__ == "__main__":
    main()
