r"""fnmatch, as posix/src/fnmatch.rs does it, written a second way: from POSIX
(XCU 2.13, XBD 9.3.5) and glibc's manual, and held to glibc 2.39's answers
(fnmatch_oracle.txt) wherever those leave a case open.

It exists for the cases where glibc's answer contradicts the text it
implements -- design-decisions section 1148 -- and fnmatch_harness.py uses it
to write fnmatch_deviations.txt: each case, glibc's answer and this one,
which posix/src/fnmatch.rs's tests hold the Rust to instead of glibc's. The
three:

- `*\/` with FNM_PATHNAME: POSIX's escaped slash is a slash in the pattern,
  and matches one; glibc's star skips it.
- a `*` before an extended group (`*@(a|)`, `*!(a)`): glibc's tries every
  split of the string but the one at its end, and reads `**(x)` as `*`.
- FNM_LEADING_DIR inside an extended group: the manual defines the flag as
  "the string starts with a directory name the pattern matches", a property
  of the whole pattern; glibc applies it inside the alternatives of `*`, `+`
  and `!` groups and not of `@` and `?` ones.

    python posix/tools/oracle/fnmatch_model.py posix/src/fnmatch_oracle.txt [N [FLAGS]]

prints how many of the oracle's cases the model answers differently from
glibc, and the first N.
"""

import sys
from pathlib import Path

PATHNAME, NOESCAPE, PERIOD, LEADING_DIR, CASEFOLD, EXTMATCH = 1, 2, 4, 8, 16, 32
FLAG_NAMES = {"0": 0, "PATHNAME": 1, "NOESCAPE": 2, "PERIOD": 4, "LEADING_DIR": 8, "CASEFOLD": 16,
              "EXTMATCH": 32}

CLASSES = {
    "alpha": lambda c: c.isascii() and c.isalpha(),
    "digit": lambda c: "0" <= c <= "9",
    "alnum": lambda c: c.isascii() and c.isalnum(),
    "upper": lambda c: "A" <= c <= "Z",
    "lower": lambda c: "a" <= c <= "z",
    "space": lambda c: c in " \t\n\v\f\r",
    "blank": lambda c: c in " \t",
    "punct": lambda c: 33 <= ord(c) <= 126 and not c.isalnum(),
    "print": lambda c: 32 <= ord(c) <= 126,
    "graph": lambda c: 33 <= ord(c) <= 126,
    "cntrl": lambda c: ord(c) < 32 or ord(c) == 127,
    "xdigit": lambda c: c in "0123456789abcdefABCDEF",
}


class Invalid(Exception):
    """A bracket expression no string can match: the whole pattern fails."""


def fold(c, flags):
    return c.lower() if flags & CASEFOLD else c


def leading_period(s, i, flags):
    return bool(flags & PERIOD) and i < len(s) and s[i] == "." and (
        i == 0 or (flags & PATHNAME and s[i - 1] == "/"))


LITERAL = "literal"  # an unterminated bracket: its `[` is an ordinary character


def skip_rest(p, i, flags):
    """After an element matched: the index past the bracket's closing `]`, or
    None if there is none -- the pattern then matches nothing. The rest is
    stepped over, not judged: an invalid class after the match does not
    count."""
    while True:
        if i >= len(p):
            return None
        c = p[i]
        i += 1
        if c == "\\" and not flags & NOESCAPE:
            if i >= len(p):
                return None
            i += 1
        elif c == "[" and i < len(p) and p[i] == ":":
            j = i + 1
            while j < len(p) and "a" <= p[j] < "z":
                j += 1
            if p[j:j + 2] == ":]":
                i = j + 2
        elif c == "[" and i < len(p) and p[i] == "=":
            if i + 3 >= len(p) + 1 or p[i + 2:i + 4] != "=]":
                return None
            i += 4
        elif c == "[" and i < len(p) and p[i] == ".":
            end = p.find(".]", i + 1)
            if end < 0:
                return None
            i = end + 2
        elif c == "]":
            return i


