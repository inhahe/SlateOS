"""POSIX regular expressions as the standard defines their matching, parsed as
glibc 2.39 parses them -- the reference regex_harness.py holds glibc's answers
against, to find where glibc's matching departs from the standard.

    import regex_model
    regex_model.compile(pattern: bytes, cflags) -> Regex, or raises Error(code)
    Regex.exec(string: bytes, eflags, start=0, end=None) -> None or [(so, eo), ...]

**The parser is glibc's grammar** -- regcomp.c's peek_token, parse_reg_exp,
parse_branch, parse_expression, parse_sub_exp, parse_dup_op and
parse_bracket_exp, rewritten, for RE_SYNTAX_POSIX_BASIC and _EXTENDED as
regcomp chooses them. POSIX leaves undefined most of what a parser must
decide (what `a**` is, whether `^` in the middle of a BRE is an anchor, what
an unmatched `)` means, which error a malformed interval gets), and there
glibc's answer is the answer. The one place this parser reads the pattern
otherwise is REG_ICASE: glibc upper-cases the pattern before parsing it, so
that `[Z-a]` becomes the reversed range `[Z-A]` and is refused, and
`[a-Z]`, reversed, is accepted as `[A-Z]`; this parses the pattern as it was
written and matches each character "and its case counterpart", as XBD 9.2
says.

**The matcher is the standard's own rule, not glibc's.** XBD 9.1: "the
longest of the leftmost matches", and then "each subpattern, from left to
right, shall match the longest possible string", where "a null string shall
be considered to be longer than no match at all". Read as Okui and Suzuki
formalise it (CIAA 2010, and Borsotti and Trofimovich after them): of every
parse of the leftmost-longest match, the one chosen is greatest when parse
trees are compared position by position in pre-order, each position by the
length of what it matched (-1 for a subpattern that did not take part). So:

- a concatenation's elements are maximised in turn, the first first;
- of the alternatives that match the same text, the first;
- a repetition's iterations are maximised in turn. An iteration past the
  minimum count must be non-empty, and one that is empty is taken only as the
  sole iteration of a repetition that matched nothing -- POSIX's own
  example, `\\(a*\\)*` against "bc", reports the null string for `\\1`.

glibc chooses otherwise: among the paths to the longest match it takes the
first in its NFA's order, which prefers the first alternative and the
greedier loop wherever the overall length allows. `(a|ab)(c|bcd)` against
"abcd" is `\\1` = "a" there and "ab" here.

pmatch is then as regexec's page says: a subexpression reports its last
match, one that took no part is -1, and one inside another reports only
within what the enclosing one reports.

**How:** every parse is enumerated, keeping for each (end, groups a later
back-reference can see) only the best parse -- which is exact, since a parse
is compared in pre-order and what follows depends only on those. Slow, but
this runs over short strings, off the tests' path.
"""

import sys

# The parser and the matcher recurse once a level of nesting; the edge cases
# nest two hundred deep.
sys.setrecursionlimit(max(sys.getrecursionlimit(), 20000))

REG_EXTENDED, REG_ICASE, REG_NEWLINE, REG_NOSUB = 1, 2, 4, 8
REG_NOTBOL, REG_NOTEOL, REG_STARTEND = 1, 2, 4

REG_NOMATCH = 1
REG_BADPAT = 2
REG_ECOLLATE = 3
REG_ECTYPE = 4
REG_EESCAPE = 5
REG_ESUBREG = 6
REG_EBRACK = 7
REG_EPAREN = 8
REG_EBRACE = 9
REG_BADBR = 10
REG_ERANGE = 11
REG_ESPACE = 12
REG_BADRPT = 13
REG_EEND = 14
REG_ESIZE = 15
REG_ERPAREN = 16

RE_DUP_MAX = 0x7FFF


class Error(Exception):
    """regcomp's answer for a pattern it refuses."""

    def __init__(self, code: int):
        super().__init__(code)
        self.code = code


# Tokens, glibc's names shortened.
(CHAR, ALT, STAR, PLUS, QMARK, OPEN_DUP, CLOSE_DUP, OPEN, CLOSE, BRACKET, PERIOD,
 ANCHOR, BACKREF, WORD, NOTWORD, SPACE, NOTSPACE, BACKSLASH, END) = range(19)
# Bracket tokens.
(B_CHAR, B_OPEN_COLL, B_OPEN_EQUIV, B_OPEN_CLASS, B_RANGE, B_CLOSE, B_NON_MATCH, B_END) = range(8)


