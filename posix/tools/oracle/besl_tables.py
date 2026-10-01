"""The tables and the oracle for posix/src/besl.rs, the long double Bessel
functions, computed with mpmath at 100 digits -- the mathematics, not any C
library's code.

  python besl_tables.py table    prints the Rust tables besl.rs carries
  python besl_tables.py check    says whether besl.rs still carries them
                                 (layout aside, which cargo fmt owns)
  python besl_tables.py oracle   writes posix/src/besl_oracle.txt
  python besl_tables.py oracle-large
                                 writes posix/src/besl_large_oracle.txt

The tables (DD: a double-long-double, hi + lo, hi the long double nearest the
value and lo the one nearest what is left; TD the same with a third part):

  constants  2/pi, Euler's gamma, ln 2 and pi/4 as DD; pi/4 again as eight
             pieces of 24 bits each (192 bits in all), so that a multiple of
             each by an integer below 2^40 is exact; 1/k as DD for k up to
             RECIPROCALS, which the Neumann series, Hankel's terms and the
             arctangent's divide by; and 1/k! as DD for k up to FACTORIALS,
             the sine's and cosine's Taylor coefficients
  Debye      the coefficients of Debye's polynomials u_0 .. u_28 (DLMF
             10.41.9-10), each u_k(t) = t^k (a_k0 + a_k1 t^2 + ... + a_kk
             t^2k), exact rationals rounded to DD -- enough terms for 2^-135
             from 32 n^(1/3) past the turning point, at every order
  zeros      for J0, J1, Y0 and Y1, every zero below XH = 48 (where besl.rs
             leaves Miller's recurrence for Hankel's expansion): the zero as
             TD, and the Taylor coefficients of the function about it,
             f(z + h) = c1 h + ... + c5 h^5, c1 to c3 as DD, c4 and c5 as
             long doubles -- over the window |h| <= W = 2^-32 each term is
             then good to 2^-130 of the first, and the next one, dropped, is
             below 2^-155 of it

The oracle: each function correctly rounded to long double (to nearest),
one call a line, `<function> <n or -> <x> = <y> <r>`, long doubles as
`SSSS:MMMMMMMMMMMMMMMM`, and <r> the exact value against y -- `<` below it,
`=` equal, `>` above -- from which the correctly rounded value in every
direction follows. The points: random ones at every magnitude from 10^-30
to 10^4900; uniform ones below 100; every zero below 150 of J0, J1, Y0 and
Y1 -- the long double nearest it and its neighbours 1 to 10^6 ulps away,
and 10^-12 to 0.3 away, both sides; either side of the switch at 48; and
for the n-th order functions orders -40 to 40 and a few up to 1000 at
random points and either side of x = n.
"""
import random
import sys
from pathlib import Path

import mpmath

mpmath.mp.dps = 100
XH = 48
W = mpmath.mpf(2) ** -32
RECIPROCALS = 128
FACTORIALS = 37
DEBYE_TERMS = 29
OUT = Path(__file__).resolve().parent.parent.parent / "src" / "besl_oracle.txt"


def ld(v):
    """The long double nearest v (an mpf), ties to even: (sign_exp, significand)."""
    v = mpmath.mpf(v)
    if v == 0:
        return (0x8000 if mpmath.sign(v) < 0 else 0, 0)
    sign = 0x8000 if v < 0 else 0
    a = abs(v)
    m, e = mpmath.frexp(a)  # a = m 2^e, 0.5 <= m < 1
    e = int(e) - 1  # a = (2m) 2^e, 1 <= 2m < 2
    if e + 16383 <= 0:
        q = int(mpmath.nint(a * mpmath.mpf(2) ** (16382 + 63)))
        return (sign | (1 if q >> 63 else 0), q)
    q = int(mpmath.nint(a * mpmath.mpf(2) ** (63 - e)))
    if q == 1 << 64:
        q >>= 1
        e += 1
    if e + 16383 >= 0x7FFF:
        return (sign | 0x7FFF, 1 << 63)
    return (sign | (e + 16383), q)


