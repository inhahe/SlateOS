#!/usr/bin/env python3
"""Generate cases for `scripts/extfloat-diff.sh`.

Two case files, because reading and writing fail differently and a harness that
only checked the round trip would let a parse error and a formatting error
cancel out:

    extfloat-cases.py read   > read.txt    # one numeral per line
    extfloat-cases.py write  > write.txt   # FORMAT<TAB>numeral per line

The generator is *seeded*, so a difference the fuzz finds can be reproduced by
re-running it, and so a run that finds nothing means the same thing twice.

The interesting inputs are not uniform random doubles. They are the places
where a 64-bit significand and a decimal expansion disagree about where a value
lands: numerals whose length crosses 19 digits (the point past which a `u64`
cannot hold the digits), exponents at the edges of the exponent range, values a
half-ulp from a rounding boundary, and the subnormal floor -- plus the plainly
malformed strings, which `strtold` must reject at exactly the same byte we do.
"""

import random
import sys

# Values whose binary and decimal forms are known to disagree somewhere, plus
# every edge of the format: the two zeros, the two infinities, the largest
# finite value, the smallest normal, the smallest subnormal, and the numbers
# just inside and just outside each.
FIXED_READ = [
    "0", "-0", "+0", "0.0", "-0.0", ".0", "0.",
    "1", "-1", "+1", "1.", ".1", "-.1", "1.0", "01", "0001.5000",
    "0.5", "1.5", "2.5", "3.5", "-0.5", "-1.5", "-2.5",
    # 2^63, 2^64 and their neighbours: the explicit integer bit's own boundary.
    "9223372036854775807", "9223372036854775808", "9223372036854775809",
    "18446744073709551615", "18446744073709551616", "18446744073709551617",
    # The first integer an f64 cannot represent, and the first this cannot.
    "9007199254740993", "18446744073709551617",
    # The measured f64 divergence quoted in the module documentation.
    "145.0612310077283783",
    # Powers of ten either side of exact representability.
    "1e0", "1e1", "1e20", "1e22", "1e23", "1e30", "1e100", "1e300",
    "1e-1", "1e-20", "1e-30", "1e-100", "1e-300",
    # The exponent range's own edges. LDBL_MAX, LDBL_MIN, LDBL_TRUE_MIN.
    "1.18973149535723176502e+4932", "1.18973149535723176503e+4932",
    "3.36210314311209350626e-4932", "3.64519953188247460253e-4951",
    "1e4932", "1e4933", "-1e4932", "-1e4933",
    "1e-4932", "1e-4951", "1e-4952", "1e-5000",
    # Absurd exponents, which must not be built as a power of ten.
    "1e999999999", "1e-999999999", "1e99999999999999999999",
    "1e-99999999999999999999", "0e999999999", "0e-999999999",
    # Hexadecimal, including the forms with no `.` and no exponent digits.
    "0x1p+0", "0X1P+0", "0x1", "0x1.8p+1", "0x.8p1", "0x8p-3",
    "0xf.fp+4", "0x1p-16444", "0x3p-16446", "0x1p+16384", "0x1p-16500",
    "0x1.0000000000000001p+0", "0x1.fffffffffffffffep+0",
    # Words. Case-insensitive, and `nan` may carry a parenthesised payload.
    "inf", "INF", "Inf", "infinity", "INFINITY", "-inf", "+infinity",
    "nan", "NAN", "-nan", "nan(1234)", "nan(0x99)", "nan()", "nan(",
    # Leading space is skipped; a lone sign is not a number.
    " 1", "\t1", "  -2.5", "+", "-", ".", "+.", "e5", ".e5", "0x", "0xp1",
    # Trailing text: `strtold` stops, and how far it got is observable.
    "1abc", "1.5e", "1.5e+", "1.5e+x", "1e", "12e5x", "0x1p", "0x1p+",
    "infin", "nanx", "1 2",
    # Not a number at all.
    "", "abc", "--1", "1e5e5",
]

FORMATS = [
    "%Lf", "%Le", "%Lg", "%La", "%LF", "%LE", "%LG", "%LA",
    "%f", "%e", "%g", "%a",
    "%.0Lf", "%.1Lf", "%.6Lf", "%.17Lf", "%.20Lf", "%.40Lf",
    "%.0Le", "%.1Le", "%.17Le", "%.30Le",
    "%.0Lg", "%.1Lg", "%.2Lg", "%.17Lg", "%.30Lg",
    "%.0La", "%.1La", "%.5La", "%.20La",
    "%.Lf", "%.Le", "%.Lg",
    "%20Lf", "%-20Lf", "%020Lf", "%+20Lf", "% 20Lf", "%#20Lf",
    "%020.5Lf", "%-020.5Lf", "%+020.5Lf", "%020La", "%-20La", "%+.3Le",
    "%#.0Lf", "%#.0Le", "%#.0Lg", "%#Lg", "%#La",
    "%+Lf", "% Lf", "%+Le", "% Le", "%+Lg", "% Lg", "%+La", "% La",
    "%1Lf", "%2Lg", "%050.40Lf",
]