def bracket(p, pi, ch, orig, flags):
    """Does the bracket expression whose `[` is at p[pi-1] take `ch` (folded)?
    (True or False, index after it); LITERAL if it is unterminated before
    anything matched; raises Invalid when it reaches an element no string can
    match -- the whole pattern then fails."""
    i = pi
    negate = i < len(p) and p[i] in "!^"
    if negate:
        i += 1
    first = True
    while True:
        if i >= len(p):
            return LITERAL
        c = p[i]
        if c == "]" and not first:
            return negate, i + 1
        first = False
        hit = False
        if c == "[" and i + 1 < len(p) and p[i + 1] == ":":
            j = i + 2
            while j < len(p) and "a" <= p[j] < "z":
                j += 1
            if p[j:j + 2] == ":]":
                name = p[i + 2:j]
                if name not in CLASSES:
                    raise Invalid
                hit = CLASSES[name](orig)  # unfolded, as glibc tests a class
                i = j + 2
                if hit:
                    after = skip_rest(p, i, flags)
                    if after is None:
                        raise Invalid
                    return not negate, after
                continue
            # not a class name: `[` is an ordinary character here
            ch1 = "["
            i += 1
            coll = False
        elif c == "[" and i + 1 < len(p) and p[i + 1] in "=.":
            kind = p[i + 1]
            end = p.find(kind + "]", i + 2)
            if end < 0:
                raise Invalid
            sym = p[i + 2:end]
            if len(sym) != 1:
                raise Invalid
            i = end + 2
            ch1 = sym
            coll = kind == "."
            if kind == "=":
                if sym == orig:  # unfolded, as glibc compares one
                    after = skip_rest(p, i, flags)
                    if after is None:
                        raise Invalid
                    return not negate, after
                continue
        elif c == "\\" and not flags & NOESCAPE:
            if i + 1 >= len(p):
                raise Invalid
            ch1 = p[i + 1]
            i += 2
            coll = False
        else:
            ch1 = c
            i += 1
            coll = False
        # a range?
        if i + 1 < len(p) and p[i] == "-" and p[i + 1] != "]":
            j = i + 1
            if p[j] == "[" and j + 1 < len(p) and p[j + 1] == ".":
                end = p.find(".]", j + 2)
                if end < 0:
                    raise Invalid
                hi = p[j + 2:end]
                if len(hi) != 1:
                    raise Invalid
                j = end + 2
            elif p[j] == "\\" and not flags & NOESCAPE:
                if j + 1 >= len(p):
                    raise Invalid
                hi = p[j + 1]
                j += 2
            else:
                hi = p[j]
                j += 1
            lo = ch1
            hit = ord(fold(lo, flags)) <= ord(ch) <= ord(fold(hi, flags)) or bool(
                flags & CASEFOLD and ord(lo) <= ord(ch.upper()) <= ord(hi))
            i = j
        else:
            hit = (ch1 == orig) if coll else (fold(ch1, flags) == ch)
        if hit:
            after = skip_rest(p, i, flags)
            if after is None:
                raise Invalid
            return not negate, after


def skip_bracket(p, j):
    """p[j] is a `[`: the index past its `]`, or None if it has none."""
    k = j + 1
    if k < len(p) and p[k] in "!^":
        k += 1
    if k < len(p) and p[k] == "]":
        k += 1
    while k < len(p) and p[k] != "]":
        k += 1
    return None if k >= len(p) else k + 1


def group_end(p, i):
    """p[i] is the `(` of an extended group: the index of its `)`, or None."""
    j = i + 1
    while j < len(p):
        c = p[j]
        if c == "[":
            k = skip_bracket(p, j)
            if k is None:
                return None
            j = k
            continue
        if c in "?*+@!" and p[j + 1:j + 2] == "(":
            e = group_end(p, j + 1)
            if e is None:
                return None
            j = e + 1
            continue
        if c == ")":
            return j
        j += 1
    return None


def alternatives(p, i, e):
    """The `|`-separated patterns between p[i] `(` and p[e] `)`."""
    alts, start, j = [], i + 1, i + 1
    while j < e:
        c = p[j]
        if c == "[":
            j = skip_bracket(p, j)
            continue
        if c in "?*+@!" and p[j + 1:j + 2] == "(":
            j = group_end(p, j + 1) + 1
            continue
        if c == "|":
            alts.append(p[start:j])
            start = j + 1
        j += 1
    alts.append(p[start:e])
    return alts


