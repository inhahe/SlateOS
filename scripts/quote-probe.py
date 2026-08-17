#!/usr/bin/env python3
"""Measure GNU coreutils' two quoting styles, byte for byte.

Run this under a real GNU userland (`wsl -e python3 scripts/quote-probe.py`),
not under MSYS: MSYS re-encodes argv, so a name holding a byte that is not
valid UTF-8 never reaches the program intact and the measurement is of the
translation layer rather than of `quotearg`.

Two GNU diagnostics are used as instruments, each of which renders its
argument with one of the two styles and nothing else:

    sort -- NAME        ->  "sort: cannot read: <quotef NAME>: ..."
    sort --sort=WORD    ->  "sort: invalid argument <quote WORD> for ..."

`quotef` is gnulib's `shell_escape_quoting_style` (quote only when the shell
would need it) and is what a file name goes through; `quote` is
`locale_quoting_style` (always quote, C escapes) and is what an option
argument goes through.
"""

import os
import random
import re
import subprocess
import sys

ENV = dict(os.environ, LC_ALL="C")
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
    ]
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
    lines = [
        "# GNU coreutils quoting, measured. Do not hand-edit.",
        "# Produced by scripts/quote-probe.py under:",
        "#   " + subprocess.run(["sort", "--version"], capture_output=True,
                                text=True).stdout.split("\n")[0],
        "# Each row is <style> <input-hex> <output-hex>, LC_ALL=C.",
    ]
    def row(style, fn, arg):
        rendered = fn(arg)
        if rendered is not None:
            lines.append("%s %s %s" % (style, arg.hex(), rendered.hex()))

    for name in corpus():
        row("quotef", quotef, name)
        row("quote", quote, name)
    # Every byte, in every position that has been observed to matter: alone,
    # leading, trailing and interior, and each of those again beside a single
    # quote -- because which outer quote gets chosen depends on both.
    for b in range(1, 256):
        for shape in (b"%c", b"a%cz", b"%cz", b"a%c",
                      b"a'z%c", b"%ca'z", b"a%c'z", b"'%c", b"%c'"):
            name = shape.replace(b"%c", bytes([b]))
            if b != 0x2F:
                row("quotef", quotef, name)
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
