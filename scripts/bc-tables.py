#!/usr/bin/env python3
"""Write `userspace/gnubc/src/tables.rs`: the parse tables of GNU bc 1.07.1's
generated parser (`bc/bc.c`, Bison 3.0.2, as Ubuntu's patched source ships
it), so that the port parses -- and recovers from syntax errors -- exactly as
the C parser does.

Usage:
    python scripts/bc-tables.py BC_C OUT
    python scripts/bc-tables.py --self-test

BC_C is the patched source's `bc/bc.c`; OUT the Rust file to write. The
tables are copied as numbers, never retyped: `yytranslate`, `yypact`,
`yydefact`, `yypgoto`, `yydefgoto`, `yytable`, `yycheck`, `yyr1`, `yyr2`, and
the constants that size them.
"""

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

TABLES = ["yytranslate", "yypact", "yydefact", "yypgoto", "yydefgoto",
          "yytable", "yycheck", "yyr1", "yyr2"]
CONSTS = ["YYFINAL", "YYLAST", "YYNTOKENS", "YYNNTS", "YYNRULES", "YYNSTATES",
          "YYUNDEFTOK", "YYMAXUTOK", "YYPACT_NINF", "YYTABLE_NINF"]


def extract(text):
    """The tables and constants of a Bison-generated parser's source."""
    tables = {}
    for name in TABLES:
        m = re.search(r"static const \w+ " + name + r"\[\] =\s*\{(.*?)\};", text, re.S)
        if not m:
            raise ValueError("no table " + name)
        body = re.sub(r"/\*.*?\*/", "", m.group(1), flags=re.S)
        tables[name] = [int(v) for v in re.findall(r"-?\d+", body)]
    consts = {}
    for name in CONSTS:
        m = re.search(r"#define " + name + r"\s+\(?(-?\d+)\)?", text)
        if not m:
            raise ValueError("no constant " + name)
        consts[name] = int(m.group(1))
    return tables, consts


def render(tables, consts):
    """The Rust module."""
    out = ["//! The parse tables of GNU bc 1.07.1's generated parser (`bc/bc.c`,",
           "//! Bison 3.0.2), copied by `scripts/bc-tables.py`. Do not edit by hand.",
           ""]
    for name in CONSTS:
        out.append(f"/// `{name}`.")
        out.append(f"pub const {name}: i32 = {consts[name]};")
    for name in TABLES:
        vals = tables[name]
        out.append("")
        out.append(f"/// `{name}`.")
        out.append(f"pub const {name.upper()}: [i16; {len(vals)}] = [")
        for i in range(0, len(vals), 12):
            out.append("    " + ", ".join(str(v) for v in vals[i:i + 12]) + ",")
        out.append("];")
    return "\n".join(out) + "\n"


def self_test():
    sample = """
#define YYFINAL  2
#define YYLAST   7
#define YYNTOKENS  5
#define YYNNTS  2
#define YYNRULES  3
#define YYNSTATES  4
#define YYUNDEFTOK  2
#define YYMAXUTOK   259
#define YYPACT_NINF -3
#define YYTABLE_NINF -1
static const yytype_uint8 yytranslate[] = { 0, 2, /* x */ 3 };
static const yytype_int16 yypact[] = { -3, 1 };
static const yytype_uint8 yydefact[] = { 2, 0 };
static const yytype_int16 yypgoto[] = { -3 };
static const yytype_int16 yydefgoto[] = { -1, 1 };
static const yytype_int16 yytable[] = { 3, -1 };
static const yytype_int16 yycheck[] = { 1, 2 };
static const yytype_uint8 yyr1[] = { 0, 5 };
static const yytype_uint8 yyr2[] = { 0, 2 };
"""
    tables, consts = extract(sample)
    ok = tables["yytranslate"] == [0, 2, 3] and consts["YYPACT_NINF"] == -3
    ok = ok and "pub const YYTABLE: [i16; 2] = [" in render(tables, consts)
    print("bc-tables: self-test", "passed" if ok else "FAILED")
    return 0 if ok else 1


def main(argv):
    if selftestflag.wants_selftest(argv):
        return self_test()
    if len(argv) != 3:
        print(__doc__.split("\n\n")[1], file=sys.stderr)
        return 2
    text = Path(argv[1]).read_text(encoding="utf-8")
    tables, consts = extract(text)
    Path(argv[2]).write_text(render(tables, consts), encoding="utf-8", newline="\n")
    print(f"bc-tables: wrote {argv[2]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
