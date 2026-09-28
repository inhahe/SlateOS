"""The zeros of lgamma below -2, and the constants mathl.rs's lgammal
evaluates it near them with, computed with mpmath at 80 digits.

For each zero x0 of log|gamma| in (-n - 1, -n), n = 2..NMAX, the one nearer
-n first:

  pole    the integer nearest x0
  e0      x0 - pole, as hi + lo
  sin_e0  sin(pi e0), as hi + lo
  cot_e0  cot(pi e0), as hi + lo
  c1      psi(1 - x0), as hi + lo: the first Taylor coefficient of
          B(d) = lgamma(1 - x0) - lgamma(1 - x0 - d)
  c       the rest, c_k = (-1)^(k+1) psi^(k-1)(1 - x0) / k!, k = 2..K, with K
          the fewest for which |c_(K+1)| DMAX^K is below 2^-72

  python lgammal_zeros.py table [NMAX]   the Rust table (NMAX 30)
  python lgammal_zeros.py oracle         posix/src/lgammal_zero_oracle.txt

The oracle: log|gamma(x)| correctly rounded to long double, near each zero
and pole below -2 -- at 1 to 10^4 ulps from each zero and 10^-15 to 1/4 from
it, 1 ulp to 3/10 from each pole, both sides -- and at 1,500 random points in
(-35, -2), one line each, `<x sign_exp>:<x significand> <y ...>` in hex.
"""
import random
import sys

import mpmath

mpmath.mp.dps = 80
DMAX = mpmath.mpf("0.25")


def ld_bits(v):
    """(sign_exp, significand) of the long double nearest v, ties to even."""
    if v == 0:
        return (0x8000 if mpmath.sign(v) < 0 else 0, 0)
    s = 0x8000 if v < 0 else 0
    a = abs(v)
    e = int(mpmath.floor(mpmath.log(a, 2)))
    sig = a * mpmath.mpf(2) ** (63 - e)
    while sig >= mpmath.mpf(2) ** 64:
        e += 1
        sig = a * mpmath.mpf(2) ** (63 - e)
    while sig < mpmath.mpf(2) ** 63:
        e -= 1
        sig = a * mpmath.mpf(2) ** (63 - e)
    fl = int(mpmath.floor(sig))
    frac = sig - fl
    if frac > 0.5 or (frac == 0.5 and fl % 2 == 1):
        fl += 1
    if fl == 2 ** 64:
        fl = 2 ** 63
        e += 1
    be = e + 16383
    assert 1 <= be < 0x7FFF, (v, be)
    return (s | be, fl)


def ld_value(bits):
    """The value of a long double given as (sign_exp, significand)."""
    se, sig = bits
    if sig == 0:
        return mpmath.mpf(0)
    e = (se & 0x7FFF) - 16383
    v = mpmath.mpf(sig) * mpmath.mpf(2) ** (e - 63)
    return -v if se & 0x8000 else v


def rust_l(v):
    """A long double constant in Rust."""
    se, sig = ld_bits(v)
    return f"L::from_bits(0x{se:04X}, 0x{sig:016X})"


def rust_pair(v):
    """`(hi, lo)`: two long doubles summing to v to twice the precision."""
    hi = ld_value(ld_bits(v))
    lo = v - hi
    return f"({rust_l(hi)}, {rust_l(lo) if lo != 0 else 'ZERO'})"


def f(x):
    """log|gamma(x)|."""
    return mpmath.log(abs(mpmath.gamma(x)))


def bis(lo, hi):
    """The zero of f in (lo, hi), by bisection, to the working precision."""
    flo = f(lo)
    for _ in range(600):
        mid = (lo + hi) / 2
        fm = f(mid)
        if (fm > 0) == (flo > 0):
            lo, flo = mid, fm
        else:
            hi = mid
    return (lo + hi) / 2


def zeros(nmax):
    """The zeros below -2, two per unit interval (-n - 1, -n), n = 2..nmax."""
    out = []
    for n in range(2, nmax + 1):
        a, b = mpmath.mpf(-n - 1), mpmath.mpf(-n)
        xm = mpmath.findroot(mpmath.digamma, (a + b) / 2)
        assert a < xm < b and f(xm) < 0, (n, xm)
        eps = mpmath.mpf(10) ** -75
        out.append(bis(xm, b - eps))
        out.append(bis(a + eps, xm))
    return out


