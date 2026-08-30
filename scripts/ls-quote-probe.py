#!/usr/bin/env python3
"""Measure all ten of GNU's quoting styles at once, byte for byte.

Run this under a real GNU userland (`wsl -e python3 scripts/ls-quote-probe.py
userspace/coreutils/tests/quotearg-ls-gnu.txt`), never under MSYS: MSYS
re-encodes argv, so a name holding a byte that is not valid UTF-8 never
reaches the program intact and what gets measured is the translation layer
rather than `quotearg`.

## Why a second probe, next to `quote-probe.py`

`quote-probe.py` measures the three styles a *diagnostic* can be made to
print, using `sort` and `head` as instruments, because those are the styles a
utility picks between when it reports an error. `ls` picks between all ten,
and lets the caller say which:

    ls --quoting-style=literal|shell|shell-always|shell-escape|
                       shell-escape-always|c|c-maybe|escape|locale|clocale

so `ls` is the only instrument that reaches the other seven, and it reaches
the first three as well. Measuring all ten through one instrument is the
point: three of these rows are also measured by `quote-probe.py` through a
different program, and the two fixtures agreeing is what proves that
`quotef` really is `shell-escape` and not merely similar to it. Our
`quote::Style` dispatch assumes exactly that.

## The format, and why it is one row per name rather than one per answer

`quotearg-gnu.txt` spends a row on each (style, name) pair. Here a row is a
name and *all ten* answers:

    <name-hex> <literal> <shell> <shell-always> <shell-escape>
               <shell-escape-always> <c> <c-maybe> <escape> <locale> <clocale>

which is a third the size and, more usefully, is the form in which a
disagreement is legible: the styles that share a rendering sit next to each
other on the line, so a row where `shell` and `shell-escape` differ says
immediately which byte forced the escape.

## Why the file has to exist

`ls` renders a name in the requested style only when it *lists* it; a name it
cannot reach is reported with `quoteaf` instead, whatever `--quoting-style`
said. So each name is created as an empty file before it is measured. Three
kinds of name cannot be created and are handled by name:

  * the empty name, which is not a file name at all -- its ten renderings are
    asserted by hand in the test rather than measured;
  * `.` and `..`, which exist already and are measured without being created;
  * anything holding a `/`, which is a path rather than a name. The corpus
    holds none, and the filter is here so that it stays true.

A name holding a newline is measured on its own rather than in a batch: under
`literal`, `shell` and `shell-always` that byte is printed raw, so the output
of a batch could not be split back into one answer per name.
"""

import importlib.util
import os
import subprocess
import sys
import tempfile

# See `quote-probe.py`'s note on this: `C`, not `C.UTF-8`, would measure a
# locale SlateOS does not have, and would pin GNU's ASCII branch -- straight
# quotes for `locale`, and every valid multi-byte character escaped -- as if
# it were the only one.
ENV = dict(os.environ, LC_ALL="C.UTF-8")

STYLES = [
    "literal",
    "shell",
    "shell-always",
    "shell-escape",
    "shell-escape-always",
    "c",
    "c-maybe",
    "escape",
    "locale",
    "clocale",
]

# How many names go into one `ls`. Small enough that the command line stays
# far under `ARG_MAX` even when every name in the chunk is the 40-byte one.
CHUNK = 200