class Tok:
    __slots__ = ("type", "c", "arg", "len")

    def __init__(self, type_, c=0, arg=None, length=1):
        self.type, self.c, self.arg, self.len = type_, c, arg, length


# The C locale's classes, as glibc's ctype has them.
def _cls(pred):
    return frozenset(b for b in range(256) if pred(b))


def _alpha(b):
    return 0x41 <= b <= 0x5A or 0x61 <= b <= 0x7A


def _digit(b):
    return 0x30 <= b <= 0x39


CLASSES = {
    b"alpha": _cls(_alpha),
    b"upper": _cls(lambda b: 0x41 <= b <= 0x5A),
    b"lower": _cls(lambda b: 0x61 <= b <= 0x7A),
    b"digit": _cls(_digit),
    b"xdigit": _cls(lambda b: _digit(b) or 0x41 <= b <= 0x46 or 0x61 <= b <= 0x66),
    b"space": _cls(lambda b: b == 0x20 or 0x09 <= b <= 0x0D),
    b"print": _cls(lambda b: 0x20 <= b <= 0x7E),
    b"punct": _cls(lambda b: 0x21 <= b <= 0x7E and not _alpha(b) and not _digit(b)),
    b"graph": _cls(lambda b: 0x21 <= b <= 0x7E),
    b"cntrl": _cls(lambda b: b < 0x20 or b == 0x7F),
    b"blank": _cls(lambda b: b in (0x20, 0x09)),
    b"alnum": _cls(lambda b: _alpha(b) or _digit(b)),
}
WORD_CHARS = CLASSES[b"alnum"] | {0x5F}
ALL = frozenset(range(256))


def other_case(b: int) -> int:
    if 0x41 <= b <= 0x5A:
        return b + 0x20
    if 0x61 <= b <= 0x7A:
        return b - 0x20
    return b


def case_closed(s) -> frozenset:
    return frozenset(s) | frozenset(other_case(b) for b in s)


