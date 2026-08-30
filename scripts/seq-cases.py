#!/usr/bin/env python3
"""Generate the case file for ``scripts/seq-diff.sh``.

Every line is one invocation of ``seq``.  A line is a run of fields separated
by US (0x1f), which is used rather than a tab because bash's ``read -a``
collapses runs of *whitespace* separators and would therefore lose an empty
argument -- and ``seq -s ''`` is a case worth having.  The first field is the
case's name and the last is the sentinel ``END``, which exists because bash
drops a trailing empty field; everything between them is one argument.

Each argument is escaped so that ``printf %b`` reconstructs it exactly: a
backslash becomes ``\\\\`` and every byte outside printable ASCII becomes
``\\0ooo``.  That keeps the case file itself a line-oriented ASCII file --
which matters, because the harness reports a difference by echoing the case,
and a case containing a raw newline would be reported as two.

A case name beginning with ``x`` is one where we differ from GNU **on
purpose**; see ``scripts/seq-diff.sh``.

Usage:
    python scripts/seq-cases.py [RANDOM-CASES] [SEED]
"""

import math
import random
import sys

# ---------------------------------------------------------------------------
# encoding


def enc(arg):
    """Escape one argument into the case file's alphabet."""
    if isinstance(arg, str):
        arg = arg.encode("utf-8")
    out = bytearray()
    for b in arg:
        if b == 0x5C:  # backslash
            out += b"\\\\"
        elif 0x20 <= b < 0x7F:
            out.append(b)
        else:
            out += b"\\0%03o" % b
    return bytes(out)


CASES = []


def case(*args, name=None):
    CASES.append((name, [enc(a) for a in args]))


def xcase(*args):
    """A case we expect to differ from GNU."""
    case(*args, name="x")


# ---------------------------------------------------------------------------
# how many lines a case prints, so that the generator never emits one that
# does not stop.  `seq 1 inf` is a legitimate thing to ask for and an
# illegitimate thing to put in a test that captures stdout to a file.


def numeric(s):
    try:
        return float(s)
    except ValueError:
        pass
    try:
        return float.fromhex(s.strip())
    except (ValueError, OverflowError):
        return None


def bounded(operands, limit=500):
    """True if `seq OPERANDS` prints at most `limit` lines."""
    values = [numeric(o) for o in operands]
    if any(v is None or math.isnan(v) for v in values):
        return False
    if len(operands) == 1:
        first, step, last = 1.0, 1.0, values[0]
    elif len(operands) == 2:
        first, step, last = values[0], 1.0, values[1]
    else:
        first, step, last = values
    if step == 0 or any(math.isinf(v) for v in (first, step)):
        return False
    if math.isinf(last):
        # An infinite endpoint terminates only when it is behind the start.
        return (step > 0) != (last > 0)
    if (last - first) * (1 if step > 0 else -1) < 0:
        return True
    return (last - first) / step + 1 <= limit


# ---------------------------------------------------------------------------
# curated cases
#
# `--help` and `--version` are deliberately absent.  Both print text that
# names the implementation, so they can never match GNU byte for byte, and a
# permanent expected-difference for them would only be noise: what they
# actually have to satisfy is that they go to stdout with status 0, which the
# unit tests check.

# the three operand forms
case("5")
case("0")
case("1")
case("-1")
case("2", "7")
case("7", "2")
case("2", "2")
case("1", "2", "9")
case("1", "1", "1")
case("9", "-2", "1")
case("1", "-2", "9")
case("-3", "3")
case("-3", "1", "3")
case("0", "0.25", "1")
case("1", "0.1", "2")

# width and precision come from how the operand was *written*
for spelling in [
    "1.0",
    "1.00",
    "1.",
    ".5",
    "-.5",
    "+1",
    "+1.5",
    "0.10",
    "00001",
    "1e1",
    "1E1",
    "1e+1",
    "1e-1",
    "1.5e2",
    "1.5E2",
    "1.50e2",
    "1e0",
    "0.0001",
    "1e-4",
    "12.345e1",
    "12.345e-1",
    "1e2",
    "1.e1",
    ".5e1",
    "0x10",
    "0X10",
    "0x1p4",
    "0X1P+4",
    "0x1.8p3",
]:
    if bounded([spelling, "1", "20"]):
        case(spelling, "1", "20")
    if bounded(["1", spelling, "20"]):
        case("1", spelling, "20")
    if bounded(["1", "1", spelling]):
        case("1", "1", spelling)

