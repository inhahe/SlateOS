"""glibc 2.39's DNS-message parser (`<arpa/nameser.h>`), as the oracle for
posix/src/resolv.rs's `ns_initparse`, `ns_parserr`, `ns_skiprr` and
`ns_name_uncompress`:

    python posix/tools/oracle/ns_harness.py   # writes posix/src/ns_oracle.txt

The messages are built here, byte by byte -- a response with compressed names
in all four sections, and messages cut short, over-counted, looping, with a
bad label, an rdata past the end, names that need escaping -- and glibc's
answers are recorded, one call a line, beside the messages themselves:

    M <name> <hex>                          a message
    I <name> = <rc> <errno> <handle>        ns_initparse over all of it
    P <name> <section> <rrnum> = <rc> <errno> <rr> ; <handle>
    K <name> <offset> <section> <count> <eom> = <rc> <errno>
    U <name> <offset> <dstsiz> = <rc> <errno> <text>

A handle is `id flags c0 c1 c2 c3 s0 s1 s2 s3 sect rrnum ptr`, each pointer
an offset into the message or `-` for NULL; an rr is `name type class ttl
rdlength rdata`, the name escaped as `\\xHH` outside `!`..`~` (and `\\`).
`errno` is 1234 before each call. `P` lines run in order on one handle,
which is how their state carries from call to call.
"""

import struct
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "ns_oracle.txt"


def name(*labels):
    return b"".join(bytes([len(x)]) + x for x in labels) + b"\0"


def hdr(ident, flags, qd, an, ns, ar):
    return struct.pack(">HHHHHH", ident, flags, qd, an, ns, ar)


def ptr(off):
    return struct.pack(">H", 0xC000 | off)


def question(nm, t, c):
    return nm + struct.pack(">HH", t, c)


def rr(nm, t, c, ttl, rdata, rdlength=None):
    return nm + struct.pack(">HHIH", t, c, ttl, len(rdata) if rdlength is None else rdlength) + rdata


# The response: www.example.com, a CNAME and an A, an NS, and its glue.
# "example.com" begins at offset 16 (12 for the header, 4 for "\3www").
QNAME = name(b"www", b"example", b"com")
FULL = (hdr(0x1234, 0x8180, 1, 2, 1, 1) + question(QNAME, 1, 1)
        + rr(ptr(12), 5, 1, 300, b"\x03web" + ptr(16))
        + rr(b"\x03web" + ptr(16), 1, 1, 60, bytes([93, 184, 216, 34]))
        + rr(ptr(16), 2, 1, 86400, b"\x02ns" + ptr(16))
        + rr(b"\x02ns" + ptr(16), 1, 1, 86400, bytes([10, 0, 0, 1])))

MESSAGES = {
    "full": FULL,
    "query": hdr(1, 0x0100, 1, 0, 0, 0) + question(QNAME, 28, 1),
    "empty_counts": hdr(7, 0x8183, 0, 0, 0, 0),
    "overcount": hdr(2, 0x8180, 1, 5, 0, 0) + FULL[12:],
    "trailing": FULL + b"\x00",
    "badlabel": hdr(3, 0, 1, 0, 0, 0) + b"\x40abc\x00" + struct.pack(">HH", 1, 1),
    "loop": hdr(4, 0, 1, 0, 0, 0) + ptr(12) + struct.pack(">HH", 1, 1),
    "rdlen": hdr(5, 0x8180, 0, 1, 0, 0) + rr(name(b"a"), 1, 1, 1, b"\x01\x02", rdlength=100),
    "special": hdr(6, 0, 1, 0, 0, 0)
    + question(name(b"a.b", b"c\\d", b"e f", b"\x00\xff", b"g\"h"), 1, 1),
    "root": hdr(8, 0, 1, 1, 0, 0) + question(b"\0", 2, 1) + rr(b"\0", 2, 1, 5, b"\0"),
    "forward": hdr(9, 0, 1, 0, 0, 0) + ptr(16) + struct.pack(">HH", 1, 1) + b"\x03far\x00",
}
for n in (0, 1, 11, 12, 20, 33, 40, 50, len(FULL) - 1):
    MESSAGES[f"cut{n}"] = FULL[:n]

# ns_parserr's calls on `full`, in order, on one handle: every record in
# order, one past the end of each section, the "next" form, random access,
# and the sections that are no section.
SEQ_FULL = ([(s, i) for s, n in enumerate((1, 2, 1, 1)) for i in range(n + 1)]
            + [(1, -1), (1, -1), (1, -1), (1, -1)]
            + [(1, 1), (1, 0), (3, 0), (0, 0), (2, 0), (1, -1), (4, 0), (-1, 0), (1, 2), (1, -2), (0, 0)])
SEQS = {name_: [(s, i) for s in range(4) for i in range(3)] for name_ in MESSAGES}
SEQS["full"] = SEQ_FULL

# ns_skiprr on `full`: (offset, section, count, eom offset).
SKIPS = [(12, 0, 1, len(FULL)), (33, 1, 2, len(FULL)), (33, 1, 4, len(FULL)), (33, 1, 5, len(FULL)),
         (33, 1, 0, len(FULL)), (12, 1, 1, len(FULL)), (12, 0, 1, 20), (33, 1, 1, 40), (12, 0, -1, len(FULL))]