class Parser:
    """glibc's regcomp grammar over `pattern`, for BRE or ERE."""

    def __init__(self, pattern: bytes, cflags: int):
        self.p = pattern
        self.ere = bool(cflags & REG_EXTENDED)
        self.icase = bool(cflags & REG_ICASE)
        self.newline = bool(cflags & REG_NEWLINE)
        self.pos = 0
        self.tok = None
        self.nsub = 0
        self.completed = 0

    # -- tokens -----------------------------------------------------------

    def peek(self, i: int, caret_here: bool) -> Tok:
        p = self.p
        if i >= len(p):
            return Tok(END, 0, None, 0)
        c = p[i]
        if c == 0x5C:
            if i + 1 >= len(p):
                return Tok(BACKSLASH, c)
            c2 = p[i + 1]
            t = Tok(CHAR, c2, None, 2)
            ch = chr(c2)
            if ch == "|" and not self.ere:
                t.type = ALT
            elif "1" <= ch <= "9":
                t.type, t.arg = BACKREF, c2 - 0x31
            elif ch == "<":
                t.type, t.arg = ANCHOR, "wstart"
            elif ch == ">":
                t.type, t.arg = ANCHOR, "wend"
            elif ch == "b":
                t.type, t.arg = ANCHOR, "wordb"
            elif ch == "B":
                t.type, t.arg = ANCHOR, "notwordb"
            elif ch == "w":
                t.type = WORD
            elif ch == "W":
                t.type = NOTWORD
            elif ch == "s":
                t.type = SPACE
            elif ch == "S":
                t.type = NOTSPACE
            elif ch == "`":
                t.type, t.arg = ANCHOR, "bufstart"
            elif ch == "'":
                t.type, t.arg = ANCHOR, "bufend"
            elif ch == "(" and not self.ere:
                t.type = OPEN
            elif ch == ")" and not self.ere:
                t.type = CLOSE
            elif ch == "+" and not self.ere:
                t.type = PLUS
            elif ch == "?" and not self.ere:
                t.type = QMARK
            elif ch == "{" and not self.ere:
                t.type = OPEN_DUP
            elif ch == "}" and not self.ere:
                t.type = CLOSE_DUP
            return t
        t = Tok(CHAR, c)
        ch = chr(c)
        if ch == "|" and self.ere:
            t.type = ALT
        elif ch == "*":
            t.type = STAR
        elif ch == "+" and self.ere:
            t.type = PLUS
        elif ch == "?" and self.ere:
            t.type = QMARK
        elif ch == "{" and self.ere:
            t.type = OPEN_DUP
        elif ch == "}" and self.ere:
            t.type = CLOSE_DUP
        elif ch == "(" and self.ere:
            t.type = OPEN
        elif ch == ")" and self.ere:
            t.type = CLOSE
        elif ch == "[":
            t.type = BRACKET
        elif ch == ".":
            t.type = PERIOD
        elif ch == "^":
            if self.ere or caret_here or i == 0:
                t.type, t.arg = ANCHOR, "bol"
        elif ch == "$":
            if self.ere or i + 1 == len(p) or self.peek(i + 1, False).type in (ALT, CLOSE):
                t.type, t.arg = ANCHOR, "eol"
        return t

    def fetch(self, caret_here: bool = False) -> None:
        self.tok = self.peek(self.pos, caret_here)
        self.pos += self.tok.len

    def peek_bracket(self, i: int) -> Tok:
        p = self.p
        if i >= len(p):
            return Tok(B_END, 0, None, 0)
        c = p[i]
        if c == 0x5B:
            c2 = p[i + 1] if i + 1 < len(p) else 0
            kind = {0x2E: B_OPEN_COLL, 0x3D: B_OPEN_EQUIV, 0x3A: B_OPEN_CLASS}.get(c2)
            if kind is not None:
                return Tok(kind, c2, None, 2)
            return Tok(B_CHAR, c)
        kind = {0x2D: B_RANGE, 0x5D: B_CLOSE, 0x5E: B_NON_MATCH}.get(c, B_CHAR)
        return Tok(kind, c)

    # -- grammar ----------------------------------------------------------

    def parse(self):
        self.fetch(caret_here=True)
        tree = self.reg_exp(0)
        assert self.tok.type == END
        return tree

    def reg_exp(self, nest: int):
        initial = self.completed
        branches = [self.branch(nest)]
        while self.tok.type == ALT:
            self.fetch(caret_here=True)
            if self.tok.type not in (ALT, END) and (nest == 0 or self.tok.type != CLOSE):
                accumulated = self.completed
                self.completed = initial
                b = self.branch(nest)
                self.completed |= accumulated
            else:
                b = None
            branches.append(b)
        if len(branches) == 1:
            return branches[0]
        return ("alt", [EMPTY if b is None else b for b in branches])

    def branch(self, nest: int):
        items = []
        e = self.expression(nest)
        if e is not None:
            items.append(e)
        while self.tok.type not in (ALT, END) and (nest == 0 or self.tok.type != CLOSE):
            e = self.expression(nest)
            if e is not None:
                items.append(e)
        if not items:
            return None
        return items[0] if len(items) == 1 else ("cat", items)

    def expression(self, nest: int):
        t = self.tok
        if t.type == CHAR:
            tree = self.literal(t.c)
        elif t.type == OPEN:
            tree = self.sub_exp(nest + 1)
        elif t.type == BRACKET:
            tree = self.bracket()
        elif t.type == BACKREF:
            if not (self.completed >> t.arg) & 1:
                raise Error(REG_ESUBREG)
            tree = ("bref", t.arg + 1)
        elif t.type in (OPEN_DUP, STAR, PLUS, QMARK, CLOSE, CLOSE_DUP):
            if t.type == OPEN_DUP and not self.ere:
                raise Error(REG_BADRPT)
            if t.type in (OPEN_DUP, STAR, PLUS, QMARK) and self.ere:
                raise Error(REG_BADRPT)
            if t.type == CLOSE and not self.ere:
                raise Error(REG_EPAREN)
            tree = self.literal(t.c)
        elif t.type == ANCHOR:
            tree = ("assert", t.arg)
            self.fetch()
            return tree
        elif t.type == PERIOD:
            s = set(ALL)
            s.discard(0)
            if self.newline:
                s.discard(0x0A)
            tree = ("set", frozenset(s))
        elif t.type in (WORD, NOTWORD, SPACE, NOTSPACE):
            s = WORD_CHARS if t.type in (WORD, NOTWORD) else CLASSES[b"space"]
            if t.type in (NOTWORD, NOTSPACE):
                s = ALL - s
            tree = ("set", frozenset(s))
        elif t.type in (ALT, END):
            return None
        elif t.type == BACKSLASH:
            raise Error(REG_EESCAPE)
        else:
            raise AssertionError(t.type)
        self.fetch()
        while self.tok.type in (STAR, PLUS, QMARK, OPEN_DUP):
            tree = self.dup_op(tree)
            if not self.ere and self.tok.type in (STAR, OPEN_DUP):
                raise Error(REG_BADRPT)
        return tree

    def literal(self, c: int):
        return ("set", case_closed({c}) if self.icase else frozenset({c}))

    def sub_exp(self, nest: int):
        cur = self.nsub
        self.nsub += 1
        self.fetch(caret_here=True)
        if self.tok.type == CLOSE:
            inner = None
        else:
            inner = self.reg_exp(nest)
            if self.tok.type != CLOSE:
                raise Error(REG_EPAREN)
        if cur <= 8:
            self.completed |= 1 << cur
        return ("group", cur + 1, inner)

    def fetch_number(self) -> int:
        num = -1
        while True:
            self.fetch()
            t = self.tok
            if t.type == END:
                return -2
            if t.type == CLOSE_DUP or t.c == 0x2C:
                return num
            if t.type != CHAR or not 0x30 <= t.c <= 0x39 or num == -2:
                num = -2
            elif num == -1:
                num = t.c - 0x30
            else:
                num = min(RE_DUP_MAX + 1, num * 10 + t.c - 0x30)

    def dup_op(self, elem):
        t = self.tok
        if t.type == OPEN_DUP:
            start = self.fetch_number()
            if start == -1:
                if self.tok.type == CHAR and self.tok.c == 0x2C:
                    start = 0
                else:
                    raise Error(REG_BADBR)
            end = 0
            if start != -2:
                if self.tok.type == CLOSE_DUP:
                    end = start
                elif self.tok.type == CHAR and self.tok.c == 0x2C:
                    end = self.fetch_number()
                else:
                    end = -2
            if start == -2 or end == -2:
                raise Error(REG_EBRACE if self.tok.type == END else REG_BADBR)
            if (end != -1 and start > end) or self.tok.type != CLOSE_DUP:
                raise Error(REG_BADBR)
            if RE_DUP_MAX < (start if end == -1 else end):
                raise Error(REG_ESIZE)
        else:
            start = 1 if t.type == PLUS else 0
            end = 1 if t.type == QMARK else -1
        self.fetch()
        if elem is None or (start == 0 and end == 0):
            return None
        return ("rep", elem, start, None if end == -1 else end)

    # -- bracket expressions ----------------------------------------------

    def bracket(self):
        t = self.peek_bracket(self.pos)
        if t.type == B_END:
            raise Error(REG_BADPAT)
        non_match = False
        if t.type == B_NON_MATCH:
            non_match = True
            self.pos += t.len
            t = self.peek_bracket(self.pos)
            if t.type == B_END:
                raise Error(REG_BADPAT)
        if t.type == B_CLOSE:
            t.type = B_CHAR
        chars = set()
        first_round = True
        while True:
            start = self.bracket_element(t, first_round)
            first_round = False
            t = self.peek_bracket(self.pos)
            is_range = False
            if start[0] not in ("class", "equiv"):
                if t.type == B_END:
                    raise Error(REG_EBRACK)
                if t.type == B_RANGE:
                    self.pos += t.len
                    t2 = self.peek_bracket(self.pos)
                    if t2.type == B_END:
                        raise Error(REG_EBRACK)
                    if t2.type == B_CLOSE:
                        self.pos -= t.len
                        t.type = B_CHAR
                    else:
                        is_range = True
            if is_range:
                end = self.bracket_element(t2, True)
                t = self.peek_bracket(self.pos)
                chars |= self.range(start, end)
            else:
                chars |= self.element(start)
            if t.type == B_END:
                raise Error(REG_EBRACK)
            if t.type == B_CLOSE:
                break
        self.pos += t.len
        if self.icase:
            chars = set(case_closed(chars))
        if non_match:
            chars = set(ALL - chars)
            if self.newline:
                chars.discard(0x0A)
        return ("set", frozenset(chars))

    def bracket_element(self, t: Tok, accept_hyphen: bool):
        self.pos += t.len
        if t.type in (B_OPEN_COLL, B_OPEN_EQUIV, B_OPEN_CLASS):
            return self.bracket_symbol(t)
        if t.type == B_RANGE and not accept_hyphen:
            if self.peek_bracket(self.pos).type != B_CLOSE:
                raise Error(REG_ERANGE)
        return ("char", t.c)

    def bracket_symbol(self, t: Tok):
        p, delim = self.p, t.c
        if self.pos >= len(p):
            raise Error(REG_EBRACK)
        name = bytearray()
        i = 0
        while True:
            if i >= 32:
                raise Error(REG_EBRACK)
            ch = p[self.pos]
            self.pos += 1
            if self.pos >= len(p):
                raise Error(REG_EBRACK)
            if ch == delim and p[self.pos] == 0x5D:
                break
            name.append(ch)
            i += 1
        self.pos += 1
        kind = {B_OPEN_COLL: "coll", B_OPEN_EQUIV: "equiv", B_OPEN_CLASS: "class"}[t.type]
        return (kind, bytes(name))

    def element(self, e) -> set:
        kind, v = e
        if kind == "char":
            return {v}
        if kind in ("coll", "equiv"):
            if len(v) != 1:
                raise Error(REG_ECOLLATE)
            return {v[0]}
        s = CLASSES.get(v)
        if s is None:
            raise Error(REG_ECTYPE)
        return set(s)

    def range(self, a, b) -> set:
        if a[0] in ("equiv", "class") or b[0] in ("equiv", "class"):
            raise Error(REG_ERANGE)

        def seq(e):
            if e[0] == "char":
                return e[1]
            if len(e[1]) == 1:
                return e[1][0]
            raise Error(REG_ECOLLATE)

        lo, hi = seq(a), seq(b)
        if lo > hi:
            raise Error(REG_ERANGE)
        return set(range(lo, hi + 1))