# leading blanks and signs are skipped before the spelling is measured
case(" 1", "1", "5")
case("\t1", "1", "5")
case("  +1.5", "1", "5")
case("+ 1")

# equal width
for operands in [
    ["1", "10"],
    ["-1", "1", "3"],
    ["0.1", "0.1", "1"],
    ["8", "12"],
    ["1e2", "1", "102"],
    ["-5", "5"],
    ["95", "105"],
    ["-105", "10", "-95"],
    ["0", "0.5", "3"],
    ["1.5", "1", "10.5"],
    ["-0.5", "0.5", "0.5"],
    ["100"],
    ["0.001", "0.001", "0.005"],
]:
    case("-w", *operands)
    case("--equal-width", *operands)

# formats
for fmt in [
    "%g",
    "%f",
    "%e",
    "%a",
    "%E",
    "%G",
    "%F",
    "%A",
    "%.0f",
    "%.3f",
    "%.15g",
    "%10.2f",
    "%-10.2fX",
    "%+f",
    "% f",
    "%#g",
    "%#.0f",
    "%05.1f",
    "%.0e",
    "%Lf",
    "%Lg",
    "%L.2f",
    "[%g]",
    "x%gy",
    "%%%g%%",
    "%%g",
    "%g%%",
    "  %g  ",
    "%1$g",
]:
    case("-f", fmt, "1", "0.5", "3")
    case("--format=" + fmt, "1", "3")

# formats that seq refuses.  These four diagnostics are the only ones in seq
# that carry no `Try 'seq --help'` line, which is exactly the sort of thing a
# reimplementation gets wrong and a differential test catches.
for fmt in ["", "abc", "%", "%d", "%s", "%g%g", "%%", "%%%", "%-", "%.", "%L", "%zg", "%'g"]:
    case("-f", fmt, "1", "3")
# ...and where the unknown directive is a byte that cannot be printed.  We
# escape it as \ooo; GNU writes the raw byte, which lets a format string put a
# newline into seq's own diagnostic.  See the head of seq.rs.
xcase("-f", "%\n", "1", "3")
xcase("-f", "%\x01", "1", "3")
xcase("-f", b"%\xc3\xa9", "1", "3")

# separators
for sep in [",", "", " ", "--", "\t", "\n", "\n\n", "%g", "\x01"]:
    case("-s", sep, "1", "4")
    case("--separator=" + sep, "1", "4")
case("-s", ",", "-w", "8", "12")
case("-w", "-s", "", "8", "12")

# option syntax, which is glibc's getopt_long and not ours
case("--equal-width", "3")
case("--equal", "3")
case("--e", "3")
case("--eq", "3")
case("--f", "%g", "3")
case("--s", ",", "3")
case("--=x", "3")
case("--", "3")
case("--", "-3")
case("-", "3")
case("-f%g", "1", "3")
case("-s,", "1", "3")
case("-wf", "%g", "1", "3")
case("-fw", "1", "3")
case("--format", "%g", "1", "3")
case("--separator", ",", "1", "3")
case("--format")
case("--separator")
case("-f")
case("-s")
case("--zzz", "3")
case("-x", "3")
case("-3")
case("-.5", "1", "3")
case("-0.5", "0.5", "0.5")

# options stop at the first operand, so everything after it is an operand
case("1", "-w", "3")
case("1", "--version")
case("1", "--help")
case("1", "-f", "%g", "3")
case("3", "-1", "1")