def load_corpus() -> list[bytes]:
    """The corpus `quote-probe.py` already defines, plus its byte sweeps.

    Imported rather than copied. Two probes with two corpora would drift, and
    the whole value of measuring both instruments is that they are asked the
    same questions.
    """
    here = os.path.dirname(os.path.abspath(__file__))
    path = os.path.join(here, "quote-probe.py")
    spec = importlib.util.spec_from_file_location("quote_probe", path)
    if spec is None or spec.loader is None:
        raise SystemExit("cannot load scripts/quote-probe.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)

    names = list(mod.corpus())
    # The same positional sweeps `quote-probe.py` runs. Position decides which
    # outer quote is chosen, and for `escape` -- which has no outer quote --
    # it decides nothing, which is itself a fact worth recording.
    shapes = (b"%c", b"a%cz", b"%cz", b"a%c",
              b"a'z%c", b"%ca'z", b"a%c'z", b"'%c", b"%c'")
    for b in range(1, 256):
        if b == 0x2F:  # `/` is a separator, not a name byte
            continue
        for shape in shapes:
            names.append(shape.replace(b"%c", bytes([b])))
    for seq in [s.encode() for s in mod.MULTIBYTE] + mod.UNDECODABLE:
        for shape in shapes:
            names.append(shape.replace(b"%c", seq))

    seen, out = set(), []
    for n in names:
        if n in seen or not n or b"/" in n:
            continue
        seen.add(n)
        out.append(n)
    return out


def run_ls(style: str, names: list[bytes]) -> bytes:
    """`ls`'s whole answer for `names`, with the final newline removed.

    `-U` is what makes the order the operand order rather than sorted, which
    is what lets an answer be matched back to the name that produced it. `-d`
    keeps a name that happens to be a directory -- `.` and `..` are in the
    corpus -- from being listed through.
    """
    argv = [b"ls", b"-U", b"-1", b"-d", b"--quoting-style=" + style.encode(), b"--"]
    r = subprocess.run(argv + names, capture_output=True, env=ENV,
                       stdin=subprocess.DEVNULL)
    if r.stderr:
        raise SystemExit("ls --quoting-style=%s complained: %s"
                         % (style, r.stderr.decode("utf-8", "replace")))
    out = r.stdout
    return out[:-1] if out.endswith(b"\n") else out


def render(style: str, names: list[bytes]) -> list[bytes]:
    """One answer per name, for names that hold no newline.

    Splitting on the separator is only sound because of that restriction --
    see [`measure`], which routes the rest through [`run_ls`] one at a time.
    """
    answers = run_ls(style, names).split(b"\n")
    if len(answers) != len(names):
        raise SystemExit(
            "ls --quoting-style=%s answered %d names with %d lines"
            % (style, len(names), len(answers)))
    return answers


def measure(names: list[bytes]) -> list[tuple[bytes, list[bytes]]]:
    """Every name's ten renderings.

    Names holding a newline are measured one at a time; see the module
    docstring for why a batch cannot be split back apart when the style
    prints that byte raw.
    """
    plain = [n for n in names if b"\n" not in n]
    lonely = [n for n in names if b"\n" in n]

    answers: dict[bytes, list[bytes]] = {n: [] for n in names}
    for style in STYLES:
        for i in range(0, len(plain), CHUNK):
            chunk = plain[i:i + CHUNK]
            for name, got in zip(chunk, render(style, chunk)):
                answers[name].append(got)
        for name in lonely:
            # Not `render`: the answer may itself span lines, so there is
            # nothing to split and nothing to count.
            answers[name].append(run_ls(style, [name]))
    return [(n, answers[n]) for n in names]


def main() -> int:
    dest = sys.argv[1] if len(sys.argv) > 1 else "-"
    if dest != "-":
        dest = os.path.abspath(dest)
    names = load_corpus()

    os.chdir(tempfile.mkdtemp(prefix="ls-quote-probe-"))
    for name in names:
        if name in (b".", b".."):
            continue
        with open(name, "wb"):
            pass

    rows = measure(names)

    version = subprocess.run(["ls", "--version"], capture_output=True,
                             text=True).stdout.split("\n")[0]
    lines = [
        "# GNU coreutils quoting, all ten styles, measured. Do not hand-edit.",
        "# Produced by scripts/ls-quote-probe.py under:",
        "#   " + version,
        "# Each row is <name-hex> then one hex field per style, in order:",
        "#   " + " ".join(STYLES),
        "# LC_ALL=C.UTF-8.",
    ]
    for name, got in rows:
        lines.append(" ".join([name.hex()] + [g.hex() for g in got]))

    text = "\n".join(lines) + "\n"
    if dest == "-":
        sys.stdout.write(text)
    else:
        with open(dest, "w", encoding="ascii", newline="\n") as f:
            f.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
