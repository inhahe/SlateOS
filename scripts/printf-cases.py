#!/usr/bin/env python3
"""Generate the case file for ``scripts/printf-diff.sh``.

The file format is the one ``scripts/seq-cases.py`` writes, and for the same
reasons: every line is one invocation, fields are separated by US (0x1f)
rather than a tab because bash's ``read -a`` collapses runs of *whitespace*
separators and would lose an empty argument -- and ``printf ''`` is a case.
The first field is the case's name, the last is the sentinel ``END`` (bash
drops a trailing empty field), and everything between them is one argument,
escaped so that ``printf %b`` reconstructs it byte for byte.

``printf`` needs that escaping more than ``seq`` did.  Its first argument is a
format string, and the interesting formats are exactly the ones containing
bytes -- a newline, a NUL, a lone ``\\`` -- that no quoting scheme survives
across the two shells and the Win32 command-line encoder that sit between this
generator and the two binaries under test.

A case name beginning with ``x`` is one where we differ from GNU **on
purpose**; see ``scripts/printf-diff.sh``.

Every argument here is valid UTF-8, and that is a limitation of the *harness*
rather than a claim about ``printf``.  Our side is a native Windows binary, so
its argv arrives as UTF-16 and MSYS must transcode each argument on the way
in; a byte sequence that is not valid UTF-8 does not survive the round trip
(``\xff\xfe`` comes back as ``\xc3\xbf\xc3\xbe``).  The mangling happens
outside our code -- an MSYS-native program invoked with the same argument sees
the original bytes, and on the target OS, where argv is bytes, the question
does not arise -- so a case like ``printf %s $'\xff'`` would measure the
Windows command line and not this program.  Non-UTF-8 bytes are still tested
where they can be: in the *format*, where they arrive as escapes the program
itself decodes, and in ``cfmt``'s unit tests, which take a byte slice
directly.

Usage:
    python scripts/printf-cases.py [RANDOM-CASES] [SEED]
"""

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
# curated cases
#
# `--help` and `--version` are absent for the reason they are absent from the
# seq harness: both print text naming the implementation, so they can never
# match byte for byte, and carrying them as permanent expected-differences
# would only train the reader to skim the report.  What they have to satisfy
# -- stdout, status 0 -- the unit tests in printf.rs check.

# ---- text with no directives at all ---------------------------------------
case("")
case("hello")
case("hello\n")
case("100%")
case("a\tb\n")
case("no-directive", "x", "y")          # excess arguments, status 0
case("\n")
case("\\")
case("a\\")

# ---- %% is a literal percent ----------------------------------------------
case("%%")
case("%%\n")
case("%%d", "5")
case("100%%\n")
case("%%%s\n", "a")
case("%d%%\n", "5")

# ---- strings ---------------------------------------------------------------
for fmt in ["%s", "%s\n", "[%s]", "%5s|", "%-5s|", "%.1s", "%.0s|", "%5.1s|",
            "%5.2s|", "%.3s", "%-8.2s|", "%1s|"]:
    case(fmt, "abcdef")
    case(fmt, "")
    case(fmt, "ab")
case("%s")                              # no argument at all
case("%s %s\n", "a")
case("%s", "-n")
case("%s", "--")
case("%s", " leading and trailing ")
case("%s", "a\nb")
case("%s", b"\xc3\xa9")

# ---- chars -----------------------------------------------------------------
for fmt in ["%c", "[%c]", "%5c|", "%-5c|", "%c%c"]:
    case(fmt, "abc", "def")
    case(fmt, "")
case("%c", "'a")
case("%c%c", "a", "b")
case("%c")

# ---- signed integers -------------------------------------------------------
for value in ["0", "1", "-1", "5", "-5", "42", "0x10", "0X10", "010", "0b101",
              "0B11", "+5", " 12", " 0x1f", "0o17", "9223372036854775807",
              "-9223372036854775808", "99999999999999999999999", "",
              "abc", "12x", "0x", "0b", "-0x10", "\t7", "  -3", "1 ", "+"]:
    case("%d\n", value)
case("%i\n", "17")
case("%d %i\n", "3", "4")

# ---- unsigned, octal, hex --------------------------------------------------
for fmt in ["%u\n", "%o\n", "%x\n", "%X\n"]:
    for value in ["0", "1", "255", "-1", "-16", "18446744073709551615",
                  "18446744073709551616", "-99999999999999999999999",
                  "0x10", "010"]:
        case(fmt, value)