EMPTY = ("empty",)


class Regex:
    def __init__(self, tree, nsub: int, cflags: int):
        self.tree = EMPTY if tree is None else tree
        self.re_nsub = nsub
        self.cflags = cflags
        self.refs = set()
        _refs(self.tree, self.refs)
        self.parent = {}
        self.inner = {}
        _nesting(self.tree, None, self.parent, self.inner)

    def exec(self, s: bytes, eflags: int = 0, start: int = 0, end=None):
        """The match's pmatch, groups 0..re_nsub, or None for no match."""
        n = len(s) if end is None else end
        m = _Matcher(self, s, n, eflags)
        for first in range(start, n + 1):
            best = m.best(first)
            if best is not None:
                e, tree = best
                pm = [(-1, -1)] * (self.re_nsub + 1)
                pm[0] = (first, e)
                _report(tree, pm, self.inner)
                return pm
        return None


def compile(pattern: bytes, cflags: int) -> Regex:
    p = Parser(pattern, cflags)
    tree = p.parse()
    return Regex(tree, p.nsub, cflags)


def _refs(node, out: set) -> None:
    kind = node[0]
    if kind == "bref":
        out.add(node[1])
    elif kind == "group":
        if node[2] is not None:
            _refs(node[2], out)
    elif kind in ("cat", "alt"):
        for c in node[1]:
            _refs(c, out)
    elif kind == "rep":
        _refs(node[1], out)


