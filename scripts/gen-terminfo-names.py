#!/usr/bin/env python3
"""Write userspace/terminfo/src/names.rs: the capability names, from the reference's libtinfo.

`term.h` numbers each capability -- `bw` is boolean 0, `cols` number 0, `cbt`
string 0 -- and libtinfo exports the names in that order: `boolnames`,
`numnames` and `strnames` (terminfo's), `boolcodes`, `numcodes` and
`strcodes` (termcap's two letters), and the C variable names
(`boolfnames`...). This compiles a few lines of C against the installed
library -- Ubuntu 24.04's ncurses 6.4+20240113 in WSL -- that print those
arrays, and writes them out as Rust tables, so the names are the
reference's own rather than a copy of a list.

    wsl -- python3 scripts/gen-terminfo-names.py

It overwrites the file and prints the counts; nothing else reads it.
"""
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DST = os.path.join(HERE, "..", "userspace", "terminfo", "src", "names.rs")

PROGRAM = r"""
#include <stdio.h>
#include <term.h>
static void dump(char t, NCURSES_CONST char *const *n, NCURSES_CONST char *const *c,
                 NCURSES_CONST char *const *f) {
    for (int i = 0; n[i]; i++)
        printf("%c\t%s\t%s\t%s\n", t, n[i], c[i], f[i]);
}
int main(void) {
    dump('b', boolnames, boolcodes, boolfnames);
    dump('n', numnames, numcodes, numfnames);
    dump('s', strnames, strcodes, strfnames);
    return 0;
}
"""


def rust_str(s):
    out = []
    for ch in s:
        if ch in '\\"':
            out.append("\\" + ch)
        elif 0x20 <= ord(ch) < 0x7f:
            out.append(ch)
        else:
            out.append("\\x%02x" % ord(ch))
    return '"' + "".join(out) + '"'


def table(name, doc, items):
    rows = "\n".join("    " + rust_str(x) + "," for x in items)
    return f"/// {doc}\npub const {name}: [&str; {len(items)}] = [\n{rows}\n];\n"


def main():
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "names.c")
        exe = os.path.join(tmp, "names")
        with open(src, "w", encoding="utf-8", newline="") as f:
            f.write(PROGRAM)
        subprocess.run(["gcc", "-o", exe, src, "-ltinfo"], check=True)
        text = subprocess.run([exe], check=True, capture_output=True, text=True, encoding="utf-8").stdout
    rows = {"b": [], "n": [], "s": []}
    for line in text.splitlines():
        t, name, code, var = line.split("\t")
        rows[t].append((name, code, var))
    parts = []
    for t, kind in (("b", "BOOL"), ("n", "NUM"), ("s", "STR")):
        r = rows[t]
        what = {"b": "boolean", "n": "numeric", "s": "string"}[t]
        parts.append(table(f"{kind}NAMES", f"`{kind.lower()}names`: the {what} capabilities' terminfo names, by index.", [x[0] for x in r]))
        parts.append(table(f"{kind}CODES", f"`{kind.lower()}codes`: their termcap names.", [x[1] for x in r]))
        parts.append(table(f"{kind}FNAMES", f"`{kind.lower()}fnames`: their C variable names.", [x[2] for x in r]))
        print(f"{kind}: {len(r)}")
    header = (
        "//! The capability names, by the index `term.h` gives each: generated from\n"
        "//! the reference's libtinfo (Ubuntu 24.04's ncurses 6.4+20240113) by\n"
        "//! `scripts/gen-terminfo-names.py`, which prints its `boolnames`,\n"
        "//! `boolcodes`, `boolfnames` and the rest. Not edited by hand: rerun the\n"
        "//! script.\n\n"
    )
    with open(DST, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + "\n".join(parts))
    print("wrote", os.path.normpath(DST))
    return 0


if __name__ == "__main__":
    sys.exit(main())
