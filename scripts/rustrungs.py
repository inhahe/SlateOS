"""Read what a Rust rung asserts, so an oracle can check the assertion itself.

A "rung" here is an `assert_eq!(call(input), expected, "why")` inside a
`self_test()` that runs at boot.  The rung grades the *implementation*: if
`strip_quotes` stops removing quotes, the rung fails and the boot fails.
What the rung cannot grade is **itself** -- a rung whose `expected` was
transcribed wrongly is confidently, permanently green.

That is the gap the `check-*-vs-bash.py` oracles exist to close: they ask
real bash what the answer should be.  But an oracle can only close it if it
reads the rung's *own* expectation.  Until this module existed, both oracles
compared a Python re-implementation against bash and never opened the Rust
at all except for a single `const` name, so a corrupted rung -- one
asserting a word bash never produces -- passed both.

This module is the reader.  It is deliberately a source parser and not a
regex: an expectation is a Rust expression (`alloc::vec!["a", "b"]`,
`b"a'b".to_vec()`, `Some(17)`), and finding where it ends means tracking
brackets and string state, which a regex cannot do.

Two properties matter more than convenience, because both were bugs in the
draft that preceded this file:

  * **Every occurrence is returned, never the first.**  An input literal can
    appear under two different functions (`"a\\ b"` is a rung of both
    `remove_quotes` and `split_words`) and can appear twice under one.
    Stopping at the first match reads a different rung than the caller
    meant, and passes by luck when the two expectations coincide.

  * **A site only counts if it is the first argument of `assert_eq!`.**
    Otherwise a mention in a `let` binding, or in a comment quoting the
    rung, is mistaken for an assertion and parsed as though the text after
    it were an expected value.
"""
import re

BS = chr(92)

_SIMPLE_ESCAPES = {"n": 10, "t": 9, "r": 13, "0": 0, BS: 92, '"': 34, "'": 39}

# `assert_eq!` then an open paren then optional whitespace, ending exactly
# where the call begins.  40 characters is comfortably more than the macro
# name plus a newline plus the deepest indentation in the kernel sources.
_ASSERT_OPEN = re.compile(r"assert_eq!\s*\(\s*$")
_LOOKBEHIND = 40


class RungParseError(Exception):
    """The source contains something this reader does not understand.

    Raised rather than returned because a reader that silently skips what it
    cannot parse degrades into the very thing it was written to replace: a
    check that reports nothing and reads as a pass.
    """


def unescape_rust(body: str) -> bytes:
    """Decode the body of a Rust `"..."` or `b"..."` literal to its bytes."""
    out = bytearray()
    i = 0
    while i < len(body):
        c = body[i]
        if c == BS and i + 1 < len(body):
            nxt = body[i + 1]
            if nxt in _SIMPLE_ESCAPES:
                out.append(_SIMPLE_ESCAPES[nxt])
                i += 2
                continue
            if nxt == "x":
                out.append(int(body[i + 2 : i + 4], 16))
                i += 4
                continue
            raise RungParseError(f"unhandled escape {BS}{nxt} in {body!r}")
        out.extend(c.encode("utf-8"))
        i += 1
    return bytes(out)


def split_top_level(expr: str) -> list[str]:
    """Split a Rust argument list on commas outside strings and brackets.

    Stops at the closing bracket that ends the list, so the caller may pass
    the whole remainder of the file.
    """
    parts, depth, cur, in_str, esc = [], 0, "", False, False
    for c in expr:
        if in_str:
            cur += c
            if esc:
                esc = False
            elif c == BS:
                esc = True
            elif c == '"':
                in_str = False
            continue
        if c == '"':
            in_str = True
            cur += c
        elif c in "([{":
            depth += 1
            cur += c
        elif c in ")]}":
            if depth == 0:
                break
            depth -= 1
            cur += c
        elif c == "," and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += c
    parts.append(cur)
    return [p.strip() for p in parts if p.strip()]


def literals_in(expr: str) -> list[bytes]:
    """Every string literal in a Rust expression, decoded, in order.

    A `b` prefix needs no special handling: this scans for the quote, and the
    prefix is simply not part of what it collects.
    """
    out = []
    i = 0
    while i < len(expr):
        if expr[i] == '"':
            j, esc = i + 1, False
            while j < len(expr):
                if esc:
                    esc = False
                elif expr[j] == BS:
                    esc = True
                elif expr[j] == '"':
                    break
                j += 1
            if j >= len(expr):
                raise RungParseError(f"unterminated literal in {expr!r}")
            out.append(unescape_rust(expr[i + 1 : j]))
            i = j + 1
        else:
            i += 1
    return out