def _nesting(node, enclosing, parent: dict, inner: dict) -> None:
    """`inner[g]`: every group inside group g."""
    kind = node[0]
    if kind == "group":
        g = node[1]
        parent[g] = enclosing
        inner.setdefault(g, set())
        e = enclosing
        while e is not None:
            inner[e].add(g)
            e = parent[e]
        if node[2] is not None:
            _nesting(node[2], g, parent, inner)
    elif kind in ("cat", "alt"):
        for c in node[1]:
            _nesting(c, enclosing, parent, inner)
    elif kind == "rep":
        _nesting(node[1], enclosing, parent, inner)


# A parse tree: (node, start, end, kids), where kids is
#   cat:   the elements' trees, as a cons list -- (first, rest) or None
#   rep:   the iterations' trees, likewise
#   alt:   (branch index, branch tree)
#   group: (inner tree,) or () for an empty group
#   leaf:  ()
# Cons lists, so that a parse is extended in constant time: a concatenation of
# hundreds of elements is built by prepending, never by copying.


def _norm(t) -> int:
    return t[2] - t[1]


def _compare(a, b) -> int:
    """Okui and Suzuki's order on two parses of the same node over the same
    text: +1 if `a` is preferred, -1 if `b`, 0 if they are alike."""
    kind = a[0][0]
    if kind in ("cat", "rep"):
        return _compare_cons(a[3], b[3])
    if kind == "group":
        if not a[3]:
            return 0
        return _compare(a[3][0], b[3][0])
    if kind == "alt":
        (ia, ta), (ib, tb) = a[3], b[3]
        if ia != ib:
            return 1 if ia < ib else -1
        return _compare(ta, tb)
    return 0


