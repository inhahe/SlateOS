#!/usr/bin/env python3
"""Measure GNU coreutils' two quoting styles, byte for byte.

Run this under a real GNU userland (`wsl -e python3 scripts/quote-probe.py`),
not under MSYS: MSYS re-encodes argv, so a name holding a byte that is not
valid UTF-8 never reaches the program intact and the measurement is of the
translation layer rather than of `quotearg`.

Two GNU diagnostics are used as instruments, each of which renders its
argument with one of the two styles and nothing else:

    sort -- NAME        ->  "sort: cannot read: <quotef NAME>: ..."
    head -- NAME        ->  "head: cannot open <quoteaf NAME> for reading: ..."
    sort --sort=WORD    ->  "sort: invalid argument <quote WORD> for ..."

Both instruments only ever *open* their argument, which matters: the corpus
holds `.` and `..`, and an instrument that walks a directory instead of
opening it -- `du` renders `quoteaf` too, and was tried first -- spends the
run traversing the tree it was started in rather than measuring anything.
The probe also runs in a fresh empty directory for the same reason: it should
not be able to read, or be slowed by, whatever happens to be around it.

The three are gnulib's `shell_escape_quoting_style` (quote only when the shell
would need it), `shell_escape_always_quoting_style` (the same, but never bare)
and `locale_quoting_style` (always quote, C escapes). Which a utility uses is
decided by the shape of the sentence, not by the utility: a name that ends the
message, as in `wc: NAME: No such file`, gets `quotef`, while one embedded in a
sentence, as in `rm: cannot remove 'NAME': ...`, gets `quoteaf`, because there
a bare name would run into the words around it. Option arguments and other
non-file text get `quote`.
"""

import os
import random
import re
import subprocess
import sys
import tempfile

# `C.UTF-8`, not `C`. Only one of the three styles moves with the locale --
# `locale_quoting_style`, the one behind `quote()` -- and it is the difference
# between `'zzz'` and `'zzz'` with the curly marks. Measuring under `LC_ALL=C`
# measured a locale SlateOS does not have and never will (design-decisions.md
# §351, and Q38 for the premise that the string layer is UTF-8 full stop), and
# so pinned the ASCII branch of GNU as if it were the only one.
#
# The choice also reaches the *file name* styles, though not through the quote
# marks -- those stay straight in every locale. It reaches them through
# printability. Under `C` a valid multi-byte character is not printable and
# comes back escaped, as `'no such '$'\303\251'' file'`, while under `C.UTF-8`
# it prints as itself: `'no such <e-acute> file'`.
#
# That difference used to be invisible here, because the corpus was built from
# single bytes and from an alphabet whose high bytes (`\xff`, `\x80`) never
# form a valid sequence. `MULTIBYTE` below exists to make it visible: every
# character in it is a *valid* sequence, so every row it produces is one the
# byte-only corpus could not have produced.
ENV = dict(os.environ, LC_ALL="C.UTF-8")
INVALID = re.compile(rb"^sort: (?:invalid|ambiguous) argument ")
# Which failure `sort` hit does not matter here; all three render the name the
# same way, and the corpus holds names that are directories as well as names
# that do not exist.
UNREADABLE = re.compile(rb"^sort: (?:cannot read|read failed|open failed): ")


def quotef(name: bytes) -> bytes | None:
    """How GNU renders `name` as a file name in a diagnostic.

    `None` when the name did not fail to be read at all -- `-` is standard
    input, which succeeds -- and so produced nothing to measure.
    """
    r = subprocess.run([b"sort", b"--", name], capture_output=True, env=ENV,
                       stdin=subprocess.DEVNULL)
    line = r.stderr.split(b"\n")[0]
    m = UNREADABLE.match(line)
    if not m:
        return None
    # The tail is ": <strerror>"; a rendered name never ends in one, because
    # every byte that could forge one is either escaped or quoted.
    return line[m.end():].rsplit(b": ", 1)[0]


OPEN_FAILED = re.compile(rb"^head: cannot open ")
READ_FAILED = re.compile(rb"^head: error reading ")


def quoteaf(name: bytes) -> bytes | None:
    """How GNU renders `name` when the name sits inside a sentence.

    `None` when `head` had nothing to complain about -- `-` is standard
    input, which succeeds.
    """
    r = subprocess.run([b"head", b"--", name], capture_output=True, env=ENV,
                       stdin=subprocess.DEVNULL)
    line = r.stderr.split(b"\n")[0]
    # A name that does not exist gets the first sentence, a directory the
    # second; both render the name with `quoteaf`. `rsplit` takes the last
    # occurrence, so a name that itself contains the suffix -- `a for
    # reading: z` -- still splits in the right place.
    m = OPEN_FAILED.match(line)
    if m:
        return line[m.end():].rsplit(b" for reading: ", 1)[0]
    m = READ_FAILED.match(line)
    if m:
        return line[m.end():].rsplit(b": ", 1)[0]
    return None