def table(nmax):
    """The Rust table, printed."""
    zs = zeros(nmax)
    entries = []
    maxk = 0
    for x0 in zs:
        m = int(mpmath.nint(x0))
        e0 = x0 - m
        y0 = 1 - x0
        cs = []
        k = 1
        while True:
            cs.append((-1) ** (k + 1) * mpmath.polygamma(k - 1, y0) / mpmath.factorial(k))
            nxt = mpmath.polygamma(k, y0) / mpmath.factorial(k + 1)
            if abs(nxt) * DMAX ** k < mpmath.mpf(2) ** -72:
                break
            k += 1
        maxk = max(maxk, len(cs))
        coefs = "\n".join("            " + rust_l(c) + "," for c in cs[1:])
        entries.append(
            "    LdZero {\n"
            f"        // {mpmath.nstr(x0, 25)}\n"
            f"        pole: {m},\n"
            f"        e0: {rust_pair(e0)},\n"
            f"        sin_e0: {rust_pair(mpmath.sin(mpmath.pi * e0))},\n"
            f"        cot_e0: {rust_pair(mpmath.cot(mpmath.pi * e0))},\n"
            f"        c1: {rust_pair(cs[0])},\n"
            "        c: &[\n"
            f"{coefs}\n"
            "        ],\n"
            "    },"
        )
    print(f"// {len(zs)} zeros, n = 2..={nmax}; at most {maxk} coefficients each.")
    print("#[rustfmt::skip]")
    print(f"static LGAMMAL_ZEROS: [LdZero; {len(zs)}] = [")
    print("\n".join(entries))
    print("];")


def hexl(v):
    """The long double nearest v, as the oracle writes one."""
    se, sig = ld_bits(v)
    return f"{se:04x}:{sig:016x}"


def ulp_of(v):
    """The spacing of the long doubles at the long double nearest v."""
    se, _ = ld_bits(v)
    return mpmath.mpf(2) ** ((se & 0x7FFF) - 16383 - 63)


def oracle():
    """The reference values, printed: see the module documentation."""
    random.seed(20260928)
    xs = []
    for n in range(2, 36):
        a, b = mpmath.mpf(-n - 1), mpmath.mpf(-n)
        xm = mpmath.findroot(mpmath.digamma, (a + b) / 2)
        eps = mpmath.mpf(10) ** -75
        zs = []
        if f(xm) < 0:
            zs = [bis(xm, b - eps), bis(a + eps, xm)]
        for z in zs:
            zl = ld_value(ld_bits(z))
            u = ulp_of(zl)
            for k in [1, 2, 3, 5, 10, 100, 10 ** 4]:
                for s in (-1, 1):
                    xs.append(zl + s * k * u)
            xs.append(zl)
            for dd in ["1e-15", "1e-12", "1e-9", "1e-6", "1e-4", "1e-3", "0.01", "0.03", "0.1", "0.2", "0.24"]:
                for s in (-1, 1):
                    v = z + s * mpmath.mpf(dd)
                    if a < v < b:
                        xs.append(ld_value(ld_bits(v)))
        # beside each pole, both sides
        for p in (b, a):
            for k in [1, 2, 3, 10, 1000, 10 ** 6]:
                for s in (-1, 1):
                    v = p + s * k * ulp_of(p)
                    if a < v < b:
                        xs.append(v)
            for dd in ["1e-17", "1e-15", "1e-12", "1e-8", "1e-4", "0.01", "0.1", "0.3"]:
                for s in (-1, 1):
                    v = p + s * mpmath.mpf(dd)
                    if a < v < b:
                        xs.append(ld_value(ld_bits(v)))
    for _ in range(1500):
        v = -mpmath.mpf(random.uniform(2.0, 35.0))
        xs.append(ld_value(ld_bits(v)))

    seen = set()
    out = []
    for x in xs:
        key = hexl(x)
        if key in seen or x == mpmath.floor(x):
            continue
        seen.add(key)
        y = f(x)
        out.append(f"{key} {hexl(y)}")
    print("\n".join(out))


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "table":
        table(int(sys.argv[2]) if len(sys.argv) > 2 else 30)
    elif len(sys.argv) > 1 and sys.argv[1] == "oracle":
        oracle()
    else:
        print(__doc__)