def value(bits):
    se, m = bits
    if se & 0x7FFF == 0:
        v = mpmath.mpf(m) * mpmath.mpf(2) ** (-16382 - 63)
    else:
        v = mpmath.mpf(m) * mpmath.mpf(2) ** ((se & 0x7FFF) - 16383 - 63)
    return -v if se & 0x8000 else v


def parts(v, k):
    """v as k long doubles, each the nearest to what the ones before leave."""
    out = []
    for _ in range(k):
        b = ld(v)
        out.append(b)
        v = v - value(b)
    return out


def rust_ld(b):
    return f"L::from_bits(0x{b[0]:04X}, 0x{b[1]:016X})"


def rust_dd(v):
    hi, lo = parts(v, 2)
    return f"DD {{ hi: {rust_ld(hi)}, lo: {rust_ld(lo)} }}"


FUNCS = {
    "J0": (lambda x: mpmath.besselj(0, x), lambda k: mpmath.besseljzero(0, k)),
    "J1": (lambda x: mpmath.besselj(1, x), lambda k: mpmath.besseljzero(1, k)),
    "Y0": (lambda x: mpmath.bessely(0, x), lambda k: mpmath.besselyzero(0, k)),
    "Y1": (lambda x: mpmath.bessely(1, x), lambda k: mpmath.besselyzero(1, k)),
}


def zeros_below(name, limit):
    _, zf = FUNCS[name]
    out, k = [], 1
    while True:
        z = zf(k)
        if z >= limit:
            return out
        out.append(z)
        k += 1


def derivative_at_zero(name, z):
    """f'(z) at a zero z of f: J0' = -J1, Y0' = -Y1, and for order 1,
    f' = f_0 - f/x, which at a zero is f_0."""
    return {
        "J0": lambda: -mpmath.besselj(1, z),
        "J1": lambda: mpmath.besselj(0, z),
        "Y0": lambda: -mpmath.bessely(1, z),
        "Y1": lambda: mpmath.bessely(0, z),
    }[name]()


def taylor(name, z, count):
    """The Taylor coefficients c1 .. c(count) of f about its zero z, from
    Bessel's equation x^2 f'' + x f' + (x^2 - nu^2) f = 0 at x = z + h: the
    coefficient of h^m gives

      a[m+2] = -(z (m+1)(2m+1) a[m+1] + (m^2 + z^2 - nu^2) a[m]
                 + 2 z a[m-1] + a[m-2]) / (z^2 (m+1)(m+2)),

    with a[0] = f(z) = 0 and a[1] = f'(z)."""
    nu = 0 if name.endswith("0") else 1
    a = [mpmath.mpf(0), derivative_at_zero(name, z)]

    def at(i):
        return a[i] if i >= 0 else mpmath.mpf(0)

    for m in range(0, count + 2):
        nxt = -(z * (m + 1) * (2 * m + 1) * at(m + 1) + (m * m + z * z - nu * nu) * at(m)
                + 2 * z * at(m - 1) + at(m - 2)) / (z * z * (m + 1) * (m + 2))
        a.append(nxt)
    # The first term dropped is far below the first kept, across the window.
    assert abs(a[count + 1]) * W ** count < mpmath.mpf(2) ** -155 * abs(a[1]), (name, z)
    # A check against mpmath's own function, at the window's edge.
    f, _ = FUNCS[name]
    series = sum(a[i] * W ** i for i in range(1, count + 1))
    exact = f(z + W)
    assert abs(series - exact) <= mpmath.mpf(2) ** -150 * abs(exact), (name, z, series, exact)
    return a[1 : count + 1]