def quote(arg: bytes) -> bytes | None:
    """How GNU renders `arg` as an option argument in a diagnostic.

    `None` when the argument turned out to name an ordering after all -- `v`
    is an unambiguous abbreviation of `version` -- and so was accepted rather
    than reported. Those bytes are covered by the other shapes.
    """
    r = subprocess.run([b"sort", b"--sort=" + arg], capture_output=True, env=ENV,
                       stdin=subprocess.DEVNULL)
    line = r.stderr.split(b"\n")[0]
    m = INVALID.match(line)
    if not m:
        return None
    return line[m.end():].rsplit(b" for ", 1)[0]


# Sequences that are *valid* UTF-8, one per reason the answer could differ.
# Under `C.UTF-8` the printability test GNU applies is `iswprint` on a decoded
# wide character, so these are the rows that tell a character test from a byte
# test -- the byte-only corpus below cannot, because every high byte in it is
# a lone one.
#
# Four groups, and the third and fourth are the interesting ones:
#
#   * printable at each UTF-8 length (2, 3 and 4 bytes), so each arm of the
#     decoder is exercised;
#   * `quote()`'s own delimiters, U+2018 and U+2019, which it has to keep
#     distinguishable from the marks it is adding;
#   * characters glibc calls *unprintable* even though they decode -- the C1
#     controls, and the line/paragraph separators;
#   * characters whose printability is a property of glibc's Unicode tables
#     rather than of anything the OS decides -- an unassigned code point, a
#     private-use one, a format character. `design-decisions.md` §101 declined
#     to model those tables, so these rows are expected to differ and
#     `tests/quotearg.rs` names them.
MULTIBYTE = [
    "\u00e9",   # two bytes, printable  (LATIN SMALL LETTER E WITH ACUTE)
    "\u20ac",   # three bytes, printable (EURO SIGN)
    "\U0001f600",  # four bytes, printable (GRINNING FACE)
    "\u4e00",   # three bytes, printable, double-width (CJK IDEOGRAPH ONE)
    "\u00a0",   # NO-BREAK SPACE -- a space that is not `isspace`-ish here
    "\u0301",   # COMBINING ACUTE ACCENT
    "\u2018",   # quote()'s own opening mark
    "\u2019",   # quote()'s own closing mark
    "\u0080",   # C1 control, decodes, not printable
    "\u009f",   # C1 control, the other end of the range
    "\u00ad",   # SOFT HYPHEN (Cf)
    "\u200b",   # ZERO WIDTH SPACE (Cf)
    "\ufeff",   # ZERO WIDTH NO-BREAK SPACE (Cf)
    "\u2028",   # LINE SEPARATOR (Zl)
    "\u2029",   # PARAGRAPH SEPARATOR (Zp)
    "\u0378",   # unassigned
    "\ue000",   # private use
]

# Sequences that are *not* valid UTF-8, one per way a decode can fail. These
# must stay byte-escaped whatever the printability test decides, and they are
# what pins the boundary between "decoded a character" and "did not".
UNDECODABLE = [
    b"\xc3",              # two-byte lead, continuation missing
    b"\xc3z",             # two-byte lead, followed by a non-continuation
    b"\xe2\x82",          # three-byte sequence, truncated
    b"\xf0\x9f\x98",      # four-byte sequence, truncated
    b"\xc0\xaf",          # overlong encoding of `/`
    b"\xed\xa0\x80",      # a surrogate, which UTF-8 may not encode
    b"\xf4\x90\x80\x80",  # above U+10FFFF
    b"\x80",              # a continuation byte with no lead
    b"\xff",              # not a lead byte at all
]