# ---- the flag / width / precision table -----------------------------------
for fmt in ["%.0d", "%.0d|", "%.d|", "%.3d", "%5.3d|", "%-5.3d|", "%05d|",
            "%05.2d|", "%-05d|", "%+d", "% d", "%+.3d", "%-+8.3d|", "%08.3d|",
            "% .3d", "%+-6d|", "%6d|", "%-6d|", "%'d", "%ld", "%lld", "%hd",
            "%jd", "%zd", "%td", "%Ld"]:
    for value in ["0", "5", "-5", "12345"]:
        case(fmt + "\n", value)

for fmt in ["%.0o", "%#.0o", "%#o", "%#x", "%#X", "%#5x|", "%#-5x|", "%#05x|",
            "%+u", "% u", "%+x", "%+o", "%#.5x", "%.5o", "%08x|", "%'u", "%'x"]:
    for value in ["0", "1", "255"]:
        case(fmt + "\n", value)

# ---- * width and precision -------------------------------------------------
case("%*d|\n", "6", "5")
case("%*d|\n", "-6", "5")
case("%*d|\n", "0", "5")
case("%.*f\n", "2", "1.5")
case("%.*f\n", "-1", "1.5")
case("%.*f\n", "0", "1.5")
case("%*.*f|\n", "10", "2", "1.5")
case("%*.*f|\n", "-10", "2", "1.5")
case("%*s|\n", "6", "ab")
case("%*c|\n", "4", "x")
case("%*d|\n", "6")                     # star with no argument left
case("%.*d|\n", "3")
case("%*d|\n", "abc", "5")
# The in-range end of the width limit is deliberately absent: `%*d` with
# 2147483647 is legal and would have both binaries write a two-gigabyte line.
# What is worth testing is the rejection just past it.
case("%*d|\n", "2147483648", "5")       # out of range for an int
case("%.*d|\n", "2147483648", "5")
case("%*d|\n", "-2147483648", "5")

# ---- floating point, which is extfloat's half ------------------------------
for fmt in ["%f", "%F", "%e", "%E", "%g", "%G", "%a", "%A", "%.0f", "%.3f",
            "%.20f", "%10.2f|", "%-10.2f|", "%+f", "% f", "%#g", "%#.0f",
            "%05.1f|", "%.0e", "%Lf", "%Le", "%'f", "%.17g", "%.1a"]:
    for value in ["1.5", "0.1", "0", "-0", "1e5000", "1e-5000", "3.14159265358979323846",
                  "inf", "-inf", "nan", "0x1p4", "1e10", "-2.5"]:
        case(fmt + "\n", value)
case("%f\n", "abc")
case("%f\n", "")
case("%f\n", "1.5x")
case("%f\n", ".")
case("%f\n", "1e")

# ---- %b, whose escapes are not the format's --------------------------------
for arg in ["a\\tb", "a\\tb\\0101", "\\0", "\\01", "\\101", "\\0101", "\\x41",
            "100%", "a\\c b", "\\\\", "plain", "", "\\n", "\\z", "a\\",
            "\\0777", "\\400"]:
    case("%b|", arg)
case("%b %b\n", "a\\tb", "c")
case("[%b]\n", "\\u0041")

# ---- %q ---------------------------------------------------------------------
for arg in ["plain", "a b", "a'b", "a\"b", "", "a\\b", "-n", "~x", "a\nb",
            "*", "$x", "#c", "a=b", b"\xc3\xa9", b"\x01"]:
    case("%q\n", arg)
case("%q %q\n", "a b", "c d")

# ---- escapes in the format -------------------------------------------------
for fmt in ["\\101", "\\0101", "\\x41", "\\x4", "\\x41x", "\\xff", "\\a", "\\b",
            "\\e", "\\f", "\\n", "\\r", "\\t", "\\v", "\\\\", "\\\"", "\\z",
            "\\", "\\0", "\\00", "\\000", "\\1", "\\7", "\\8", "\\400",
            "\\u0041", "\\u0001", "\\u0000", "\\u007f", "\\u0080", "\\u00ff",
            "\\u041", "\\U0001F600", "\\U00000041", "\\U0041", "\\ud800",
            "\\udfff", "\\uD800", "a\\tb\\nc"]:
    case(fmt + "|")
case("\\xZ")
case("\\x")

# ---- \c is an exit, not a break --------------------------------------------
case("a\\cb")
case("%s\\c%s\n", "x", "y")
case("%d\\c", "abc")                    # cancel outranks the numeric failure
case("%b", "a\\cb")
case("\\c")

# ---- character constants ----------------------------------------------------
for arg in ["'a", "'ab", "'abc", '"a', "'", '"', "''", "'0", "'-", "' ",
            "\"''\"", "\"'\""]:
    case("%d\n", arg)
case("%x\n", "'a")
case("%f\n", "'a")
case("%c\n", "'a")