# refusals
case()
case("1", "2", "3", "4")
case("1", "2", "3", "4", "5")
case("abc")
case("")
case(" ")
case("1abc")
case("1", "abc")
case("nan")
case("-nan")
case("NaN")
case("1", "nan", "3")
case("1", "3", "nan")
case("1", "0", "3")
case("1", "-0", "3")
case("1", "0.0", "3")
case("-w", "-f", "%g", "1", "3")
case("-f", "%g", "-w", "1", "3")
case("inf", "1")
case("1", "-1", "inf")
case("-inf", "-1", "-3")
case("1", "1", "-inf")
case("infinity", "1")
case("0x")
case("1e")
case("1e+")
case(".")
case("+")
case("1", "2", "")

# the integer fast paths: decimal counting that is exact past what a float can
# hold, and the step limit that makes it stop being used
case("18446744073709551615", "18446744073709551625")
case("18446744073709551615", "3", "18446744073709551625")
case("99999999999999999999999999999", "100000000000000000000000000005")
case("999999999999999999999999999999", "1", "1000000000000000000000000000005")
case("1", "200", "2001")
case("1", "201", "2001")
case("1", "200", "1")
case("0", "1", "10")
case("007", "1", "12")
case("0000", "3")
case("1e3", "1", "1005")
case("1e3", "1e3")
case("00000000000000000000001", "5")
case("1", "1", "1e1")

# the number past the end, which seq prints when it reads back as the end
case("0", "0.000001", "0.000003")
case("1", "0.1", "1.5")
case("-0.1", "0.1", "0.3")
case("0", "0.1", "1")
case("1", "0.0000001", "1.0000005")
case("0.1", "0.1", "0.5")
case("1", "0.3", "2")
case("-f", "%.2f", "0", "0.000001", "0.000003")
case("-s", ",", "0", "0.000001", "0.000003")

# ---------------------------------------------------------------------------
# random cases

SPELLINGS = [
    "0", "1", "2", "3", "5", "10", "-1", "-3", "0.5", "1.5", ".5", "1.",
    "+2", "1e1", "1e-1", "2E0", "0x10", "0x1p4", "000", "007", "1.25",
    "3.14159", "-0.5", "100", "2.5e2", "1e2", "0.001", "1e-3", "-1.5",
    "12345678901234567890", "9007199254740993", "0.1", "0.30", "-.25",
]

STEPS = [
    "1", "2", "0.5", "0.25", "-1", "-0.5", "3", "0.1", "1e-1", "0.3",
    "2.5", "-2", "1.5", "0.125", "7", "-0.25", "1.0", "100",
]

OPTIONS = [
    [], [], [], ["-w"], ["-w"], ["-s", ","], ["-s", ""], ["-s", "|"],
    ["-f", "%g"], ["-f", "%.2f"], ["-f", "%e"], ["-f", "%a"],
    ["-f", "[%05.2f]"], ["-w", "-s", ":"], ["-f", "%#.3g"],
    ["-f", "%+.1e"], ["--format=%.10g"], ["--separator=--"],
    ["-f", "%G"], ["-f", "%A"], ["-f", "%F"], ["-f", "%-8.3fX"],
    ["-f", "%.20f"], ["-f", "%.0Lf"], ["-w", "-s", ""],
]


def random_cases(count, seed):
    rng = random.Random(seed)
    made = 0
    tries = 0
    while made < count and tries < count * 100:
        tries += 1
        shape = rng.randrange(3)
        if shape == 0:
            operands = [rng.choice(SPELLINGS)]
        elif shape == 1:
            operands = [rng.choice(SPELLINGS), rng.choice(SPELLINGS)]
        else:
            operands = [
                rng.choice(SPELLINGS),
                rng.choice(STEPS),
                rng.choice(SPELLINGS),
            ]
        if not bounded(operands):
            continue
        case(*(rng.choice(OPTIONS) + operands))
        made += 1


def main():
    count = int(sys.argv[1]) if len(sys.argv) > 1 else 400
    seed = int(sys.argv[2]) if len(sys.argv) > 2 else 20260817
    random_cases(count, seed)

    us = b"\x1f"
    out = sys.stdout.buffer
    for i, (mark, args) in enumerate(CASES, start=1):
        name = (b"x%d" % i) if mark == "x" else (b"%d" % i)
        out.write(us.join([name] + args + [b"END"]) + b"\n")
    out.flush()


if __name__ == "__main__":
    main()