def random_numeral(rnd):
    """A numeral drawn from the shapes that actually break implementations."""
    kind = rnd.randrange(10)
    sign = rnd.choice(["", "", "-", "+"])
    if kind == 0:  # short integer
        return sign + str(rnd.randrange(0, 1000))
    if kind == 1:  # integer around the 64-bit boundary
        base = rnd.choice([2**53, 2**63, 2**64, 10**18, 10**19])
        return sign + str(base + rnd.randrange(-3, 4))
    if kind == 2:  # long integer, past what any fixed word holds
        return sign + "".join(rnd.choice("0123456789") for _ in range(rnd.randrange(20, 60)))
    if kind == 3:  # fixed point, few places
        return "%s%d.%0*d" % (sign, rnd.randrange(0, 10000), rnd.randrange(1, 6),
                              rnd.randrange(0, 100000))
    if kind == 4:  # fixed point, many places -- where f64 lost
        whole = rnd.randrange(0, 1000)
        frac = "".join(rnd.choice("0123456789") for _ in range(rnd.randrange(10, 40)))
        return "%s%d.%s" % (sign, whole, frac)
    if kind == 5:  # scientific, ordinary exponent
        return "%s%d.%se%s%d" % (sign, rnd.randrange(0, 10),
                                 "".join(rnd.choice("0123456789") for _ in range(rnd.randrange(0, 20))),
                                 rnd.choice(["+", "-", ""]), rnd.randrange(0, 320))
    if kind == 6:  # scientific, exponent near the format's own limits
        return "%s%d.%se%s%d" % (sign, rnd.randrange(1, 10),
                                 "".join(rnd.choice("0123456789") for _ in range(rnd.randrange(0, 20))),
                                 rnd.choice(["+", "-"]), rnd.randrange(4900, 4960))
    if kind == 7:  # hexadecimal: the bits, stated directly
        digits = "".join(rnd.choice("0123456789abcdefABCDEF") for _ in range(rnd.randrange(1, 17)))
        frac = "".join(rnd.choice("0123456789abcdef") for _ in range(rnd.randrange(0, 17)))
        dot = "." + frac if frac else rnd.choice(["", "."])
        return "%s0x%s%sp%s%d" % (sign, digits, dot, rnd.choice(["+", "-", ""]),
                                  rnd.randrange(0, 200))
    if kind == 8:  # a half-way case: k + 1/2 at some scale, where ties are broken
        return "%s%d.5" % (sign, rnd.randrange(0, 10000))
    # subnormal territory
    return "%s%d.%se-49%02d" % (sign, rnd.randrange(1, 10),
                                "".join(rnd.choice("0123456789") for _ in range(rnd.randrange(0, 10))),
                                rnd.randrange(20, 60))


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "read"
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 4000
    rnd = random.Random(20260817)
    out = []
    if mode == "read":
        out.extend(FIXED_READ)
        for _ in range(count):
            out.append(random_numeral(rnd))
        # A numeral with trailing text, to pin where the scan stopped.
        for _ in range(count // 4):
            out.append(random_numeral(rnd) + rnd.choice(["x", " ", "e", ".", "p", ")", "\t9"]))
    elif mode == "write":
        # Every format against every fixed value, then the fuzz. The cross
        # product is what catches a flag that only misbehaves on one shape --
        # `%#.0Lg` of an integer, say, or `%020La` of a subnormal.
        finite = [v for v in FIXED_READ if _is_finite_numeral(v)]
        for fmt in FORMATS:
            for v in finite:
                out.append(fmt + "\t" + v)
        for _ in range(count):
            out.append(rnd.choice(FORMATS) + "\t" + random_numeral(rnd))
    else:
        sys.exit("extfloat-cases.py: expected 'read' or 'write'")
    sys.stdout.write("".join(line + "\n" for line in out if "\n" not in line))


def _is_finite_numeral(v):
    """Whether the C side will accept this as a whole numeral.

    The write mode's fixed list is drawn from the read list, which deliberately
    contains rejects and overflows. Those are `read`'s business; feeding them
    here would only compare two `!bad-literal` lines.
    """
    if not v or v.strip() != v:
        return False
    try:
        f = float(v.replace("nan(", "nan#"))
    except ValueError:
        return False
    if f != f or f in (float("inf"), float("-inf")):
        return False
    # Anything Python reads as zero may still be an underflow the C side
    # rejects, but zero itself is fine and the huge-exponent forms are not.
    return "9999" not in v


main()