def expectations(src: str, call: str) -> list[tuple[int, str, list[bytes]]]:
    """Every `assert_eq!(<call>, EXPECTED, ..)` in `src`.

    `call` is the exact Rust text of the call, e.g. `split_words("a b  c")`
    or `strip_quotes(b"a\\\\ b")`.  Returns one `(line, expr, literals)` per
    occurrence, in file order.  An empty list means the rung is absent --
    itself a finding, since the oracle is then asserting something about a
    call the tree does not make.
    """
    found = []
    at = src.find(call)
    while at >= 0:
        before = src[max(0, at - _LOOKBEHIND) : at]
        if _ASSERT_OPEN.search(before):
            rest = src[at + len(call) :].lstrip()
            if rest.startswith(","):
                args = split_top_level(rest[1:])
                if args:
                    line = src.count("\n", 0, at) + 1
                    found.append((line, args[0], literals_in(args[0])))
        at = src.find(call, at + 1)
    return found


# --- discovery floor ------------------------------------------------------
#
# The fixture is a miniature of the real thing and carries, on purpose, one
# rung of each shape this reader has to survive: a bare call, a call with a
# trailing message, the same input under two different functions, a
# non-literal expectation, and a decoy that is NOT an assertion.

_FIXTURE = '''
pub fn self_test() {
    assert_eq!(remove_quotes("a\\\\ b"), "a b");
    assert_eq!(
        split_words("a\\\\ b"),
        alloc::vec!["a b"],
        "an escaped blank does not split"
    );
    assert_eq!(split_words("a b  c"), alloc::vec!["a", "b", "c"], "runs collapse");
    assert_eq!(find_bare(b"a>b", b'>'), Some(1));
    let decoy = split_words("never asserted");
    assert_eq!(strip_quotes(b"\\"C:\\\\dir\\""), b"C:\\\\dir".to_vec());
}
'''


def self_test() -> int:
    """Prove this reader can still find a rung and still tell two apart."""
    fails = 0

    def check(what, got, want):
        nonlocal fails
        ok = got == want
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} {what}")
        if not ok:
            print(f"       got  {got!r}")
            print(f"       want {want!r}")

    # The same input under two functions must resolve to two different rungs.
    rq = expectations(_FIXTURE, 'remove_quotes("a\\\\ b")')
    sw = expectations(_FIXTURE, 'split_words("a\\\\ b")')
    check("remove_quotes rung found exactly once", len(rq), 1)
    check("split_words rung found exactly once", len(sw), 1)
    check("the two rungs are at different lines", rq[0][0] != sw[0][0], True)

    # Content, including a multi-word expectation and an escaped backslash.
    check("bare call with no message", rq[0][2], [b"a b"])
    check("vec! expectation", sw[0][2], [b"a b"])
    check(
        "multi-word vec! expectation",
        expectations(_FIXTURE, 'split_words("a b  c")')[0][2],
        [b"a", b"b", b"c"],
    )
    check(
        "byte-string literal and escaped backslash",
        expectations(_FIXTURE, 'strip_quotes(b"\\"C:\\\\dir\\"")')[0][2],
        [b"C:" + BS.encode() + b"dir"],
    )

    # A non-literal expectation is reported as such, not silently as empty.
    fb = expectations(_FIXTURE, 'find_bare(b"a>b", b\'>\')')
    check("non-literal expectation keeps its raw expression", fb[0][1], "Some(1)")
    check("non-literal expectation yields no literals", fb[0][2], [])

    # The decoy `let` binding must not be mistaken for a rung.
    check(
        "a non-assert mention is not a rung",
        expectations(_FIXTURE, 'split_words("never asserted")'),
        [],
    )

    # An absent rung is an empty list, which the caller must treat as a finding.
    check("absent rung", expectations(_FIXTURE, "split_words(\"nope\")"), [])

    print(f"\nrustrungs self-test: {fails} failure(s)")
    return 1 if fails else 0


if __name__ == "__main__":
    import sys

    sys.exit(self_test())