def _compare_cons(x, y) -> int:
    """Two sequences of sibling parses covering the same text, in order: at
    each position the longer wins, one that is absent counting as -1."""
    while x is not None or y is not None:
        if x is None:
            return -1
        if y is None:
            return 1
        tx, ty = x[0], y[0]
        nx, ny = _norm(tx), _norm(ty)
        if nx != ny:
            return 1 if nx > ny else -1
        r = _compare(tx, ty)
        if r:
            return r
        x, y = x[1], y[1]
    return 0


def _first_wins(cand, old) -> bool:
    """Whether `cand`, a non-empty cons list, beats `old`, one for the same
    (end, env) -- empty, None, when it stops where it starts: decided by
    their first elements, since the rest of each is the best there is after
    that first element."""
    if old is None:
        return True
    return _compare_cons((cand[0], None), (old[0], None)) > 0


def _report(tree, pm: list, inner: dict) -> None:
    node, s, e, kids = tree
    kind = node[0]
    if kind == "group":
        g = node[1]
        for i in inner[g]:
            pm[i] = (-1, -1)
        pm[g] = (s, e)
        for k in kids:
            _report(k, pm, inner)
    elif kind in ("cat", "rep"):
        x = kids
        while x is not None:
            _report(x[0], pm, inner)
            x = x[1]
    elif kind == "alt":
        _report(kids[1], pm, inner)


