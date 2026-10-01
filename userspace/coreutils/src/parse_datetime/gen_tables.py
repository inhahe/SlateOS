#!/usr/bin/env python3
"""Regenerate tables.rs from the Bison output GNU ships.

Usage:
    python gen_tables.py PATH/TO/lib/parse-datetime.c > tables.rs

The input is the `lib/parse-datetime.c` in a coreutils release tarball, which
is what `bison` made of gnulib's `parse-datetime.y` for that release. The
tables are copied rather than rebuilt because the parser's behaviour on every
input it does not accept -- and on the 31 shift/reduce conflicts the grammar
declares -- is a property of *those* tables, including which states reduce by
default without reading a token. A table built by any other generator would be
a different parser that happens to agree on the easy inputs.

The script refuses anything it does not recognise, so that a Bison upgrade
which renames or retypes a table fails here rather than producing a Rust file
that compiles and parses differently.
"""

import hashlib
import re
import sys

# The arrays the LALR driver reads. Each keeps the element type Bison chose
# for it (`yytype_int8` for every one of these, for this grammar), so that the
# driver's arithmetic on them is the arithmetic yacc.c does; `array` refuses a
# table Bison typed differently, and `emit` checks every value fits.
ARRAYS = [
    ("yypact", "i8"),
    ("yydefact", "i8"),
    ("yypgoto", "i8"),
    ("yydefgoto", "i8"),
    ("yytable", "i8"),
    ("yycheck", "i8"),
    ("yyr1", "i8"),
    ("yyr2", "i8"),
]

SCALARS = [
    "YYFINAL",
    "YYLAST",
    "YYNTOKENS",
    "YYNNTS",
    "YYNRULES",
    "YYNSTATES",
    "YYPACT_NINF",
    "YYTABLE_NINF",
]

RANGES = {"i8": (-128, 127), "u8": (0, 255)}


def die(msg):
    sys.stderr.write("gen_tables.py: " + msg + "\n")
    sys.exit(1)


def scalar(src, name):
    m = re.search(r"^#define %s\s+\(?(-?\d+)\)?\s*$" % name, src, re.M)
    if not m:
        die("no #define for " + name)
    return int(m.group(1))


def array(src, name):
    m = re.search(r"static const yytype_int8 %s\[\] =\s*\{([^}]*)\};" % name, src)
    if not m:
        die("no yytype_int8 table named " + name)
    return [int(v) for v in m.group(1).replace("\n", " ").split(",") if v.strip()]


# Scalars the driver has no use for: they describe the tables' shape, and only
# the tests read them. `YYTABLE_NINF` is among them because
# `yytable_value_is_error` is constant 0 for these tables (checked in main),
# so the driver never compares against it.
#
# Named one by one rather than covered by a module-wide `#![allow(dead_code)]`:
# that would hide an ARRAY the driver stopped reading, which is a real finding
# (scripts/check-dead-code-allows.py refuses the module-wide form).
SHAPE_ONLY = {"YYNNTS", "YYNRULES", "YYNSTATES", "YYTABLE_NINF"}


def emit(name, ty, values, out):
    lo, hi = RANGES[ty]
    for v in values:
        if not lo <= v <= hi:
            die("%s holds %d, which does not fit %s" % (name, v, ty))
    # Twelve to a line, as generated: rustfmt would repack them, and a table
    # that changes shape when reformatted is harder to diff against its source.
    out.append("#[rustfmt::skip]")
    out.append("pub(super) const %s: [%s; %d] = [" % (name.upper(), ty, len(values)))
    for i in range(0, len(values), 12):
        out.append("    " + ", ".join(str(v) for v in values[i : i + 12]) + ",")
    out.append("];")
    out.append("")


def main():
    if len(sys.argv) != 2:
        die("usage: gen_tables.py PATH/TO/parse-datetime.c")
    raw = open(sys.argv[1], "rb").read()
    src = raw.decode("utf-8")
    if "A Bison parser, made by GNU Bison" not in src:
        die("not Bison output")
    bison = re.search(r"made by GNU Bison (\d+(?:\.\d+)*)", src).group(1)
    # The driver in grammar.rs is yacc.c's; these two facts are what it
    # assumes about the table encoding.
    if "#define yytable_value_is_error(Yyn) \\\n  0" not in src:
        die("yytable_value_is_error is not constant 0; the driver assumes it is")
    if "#define yypact_value_is_default(Yyn) \\\n  ((Yyn) == YYPACT_NINF)" not in src:
        die("yypact_value_is_default has changed shape")

    out = [
        "//! The LALR(1) tables of GNU's `parse-datetime.y`, as Bison built them.",
        "//!",
        "//! **Generated -- do not edit.** Regenerate with",
        "//! `python gen_tables.py lib/parse-datetime.c > tables.rs` from the",
        "//! coreutils release tarball's `lib/parse-datetime.c`; see that script",
        "//! for why the tables are copied rather than rebuilt.",
        "//!",
        "//! Source: `lib/parse-datetime.c`, GNU Bison %s," % bison,
        "//! sha256 `%s`." % hashlib.sha256(raw).hexdigest(),
        "",
        "// The driver in grammar.rs reads every array and most of the scalars.",
        "// The four that only describe the tables' shape -- which the tests check",
        "// the arrays against -- carry their own `allow`, item by item, so that",
        "// anything the driver stops reading is still reported.",
        "",
    ]
    for name in SCALARS:
        ty = "i8" if name.endswith("NINF") else "usize"
        if name in SHAPE_ONLY:
            out.append("#[cfg_attr(not(test), allow(dead_code))]")
        out.append("pub(super) const %s: %s = %d;" % (name, ty, scalar(src, name)))
    out.append("")
    for name, ty in ARRAYS:
        emit(name, ty, array(src, name), out)
    # Bytes, not text: text-mode stdout on Windows would write CRLF.
    sys.stdout.buffer.write(("\n".join(out).rstrip("\n") + "\n").encode("ascii"))


main()