def debye_polys(kmax):
    """Debye's polynomials u_0 .. u_kmax as exact coefficient lists (index =
    power of t): u_(k+1)(t) = t^2 (1 - t^2) u_k'(t) / 2 + (1/8) int_0^t
    (1 - 5 s^2) u_k(s) ds."""
    from fractions import Fraction

    u = [[Fraction(1)]]
    for _ in range(kmax):
        prev = u[-1]
        deg = len(prev) - 1
        new = [Fraction(0)] * (deg + 4)
        for i in range(1, deg + 1):
            c = prev[i] * i / 2
            new[i + 1] += c
            new[i + 3] -= c
        for i in range(0, deg + 1):
            new[i + 1] += prev[i] / 8 / (i + 1)
            new[i + 3] -= 5 * prev[i] / 8 / (i + 3)
        u.append(new)
    return u


def table():
    lines = ["// Generated by posix/tools/oracle/besl_tables.py (mpmath, 100 digits): do not edit.", ""]
    consts = {
        "TWO_OVER_PI": 2 / mpmath.pi,
        "EULER_GAMMA": mpmath.euler,
        "LN_2": mpmath.log(2),
        "PI_OVER_4": mpmath.pi / 4,
    }
    for name, v in consts.items():
        lines.append(f"const {name}: DD = {rust_dd(v)};")
    # pi/4 in eight 24-bit pieces.
    rest = mpmath.pi / 4
    pieces = []
    for _ in range(8):
        e = int(mpmath.floor(mpmath.log(abs(rest), 2)))
        q = mpmath.floor(rest * mpmath.mpf(2) ** (23 - e))  # 24 bits, truncated
        piece = q * mpmath.mpf(2) ** (e - 23)
        pieces.append(ld(piece))
        assert value(ld(piece)) == piece
        rest -= piece
    assert abs(rest) < mpmath.mpf(2) ** -190
    lines.append("/// pi/4 as eight pieces of at most 24 significant bits, largest first:")
    lines.append("/// their sum is pi/4 to 2^-190, and each times an integer below 2^40 is exact.")
    lines.append("const PI_OVER_4_PIECES: [L; 8] = [")
    for p in pieces:
        lines.append(f"    {rust_ld(p)},")
    lines.append("];")
    lines.append(f"/// 1/k for k = 1 to {RECIPROCALS}, as DD (index k - 1).")
    lines.append(f"const RECIPROCALS: [DD; {RECIPROCALS}] = [")
    for k in range(1, RECIPROCALS + 1):
        lines.append(f"    {rust_dd(mpmath.mpf(1) / k)},")
    lines.append("];")
    lines.append(f"/// 1/k! for k = 0 to {FACTORIALS}, as DD.")
    lines.append(f"const INV_FACTORIALS: [DD; {FACTORIALS + 1}] = [")
    for k in range(0, FACTORIALS + 1):
        lines.append(f"    {rust_dd(1 / mpmath.factorial(k))},")
    lines.append("];")
    u = debye_polys(DEBYE_TERMS - 1)
    assert u[1][1:4] == [3 / __import__("fractions").Fraction(24), 0, -5 / __import__("fractions").Fraction(24)]
    lines.append(f"/// Debye's polynomials u_0 .. u_{DEBYE_TERMS - 1}: u_k(t) = t^k times the")
    lines.append("/// polynomial in t^2 whose coefficients, lowest first, are row k.")
    lines.append(f"const DEBYE: [&[DD]; {DEBYE_TERMS}] = [")
    for k in range(DEBYE_TERMS):
        coeffs = u[k][k::2]
        assert all(c == 0 for i, c in enumerate(u[k]) if (i - k) % 2 or i < k)
        row = ", ".join(rust_dd(mpmath.mpf(c.numerator) / c.denominator) for c in coeffs)
        lines.append(f"    &[{row}],")
    lines.append("];")
    lines.append("")
    for name in FUNCS:
        lines.append(f"/// The zeros of {name} below {XH}, and its Taylor coefficients about each.")
        lines.append(f"const {name}_ZEROS: &[Zero] = &[")
        for z in zeros_below(name, XH):
            cs = taylor(name, z, 5)
            zp = parts(z, 3)
            lines.append("    Zero {")
            lines.append(f"        z: [{', '.join(rust_ld(b) for b in zp)}],")
            lines.append(f"        c: [{', '.join(rust_dd(c) for c in cs[:3])}],")
            lines.append(f"        d: [{', '.join(rust_ld(ld(c)) for c in cs[3:])}],")
            lines.append("    },")
        lines.append("];")
        lines.append("")
    print("\n".join(lines))