# ---- invalid conversion specifications, which are fatal --------------------
for fmt in ["%", "%z", "%'e", "%#d", "%0s", "%#s", "%.1c", "%-", "%.", "%5",
            "%l", "%L", "%#c", "%0c", "%'s", "%'c", "%Is", "%5b", "%-q",
            "%.2q", "%#b", "%$s", "%1$s", "%-.2c", "%0b"]:
    case(fmt, "1")
case("a%zb")                            # the text before a fatal spec is kept
case("%d%z", "5")
case("%s%", "a")

# ...and where the byte that makes it invalid cannot be printed.  We escape it
# as \ooo; GNU writes it raw, which lets a format string put a newline (or a
# terminal escape sequence) into printf's own diagnostic.  See printf.rs.
xcase("%\n")
xcase("%\x01")
xcase(b"%\xc3\xa9")
xcase("%\x1b[31m")
# The same divergence in the other sentence that echoes caller bytes: the tail
# of a character constant.
xcase("%d", b"'a\xc3\xa9")
xcase("%d", b"'\xc3\xa9")
xcase("%d", "'a\nb")

# ---- format reuse -----------------------------------------------------------
case("%s %s\n", "a", "b", "c", "d")
case("%s-%s\n", "a", "b", "c")
case("%d\n", "1", "2", "3")
case("%s\n", "a", "b", "c", "d", "e")
case("[%s]", "a", "b", "c")
case("%s %s %s\n", "a", "b", "c", "d")
case("x\n", "a", "b")                   # consumes nothing: one pass, then warn
case("%%\n", "a")
case("\\n", "a")
case("%b\n", "a", "b")
case("%q\n", "a", "b")
case("%d %s\n", "1", "a", "2", "b")

# ---- operands and option syntax ---------------------------------------------
case()
case("--")
case("--", "%s", "a")
case("-x")
case("-n")
case("-e")
case("--foo")
case("-")
case("--", "--")
case("--", "-n")

# ---- a NUL on stdout ---------------------------------------------------------
# Only ever as an *escape*: no argument can contain a NUL, because argv cannot,
# so `case("%s", "\x00")` would be an obfuscated spelling of the empty string.
case("\\0|\\0|")
case("%b|", "\\0")
case("%c|%c|", "", "")

# ---------------------------------------------------------------------------
# random cases
#
# The point of the random half is the interaction between the pieces: a flag
# with a precision with a width with a length modifier is where a
# hand-transcribed table goes wrong, and there are more combinations than are
# worth writing out.

FLAGS = ["", "", "", "-", "+", " ", "#", "0", "'", "-0", "+ ", "#0", "-+",
         "0#", "+#", "-#", "' "]
WIDTHS = ["", "", "", "1", "3", "5", "8", "12", "20", "0", "*"]
PRECS = ["", "", "", ".", ".0", ".1", ".3", ".5", ".10", ".*"]
LENGTHS = ["", "", "", "", "l", "ll", "h", "hh", "L", "j", "z", "t"]
CONVS = list("diouxXfFeEgGaAcsbq")

LITERALS = ["", "|", " ", "x", "\\t", "\\n", "[", "]", "%%", "-", "\\\\"]

ARGS = [
    "0", "1", "-1", "5", "-5", "42", "255", "3", "2", "-3", "7",
    "0x10", "010", "0b101", "+5", " 12", "-0x10",
    "9223372036854775807", "-9223372036854775808", "18446744073709551615",
    "99999999999999999999999", "-99999999999999999999999",
    "1.5", "0.1", "-2.5", "1e10", "1e-10", "3.14159265358979323846",
    "1e5000", "1e-5000", "inf", "-inf", "nan", "0x1p4", "0.0", "-0.0",
    "abc", "12x", "", " ", "a b", "hello", "a'b", "a\\tb", "\\101", "\\x41",
    "'a", "'ab", "\"a", "-n", "\\0101", "\\c",
]


def random_format(rng):
    """One format string: literal, directive, literal, ... in random amounts."""
    out = [rng.choice(LITERALS)]
    for _ in range(rng.randrange(1, 4)):
        out.append(
            "%"
            + rng.choice(FLAGS)
            + rng.choice(WIDTHS)
            + rng.choice(PRECS)
            + rng.choice(LENGTHS)
            + rng.choice(CONVS)
        )
        out.append(rng.choice(LITERALS))
    return "".join(out)


def random_cases(count, seed):
    rng = random.Random(seed)
    for _ in range(count):
        fmt = random_format(rng)
        args = [rng.choice(ARGS) for _ in range(rng.randrange(0, 5))]
        case(fmt, *args)


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