def corpus() -> list[bytes]:
    """Adversarial names, then random ones over the bytes that matter."""
    fixed = [
        b"", b"a", b"ab", b"-", b"--", b"-a", b".", b"..",
        b"'", b"''", b"'a", b"a'", b"a'z", b"a'z'", b"'a'",
        b'"', b'a"z', b"a'z\"", b'a"z\'',
        b" ", b"a z", b" a", b"a ",
        b"#", b"a#z", b"#a", b"~", b"a~z", b"~a",
        b"%", b"+", b",", b".", b"@", b"]", b"_", b"{", b"}",
        b"!", b"$", b"&", b"(", b")", b"*", b":", b";", b"<", b"=",
        b">", b"?", b"[", bytes([92]), b"^", b"`", b"|",
        b"\t", b"\n", b"\r", b"\x01", b"\x7f", b"\xff", b"\x80",
        b"a\tz", b"a\nz", b"a\x01z", b"a\xffz",
        b"a'z\n", b"'\n", b"\n'", b"'a\n", b"a\n'", b"'a'\n", b"\n'a",
        b"\n\n", b"a\nz\nb", b"a'b\tc'd", b"\xff'", b"'\xff", b"a'\xff",
        b"z'\n", b"nl\nsort: forged: line", b"a" * 40,
        # A printable character, an undecodable byte and ASCII in one name.
        # This is the row that shows how the two kinds of run are grouped:
        # GNU prints `'a<e-acute>b'$'\377''c'`, so the printable character
        # joins the *literal* run rather than starting an escape of its own.
        "a\u00e9b".encode() + b"\xffc",
        b"\xff" + "\u00e9".encode(),
        "\u00e9".encode() + b"\xff",
        # A decodable-but-unprintable character between two printable ones,
        # which is the same grouping question with the roles swapped.
        "a\u0080b".encode(),
        "\u00e9\u0080\u00e9".encode(),
        # Both of quote()'s marks inside the thing being quoted.
        "\u2018a\u2019".encode(),
    ]
    for s in MULTIBYTE:
        fixed.append(s.encode())
        fixed.append(("a" + s + "z").encode())
        fixed.append(("a'z" + s).encode())
    fixed.extend(UNDECODABLE)
    random.seed(20260816)
    alpha = b"a'\n\t \"$" + bytes([92]) + b"!#~%*z:;<>?[]{}|&()\x01\x7f\xff\x80"
    for _ in range(600):
        n = random.randint(0, 6)
        fixed.append(bytes(random.choice(alpha) for _ in range(n)))
    seen, out = set(), []
    for c in fixed:
        if c not in seen:
            seen.add(c)
            out.append(c)
    return out


def main() -> int:
    dest = sys.argv[1] if len(sys.argv) > 1 else "-"
    if dest != "-":
        dest = os.path.abspath(dest)
    # Measure from an empty directory: the corpus contains `.` and `..`, and
    # a name that happens to exist would be opened rather than reported.
    os.chdir(tempfile.mkdtemp(prefix="quote-probe-"))
    lines = [
        "# GNU coreutils quoting, measured. Do not hand-edit.",
        "# Produced by scripts/quote-probe.py under:",
        "#   " + subprocess.run(["sort", "--version"], capture_output=True,
                                text=True).stdout.split("\n")[0],
        "# Each row is <style> <input-hex> <output-hex>, LC_ALL=C.UTF-8.",
    ]
    def row(style, fn, arg):
        rendered = fn(arg)
        if rendered is not None:
            lines.append("%s %s %s" % (style, arg.hex(), rendered.hex()))

    for name in corpus():
        row("quotef", quotef, name)
        row("quoteaf", quoteaf, name)
        row("quote", quote, name)
    # Every byte, in every position that has been observed to matter: alone,
    # leading, trailing and interior, and each of those again beside a single
    # quote -- because which outer quote gets chosen depends on both.
    shapes = (b"%c", b"a%cz", b"%cz", b"a%c",
              b"a'z%c", b"%ca'z", b"a%c'z", b"'%c", b"%c'")
    for b in range(1, 256):
        for shape in shapes:
            name = shape.replace(b"%c", bytes([b]))
            if b != 0x2F:
                row("quotef", quotef, name)
                row("quoteaf", quoteaf, name)
            row("quote", quote, name)
    # The same sweep for whole *characters* and for whole undecodable
    # sequences, because position decides the outer quote here too -- and
    # because a sequence that straddles the boundary between a literal run and
    # an escaped one is exactly where a byte-at-a-time renderer goes wrong.
    for seq in [s.encode() for s in MULTIBYTE] + UNDECODABLE:
        for shape in shapes:
            name = shape.replace(b"%c", seq)
            row("quotef", quotef, name)
            row("quoteaf", quoteaf, name)
            row("quote", quote, name)
    text = "\n".join(lines) + "\n"
    if dest == "-":
        sys.stdout.write(text)
    else:
        with open(dest, "w", encoding="ascii", newline="\n") as f:
            f.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