def hexv(b):
    return f"{b[0]:04x}:{b[1]:016x}"


def relation(exact, b):
    y = value(b)
    return "=" if exact == y else ("<" if exact < y else ">")


def ulp_neighbours(x, rng):
    """The long double nearest x, and ones 1 to 10^6 ulps either side."""
    b = ld(x)
    out = [b]
    for d in sorted({1, 2, 3, 10, 100, 1000, 10 ** 6} | {rng.randint(1, 10 ** 6) for _ in range(3)}):
        for s in (-1, 1):
            m = b[1] + s * d
            if 1 << 63 <= m < 1 << 64:
                out.append((b[0], m))
    return out


def hankel(kind, nu, x):
    """J_nu(x) or Y_nu(x) for x > 10^6 and x > 100 nu^2, by Hankel's expansion
    at log10(x) + 120 digits: mpmath's own besselj loses every digit by
    x = 10^1000 (it reduces x by pi at the working precision), and takes
    minutes there."""
    digits = int(mpmath.log10(x)) + 120
    with mpmath.workdps(digits):
        mu = 4 * mpmath.mpf(nu) ** 2
        p, q, t = mpmath.mpf(1), mpmath.mpf(0), mpmath.mpf(1)
        eps = mpmath.mpf(10) ** -115
        k = 0
        while abs(t) >= eps:
            k += 1
            t = t * (mu - (2 * k - 1) ** 2) / (8 * k * x)
            if k % 4 == 1:
                q += t
            elif k % 4 == 2:
                p -= t
            elif k % 4 == 3:
                q -= t
            else:
                p += t
            assert k < 500, (kind, nu, x)
        chi = x - (2 * nu + 1) * mpmath.pi / 4
        amp = mpmath.sqrt(2 / (mpmath.pi * x))
        if kind == "J":
            r = amp * (p * mpmath.cos(chi) - q * mpmath.sin(chi))
        else:
            r = amp * (p * mpmath.sin(chi) + q * mpmath.cos(chi))
    return +r


def exact_value(kind, n, x):
    """J_n(x) or Y_n(x), trusted: past x = 10^6 (and 100 n^2) Hankel's
    expansion; below, mpmath at rising precision until two successive
    answers agree to 45 digits -- 150 bits, where the rounding to 64 needs
    about 70 -- as near a zero, where the answer is small and the working
    precision's relative digits are lost, they take more. Order -n is (-1)^n
    order n."""
    sign = -1 if n < 0 and n % 2 else 1
    m = abs(n)
    if x > 10 ** 6 and x > 100 * m * m:
        return sign * hankel(kind, m, x)
    f = mpmath.besselj if kind == "J" else mpmath.bessely
    prev = None
    for dps in (110, 150, 200, 300, 450, 700):
        with mpmath.workdps(dps):
            y = f(m, x)
        if prev is not None and abs(y - prev) <= abs(y) * mpmath.mpf(10) ** -45:
            return sign * y
        prev = y
    raise AssertionError((kind, n, x))


def check_hankel():
    """Hankel's values against mpmath's where both are good."""
    for kind in "JY":
        for nu in (0, 1, 7):
            for x in (mpmath.mpf(10) ** 7 + mpmath.mpf("0.3"), mpmath.mpf(10) ** 30 / 3):
                f = mpmath.besselj if kind == "J" else mpmath.bessely
                with mpmath.workdps(130):
                    ref = f(nu, x)
                got = hankel(kind, nu, x)
                assert abs(got - ref) <= abs(ref) * mpmath.mpf(10) ** -95, (kind, nu, x, got, ref)