def ext(p, pi, s, si, se, flags):
    """An extended group at p[pi] (`?*+@!` and `(`): does it and the rest of
    the pattern match s[si:se]? None if the group has no `)` -- its
    characters are then ordinary ones."""
    kind = p[pi]
    e = group_end(p, pi + 1)
    if e is None:
        return None
    alts = alternatives(p, pi + 1, e)
    rest = e + 1
    # An alternative matches a part of the string, whose tail LEADING_DIR
    # does not concern.
    sub = flags & ~LEADING_DIR

    def one(k):
        return any(match(a, 0, s, si, k, sub) for a in alts)

    def repeat(i):
        """Zero or more occurrences from s[i:], then the rest."""
        if match(p, rest, s, i, se, flags):
            return True
        for k in range(i + 1, se + 1):
            if any(match(a, 0, s, i, k, sub) for a in alts) and repeat(k):
                return True
        return False

    if kind == "@":
        return any(one(k) and match(p, rest, s, k, se, flags) for k in range(si, se + 1))
    if kind == "?":
        return match(p, rest, s, si, se, flags) or any(
            one(k) and match(p, rest, s, k, se, flags) for k in range(si, se + 1))
    if kind == "*":
        return repeat(si)
    if kind == "+":
        return any(one(k) and repeat(k) for k in range(si, se + 1))
    # "!"
    return any(not one(k) and match(p, rest, s, k, se, flags) for k in range(si, se + 1))


def match(p, pi, s, si, se, flags):
    while True:
        if pi >= len(p):
            return si == se or (bool(flags & LEADING_DIR) and s[si] == "/")
        c = p[pi]
        if flags & EXTMATCH and c in "?*+@!" and p[pi + 1:pi + 2] == "(":
            r = ext(p, pi, s, si, se, flags)
            if r is not None:
                return r
        if c == "?":
            if si >= se or (flags & PATHNAME and s[si] == "/") or leading_period(s, si, flags):
                return False
            pi += 1
            si += 1
            continue
        if c == "*":
            pi += 1
            while pi < len(p) and p[pi] == "*" and not (flags & EXTMATCH and p[pi + 1:pi + 2] == "("):
                pi += 1
            if leading_period(s, si, flags):
                return False
            k = si
            while True:
                if match(p, pi, s, k, se, flags):
                    return True
                if k >= se or (flags & PATHNAME and s[k] == "/"):
                    return False
                k += 1
        if c == "[":
            if si >= se:
                return False
            if leading_period(s, si, flags):
                return False
            if flags & PATHNAME and s[si] == "/":
                return False
            try:
                b = bracket(p, pi + 1, fold(s[si], flags), s[si], flags)
            except Invalid:
                return False
            if b is not LITERAL:
                took, after = b
                if not took:
                    return False
                pi = after
                si += 1
                continue
            # unterminated: an ordinary `[`
        if c == "\\" and not flags & NOESCAPE:
            pi += 1
            if pi >= len(p):
                return False
            c = p[pi]
        if si >= se or fold(s[si], flags) != fold(c, flags):
            return False
        pi += 1
        si += 1


def unescape(t):
    out, i = [], 0
    while i < len(t):
        if t[i] == "\\" and t[i + 1:i + 2] == "x":
            if i + 4 <= len(t):
                out.append(chr(int(t[i + 2:i + 4], 16)))
                i += 4
            else:
                i += 2
        else:
            out.append(t[i])
            i += 1
    return "".join(out)


def main():
    oracle = Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
    strings = next(l for l in oracle if l.startswith("# strings: "))[11:].split(" ")
    strings = [unescape(s) for s in strings]
    bad, n = [], 0
    for line in oracle:
        if line.startswith("#"):
            continue
        flags_s, pat, answers = line.split(" ")
        if len(sys.argv) > 3 and not flags_s.startswith(sys.argv[3]):
            continue
        flags = sum(FLAG_NAMES[f] for f in flags_s.split("|"))
        p = unescape(pat)
        for s, want in zip(strings, answers):
            got = "0" if match(p, 0, s, 0, len(s), flags) else "1"
            n += 1
            if got != want:
                bad.append(f"{flags_s} {p!r} {s!r}: glibc {want}, model {got}")
    print(f"{len(bad)} of {n} differ")
    for b in bad[:int(sys.argv[2]) if len(sys.argv) > 2 else 40]:
        print(" ", b)


if __name__ == "__main__":
    main()