class _Matcher:
    def __init__(self, rx: Regex, s: bytes, n: int, eflags: int):
        self.rx = rx
        self.s = s
        self.n = n
        self.notbol = bool(eflags & REG_NOTBOL)
        self.noteol = bool(eflags & REG_NOTEOL)
        self.newline = bool(rx.cflags & REG_NEWLINE)
        self.icase = bool(rx.cflags & REG_ICASE)
        self.memo = {}

    # The context glibc gives a position: what precedes it and what follows.
    def _prev(self, p: int):
        """(is a word character, begins a line, begins the buffer)."""
        if p == 0:
            return (False, not self.notbol, True)
        c = self.s[p - 1]
        return (c in WORD_CHARS, self.newline and c == 0x0A, False)

    def _next(self, p: int):
        if p == self.n:
            return (False, not self.noteol, True)
        c = self.s[p]
        return (c in WORD_CHARS, self.newline and c == 0x0A, False)

    def holds(self, what: str, p: int) -> bool:
        pw, pl, pb = self._prev(p)
        nw, nl, nb = self._next(p)
        if what == "bol":
            return pl
        if what == "eol":
            return nl
        if what == "bufstart":
            return pb
        if what == "bufend":
            return nb
        if what == "wstart":
            return not pw and nw
        if what == "wend":
            return pw and not nw
        if what == "wordb":
            return pw != nw
        if what == "notwordb":
            return pw == nw
        raise AssertionError(what)

    def best(self, first: int):
        """The preferred parse of the whole pattern from `first`: (end, tree)."""
        found = self.parses(self.rx.tree, first, ())
        if not found:
            return None
        top = None
        for (e, _env), tree in found.items():
            if top is None or e > top[0] or (e == top[0] and _compare(tree, top[1]) > 0):
                top = (e, tree)
        return top

    def parses(self, node, pos: int, env: tuple) -> dict:
        """{(end, env after): the preferred parse of `node` from `pos`} -- env
        being the spans of the groups a back-reference names, as a sorted
        tuple of (group, start, end)."""
        key = (id(node), pos, env)
        hit = self.memo.get(key)
        if hit is not None:
            return hit
        out = {}
        kind = node[0]

        def offer(e, env2, tree):
            k = (e, env2)
            old = out.get(k)
            if old is None or _compare(tree, old) > 0:
                out[k] = tree

        if kind == "set":
            if pos < self.n and self.s[pos] in node[1]:
                offer(pos + 1, env, (node, pos, pos + 1, ()))
        elif kind == "empty":
            offer(pos, env, (node, pos, pos, ()))
        elif kind == "assert":
            if self.holds(node[1], pos):
                offer(pos, env, (node, pos, pos, ()))
        elif kind == "bref":
            span = dict((g, (a, b)) for g, a, b in env).get(node[1])
            if span is not None:
                a, b = span
                length = b - a
                if pos + length <= self.n and self._same(a, pos, length):
                    offer(pos + length, env, (node, pos, pos + length, ()))
        elif kind == "group":
            g, inner = node[1], node[2]
            if inner is None:
                offer(pos, self._bind(env, g, pos, pos), (node, pos, pos, ()))
            else:
                for (e, env2), t in self.parses(inner, pos, env).items():
                    offer(e, self._bind(env2, g, pos, e), (node, pos, e, (t,)))
        elif kind == "alt":
            for i, b in enumerate(node[1]):
                for (e, env2), t in self.parses(b, pos, env).items():
                    offer(e, env2, (node, pos, e, (i, t)))
        elif kind == "cat":
            for (e, env2), kids in self._cat(node[1], pos, env).items():
                offer(e, env2, (node, pos, e, kids))
        elif kind == "rep":
            body, lo, hi = node[1], node[2], node[3]
            for (e, env2), kids in self._rep(body, lo, hi, pos, env).items():
                offer(e, env2, (node, pos, e, kids))
            if lo == 0:
                # The sole empty iteration of a repetition that matched
                # nothing, preferred to none: "a null string ... longer than
                # no match at all".
                for (e, env2), t in self.parses(body, pos, env).items():
                    if e == pos:
                        offer(pos, env2, (node, pos, pos, (t, None)))
        else:
            raise AssertionError(kind)
        self.memo[key] = out
        return out

    def _cat(self, items, pos: int, env: tuple) -> dict:
        """{(end, env): best cons list of element trees} for `items` from pos:
        the states each element can leave the match in, found forwards; then,
        backwards, each state's best way on to every end."""
        reach = [{(pos, env)}]
        for item in items:
            nxt = set()
            for p, e in reach[-1]:
                nxt.update(self.parses(item, p, e).keys())
            reach.append(nxt)
        best = {st: {st: None} for st in reach[-1]}
        for i in range(len(items) - 1, -1, -1):
            cur = {}
            for p, e in reach[i]:
                res = {}
                for (e1, env1), t in self.parses(items[i], p, e).items():
                    for k, rest in best[(e1, env1)].items():
                        cand = (t, rest)
                        if k not in res or _first_wins(cand, res[k]):
                            res[k] = cand
                cur[(p, e)] = res
            best = cur
        return best[(pos, env)]

    def _rep(self, body, lo: int, hi, pos: int, env: tuple) -> dict:
        """{(end, env): best cons list of iterations}: at least `lo`, at most
        `hi` (None for no limit), those past `lo` non-empty."""
        def canon(done):
            return min(done, lo) if hi is None else done

        start = (0, pos, env)
        reach = {start}
        todo = [start]
        edges = {}
        while todo:
            st = todo.pop()
            done, p, e = st
            out = []
            if hi is None or done < hi:
                for (e1, env1), t in self.parses(body, p, e).items():
                    if e1 == p and done >= lo:
                        continue
                    nx = (canon(done + 1), e1, env1)
                    out.append((t, nx))
                    if nx not in reach:
                        reach.add(nx)
                        todo.append(nx)
            edges[st] = out
        # Every edge raises the count, or past the minimum without a limit
        # the position: so this order visits a state after all it leads to.
        order = sorted(reach, key=lambda st: (st[0], st[1]), reverse=True)
        best = {}
        for st in order:
            done, p, e = st
            res = {}
            if done >= lo:
                res[(p, e)] = None
            for t, nx in edges[st]:
                for k, rest in best[nx].items():
                    cand = (t, rest)
                    if k not in res or _first_wins(cand, res[k]):
                        res[k] = cand
            best[st] = res
        return best[start]

    def _bind(self, env: tuple, g: int, a: int, b: int) -> tuple:
        if g not in self.rx.refs:
            return env
        d = dict((x, (y, z)) for x, y, z in env)
        d[g] = (a, b)
        return tuple(sorted((x, y, z) for x, (y, z) in d.items()))

    def _same(self, a: int, pos: int, length: int) -> bool:
        s = self.s
        for k in range(length):
            x, y = s[a + k], s[pos + k]
            if x != y and not (self.icase and other_case(x) == y):
                return False
        return True