def oracle():
    check_hankel()
    rng = random.Random(1972)
    points = {k: [] for k in ("j0l", "j1l", "y0l", "y1l")}
    # random magnitudes, 10^-30 to 10^4900
    for _ in range(500):
        e = rng.uniform(-30, 7) if rng.random() < 0.8 else rng.uniform(7, 4900)
        x = mpmath.mpf(10) ** e * (1 + mpmath.mpf(rng.random()))
        b = ld(x)
        for k in points:
            points[k].append(b)
        points["j0l"].append((b[0] | 0x8000, b[1]))
        points["j1l"].append((b[0] | 0x8000, b[1]))
    # uniform below 100
    for _ in range(700):
        b = ld(mpmath.mpf(rng.uniform(0, 100)))
        for k in points:
            points[k].append(b)
    # near the zeros below 150
    for name, key in (("J0", "j0l"), ("J1", "j1l"), ("Y0", "y0l"), ("Y1", "y1l")):
        for z in zeros_below(name, 150):
            points[key] += ulp_neighbours(z, rng)
            for t in (mpmath.mpf(10) ** -12, mpmath.mpf(10) ** -6, mpmath.mpf(10) ** -3,
                      mpmath.mpf(1) / 10, mpmath.mpf(1) / 4, mpmath.mpf(0.3)):
                points[key] += [ld(z - t), ld(z + t)]
    # either side of the switch
    for d in (-3, -2, -1, 0, 1, 2, 3):
        b = ld(mpmath.mpf(XH))
        for k in points:
            points[k].append((b[0], b[1] + d))
    funcs = {"j0l": (0, "J"), "j1l": (1, "J"), "y0l": (0, "Y"), "y1l": (1, "Y")}
    lines = []
    for key, pts in points.items():
        n, kind = funcs[key]
        seen = set()
        for b in pts:
            if b in seen:
                continue
            seen.add(b)
            x = value(b)
            if x == 0 or (kind == "Y" and x < 0):
                continue
            y = exact_value(kind, n, x)
            yb = ld(y)
            lines.append(f"{key} - {hexv(b)} = {hexv(yb)} {relation(y, yb)}")
    # the n-th order functions
    orders = list(range(-6, 7)) + [10, 17, 25, 40, -25, -40, 100, 1000, -999]
    for n in orders:
        m = max(abs(n), 2)
        for _ in range(25 if abs(n) <= 40 else 12):
            x = mpmath.mpf(rng.uniform(0.01, 3 * m))
            b = ld(x)
            for key, kind in (("jnl", "J"), ("ynl", "Y")):
                y = exact_value(kind, n, value(b))
                yb = ld(y)
                lines.append(f"{key} {n} {hexv(b)} = {hexv(yb)} {relation(y, yb)}")
        for x in (mpmath.mpf(abs(n)) - 1, mpmath.mpf(abs(n)), mpmath.mpf(abs(n)) + 1,
                  mpmath.mpf(abs(n)) / 3, mpmath.mpf(abs(n)) * 5, mpmath.mpf(10) ** 30):
            if x <= 0:
                continue
            b = ld(x)
            for key, kind in (("jnl", "J"), ("ynl", "Y")):
                y = exact_value(kind, n, value(b))
                yb = ld(y)
                lines.append(f"{key} {n} {hexv(b)} = {hexv(yb)} {relation(y, yb)}")
    header = ("# Correctly rounded long double Bessel values, mpmath at 100 digits\n"
              "# (posix/tools/oracle/besl_tables.py): <function> <order or -> <x> = <y> <r>,\n"
              "# <r> the exact value against y: < below, = equal, > above.\n")
    OUT.write_text(header + "\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(lines)} lines -> {OUT}")