# ns_name_uncompress: (message, offset, dstsiz).
UNCOMPRESS = [("full", 12, 1025), ("full", 33, 1025), ("full", 45, 1025), ("full", 12, 16),
              ("full", 12, 15), ("full", 12, 1), ("special", 12, 1025), ("special", 12, 8),
              ("root", 12, 1025), ("root", 12, 1), ("loop", 12, 1025), ("badlabel", 12, 1025),
              ("forward", 12, 1025), ("full", 40, 1025)]


def c_bytes(b: bytes) -> str:
    return "{" + ",".join(str(x) for x in b) + "}" if b else "{0}"


def c_program() -> str:
    L = [r'''#define _GNU_SOURCE
#include <arpa/nameser.h>
#include <errno.h>
#include <resolv.h>
#include <stdio.h>
#include <string.h>

static const unsigned char *base, *end;
static void off(const unsigned char *p) {
  if (!p) printf(" -"); else if (p >= base && p <= end) printf(" %ld", (long)(p - base)); else printf(" ?");
}
static void handle(const ns_msg *h) {
  printf(" %u %u %u %u %u %u", h->_id, h->_flags, h->_counts[0], h->_counts[1], h->_counts[2], h->_counts[3]);
  for (int i = 0; i < 4; i++) off(h->_sections[i]);
  printf(" %d %d", (int)h->_sect, h->_rrnum);
  off(h->_msg_ptr);
}
static void text(const char *s) {
  putchar(' ');
  if (!*s) { printf("\\x"); return; }  /* an empty name, written so it is a token */
  for (; *s; s++) {
    unsigned char c = (unsigned char)*s;
    if (c < 0x21 || c > 0x7e || c == '\\') printf("\\x%02x", c); else putchar(c);
  }
}
static void msg(const char *nm, const unsigned char *m, int len, const int (*seq)[2], int nseq) {
  ns_msg h; memset(&h, 0, sizeof h);
  base = m; end = m + len;
  errno = 1234; int rc = ns_initparse(m, len, &h); int en = errno;
  printf("I %s = %d %d", nm, rc, en);
  if (rc == 0) handle(&h);
  putchar('\n');
  if (rc != 0) return;
  for (int k = 0; k < nseq; k++) {
    ns_rr r; memset(&r, 0, sizeof r); r.rdata = NULL;
    errno = 1234; rc = ns_parserr(&h, (ns_sect)seq[k][0], seq[k][1], &r); en = errno;
    printf("P %s %d %d = %d %d", nm, seq[k][0], seq[k][1], rc, en);
    if (rc == 0) { text(r.name); printf(" %u %u %u %u", r.type, r.rr_class, r.ttl, r.rdlength); off(r.rdata); }
    printf(" ;");
    handle(&h);
    putchar('\n');
  }
}
''']
    a = L.append
    a("int main(void) {")
    for nm, m in MESSAGES.items():
        seq = SEQS[nm]
        a(f"  {{ static const unsigned char m[] = {c_bytes(m)}; "
          f"static const int s[][2] = {{{','.join(f'{{{x},{y}}}' for x, y in seq)}}}; "
          f"msg(\"{nm}\", m, {len(m)}, s, {len(seq)}); }}")
    a(f"  {{ static const unsigned char m[] = {c_bytes(FULL)};")
    for o, s, c, e in SKIPS:
        a(f"    errno = 1234; {{ int rc = ns_skiprr(m + {o}, m + {e}, (ns_sect){s}, {c}); "
          f"printf(\"K full {o} {s} {c} {e} = %d %d\\n\", rc, errno); }}")
    a("  }")
    for nm, o, size in UNCOMPRESS:
        m = MESSAGES[nm]
        a(f"  {{ static const unsigned char m[] = {c_bytes(m)}; char out[1100]; memset(out, 0, sizeof out);"
          f" errno = 1234; int rc = ns_name_uncompress(m, m + {len(m)}, m + {o}, out, {size});"
          f" int en = errno; printf(\"U {nm} {o} {size} = %d %d\", rc, en);"
          f" if (rc >= 0) text(out); putchar('\\n'); }}")
    a("  return 0;")
    a("}")
    return "\n".join(L) + "\n"


def main():
    with workdir() as tmp:
        work = Path(tmp)
        (work / "ns_oracle.c").write_text(c_program(), encoding="utf-8", newline="\n")
        w = wsl_path(work)
        r = subprocess.run(["wsl", "-d", "Ubuntu", "--exec", "bash", "-c",
                            f"cd {w} && gcc -O0 -w -o ns_oracle ns_oracle.c -lresolv && ./ns_oracle"],
                           capture_output=True, timeout=300)
        if r.returncode != 0:
            sys.exit(f"oracle failed:\n{r.stderr.decode(errors='replace')[-4000:]}")
    lines = ["# glibc 2.39's ns_initparse, ns_parserr, ns_skiprr and ns_name_uncompress:",
             "# posix/tools/oracle/ns_harness.py. The messages (M), then glibc's answers."]
    lines += [f"M {nm} {m.hex() or '-'}" for nm, m in MESSAGES.items()]
    lines += r.stdout.decode("ascii").splitlines()
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(lines)} lines")


if __name__ == "__main__":
    main()