def debye_reference(kind, n, x):
    """J_n(x) or Y_n(x) by Debye's expansions at 60 digits with 40 terms, for
    x at least 64 n^(1/3) from n, where the terms fall below 10^-60: the
    oracle for orders mpmath's besselj cannot reach."""
    u = debye_polys(40)
    with mpmath.workdps(80 + int(mpmath.log10(x))):
        n, x = mpmath.mpf(n), mpmath.mpf(x)

        def ev(k, t):
            r = mpmath.mpc(0)
            for c in reversed(u[k]):
                r = r * t + mpmath.mpf(c.numerator) / c.denominator
            return r

        if x > n:
            s = mpmath.sqrt(x * x - n * n)
            beta = mpmath.atan(s / n)
            t = mpmath.mpc(0, 1) * n / s
            even = sum(ev(2 * k, t) / n ** (2 * k) for k in range(20))
            odd = sum(ev(2 * k + 1, t) / n ** (2 * k + 1) for k in range(20))
            xi = s - n * beta - mpmath.pi / 4
            amp = mpmath.sqrt(2 / (mpmath.pi * s))
            if kind == "J":
                r = amp * (mpmath.cos(xi) * even - mpmath.mpc(0, 1) * mpmath.sin(xi) * odd)
            else:
                r = amp * (mpmath.sin(xi) * even + mpmath.mpc(0, 1) * mpmath.cos(xi) * odd)
            return +r.real
        th = mpmath.sqrt(n * n - x * x) / n
        alpha = mpmath.atanh(th)
        t = 1 / th
        g = n * (alpha - th)
        if kind == "J":
            ssum = sum(ev(k, t) / n ** k for k in range(40))
            return +(mpmath.exp(-g) / mpmath.sqrt(2 * mpmath.pi * n * th) * ssum).real
        ssum = sum((-1) ** k * ev(k, t) / n ** k for k in range(40))
        return +(-mpmath.exp(g) / mpmath.sqrt(mpmath.pi * n * th / 2) * ssum).real


def recurrence_reference(kind, n, x, dps):
    """J_n(x) or Y_n(x) by the recurrences at `dps` digits: upward from
    mpmath's order 0 and 1 for Y (stable for it everywhere) and for J where
    x >= n (stable up to the turning point); for J below it, Miller's
    downward from n + 40 n^(1/3) + 400 -- where J has fallen by e^-400 and
    more -- normalised by J0 + 2 (J2 + J4 + ...) = 1."""
    with mpmath.workdps(dps):
        x = mpmath.mpf(x)
        if kind == "Y" or x >= n:
            f = mpmath.besselj if kind == "J" else mpmath.bessely
            a, b = f(0, x), f(1, x)
            for k in range(1, n):
                a, b = b, 2 * k / x * b - a
            return +b
        big = n + int(40 * n ** (1 / 3)) + 400
        nxt, cur = mpmath.mpf(0), mpmath.mpf(1)
        norm, ans = mpmath.mpf(0), None
        for k in range(big, 0, -1):
            if k == n:
                ans = cur
            if k % 2 == 0:
                norm += 2 * cur
            nxt, cur = cur, 2 * k / x * cur - nxt
        norm += cur
        return ans / norm


def large_value(kind, n, x):
    """J_n(x) or Y_n(x) at a large order: the recurrences at 60 and 80
    digits, which must agree to 45, up to order 2^16 -- mpmath's own besselj
    and bessely take tens of seconds a value there -- and Debye's expansions
    at 80 digits beyond."""
    if n > 2 ** 16:
        return debye_reference(kind, n, x)
    a = recurrence_reference(kind, n, x, 60)
    b = recurrence_reference(kind, n, x, 80)
    assert abs(a - b) <= abs(b) * mpmath.mpf(10) ** -45, (kind, n, x, a, b)
    return b


def check_large_reference():
    """The recurrences against mpmath's own functions where those are quick,
    and against Debye's expansions where both hold."""
    for n, x in ((2048, "2048.37"), (2048, "1900.25"), (2048, "2400.5")):
        for kind, f in (("J", mpmath.besselj), ("Y", mpmath.bessely)):
            with mpmath.workdps(60):
                ref = f(n, mpmath.mpf(x), maxterms=10 ** 6, maxprec=100000)
            got = large_value(kind, n, mpmath.mpf(x))
            assert abs(got - ref) <= abs(ref) * mpmath.mpf(10) ** -40, (kind, n, x, got, ref)
    for kind in "JY":
        n = 2 ** 14
        c = mpmath.mpf(n) ** (mpmath.mpf(1) / 3)
        for d in (-100, 100):
            x = n + d * c
            a = recurrence_reference(kind, n, x, 80)
            b = debye_reference(kind, n, x)
            assert abs(a - b) <= abs(b) * mpmath.mpf(10) ** -40, (kind, n, d, a, b)


def oracle_large():
    """posix/src/besl_large_oracle.txt: jnl and ynl at orders from 600 to
    2^31, across the turning point -- before it, within 32 n^(1/3) of it
    and past it -- in the same format as the main oracle."""
    check_large_reference()
    lines = []
    out = OUT.parent / "besl_large_oracle.txt"
    for n in (600, 2048, 3000, 2 ** 13, 2 ** 14):
        c = mpmath.mpf(n) ** (mpmath.mpf(1) / 3)
        for d in (-100, -40, -33, -31, -16, -4, -1, 0, 1, 4, 16, 31, 33, 40, 100):
            x = mpmath.mpf(n) + d * c + mpmath.mpf("0.37")
            if x <= 0:
                continue
            b = ld(x)
            for key, kind in (("jnl", "J"), ("ynl", "Y")):
                y = large_value(kind, n, value(b))
                if y == 0:
                    continue
                yb = ld(y)
                lines.append(f"{key} {n} {hexv(b)} = {hexv(yb)} {relation(y, yb)}")
        for mult in (2, 10, 1000):
            b = ld(mpmath.mpf(n) * mult + mpmath.mpf("0.5"))
            for key, kind in (("jnl", "J"), ("ynl", "Y")):
                y = large_value(kind, n, value(b))
                yb = ld(y)
                lines.append(f"{key} {n} {hexv(b)} = {hexv(yb)} {relation(y, yb)}")
        print(n, len(lines), flush=True)
    for n in (2 ** 20, 2 ** 31 - 1):
        c = mpmath.mpf(n) ** (mpmath.mpf(1) / 3)
        for d in (-200, -64, 64, 200, 10 ** 4):
            x = mpmath.mpf(n) + d * c + mpmath.mpf("0.25")
            b = ld(x)
            for key, kind in (("jnl", "J"), ("ynl", "Y")):
                y = large_value(kind, n, value(b))
                yb = ld(y)
                if yb[0] & 0x7FFF in (0, 0x7FFF):
                    continue
                lines.append(f"{key} {n} {hexv(b)} = {hexv(yb)} {relation(y, yb)}")
        print(n, len(lines), flush=True)
    header = ("# Correctly rounded long double Bessel values at large orders\n"
              "# (posix/tools/oracle/besl_tables.py oracle-large): the recurrences at 80 digits to\n"
              "# order 2^16, Debye's expansions at 80 digits beyond; <function> <order> <x> = <y> <r>.\n")
    out.write_text(header + "\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(lines)} lines -> {out}")


def check():
    """Exit 1 unless the tables pasted in posix/src/besl.rs are what `table`
    prints now, whitespace and trailing commas aside."""
    import contextlib
    import io
    import re

    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        table()

    def squash(text):
        return re.sub(r",([)\]}])", r"\1", re.sub(r"\s+", "", text))

    src = (OUT.parent / "besl.rs").read_text(encoding="utf-8")
    if squash(buf.getvalue()) in squash(src):
        print("posix/src/besl.rs: the pasted tables are still the generator's")
        return
    print("posix/src/besl.rs: the pasted tables differ from the generator's")
    sys.exit(1)


if __name__ == "__main__":
    {"table": table, "check": check, "oracle": oracle, "oracle-large": oracle_large}[
        sys.argv[1] if len(sys.argv) > 1 else "table"]()
