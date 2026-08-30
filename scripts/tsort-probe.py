#!/usr/bin/env python3
"""Ad-hoc measurement of GNU `tsort`. Not part of the build; kept only so the
rows quoted in `tsort.rs`'s documentation can be re-derived.

Run it inside WSL (or on any glibc host):

    wsl -e python3 scripts/tsort-probe.py

Standard output and standard error are shown separately and in that order,
because for `tsort` they are not two views of one run: a cyclic input names the
cycle's members on *stderr* while still listing every item on *stdout*, and a
probe that merged the two would show them interleaved by whichever stream was
flushed first rather than by what the program decided.
"""

import os
import subprocess
import tempfile

ENV = {"LC_ALL": "C", "LANG": "C", "PATH": "/usr/bin:/bin"}

FIXTURES = {
    # `a` precedes three items and none of them precedes another, so the only
    # thing that can order b, c and d is the order the relations were read.
    "SUCC": b"a c\na b\na d\n",
    # Two ready items at once: the name order decides.
    "DIAMOND": b"a b\na c\nb d\nc d\n",
    # The smallest loop, and a loop beside an acyclic part.
    "CYC2": b"a b\nb a\n",
    "CYC3": b"a b\nb c\nc a\nx y\n",
    # Written backwards, so the backward walk names it in an order that is
    # neither the input's nor sorted.
    "CYCREV": b"b a\nc b\na c\nd e\n",
    # Two independent loops: the outer loop runs twice and reports twice.
    "CYC2X": b"a b\nb a\nc d\nd c\n",
    # `\r`, `\v` and `\f` are not delimiters, so these are two tokens each.
    "CR": b"a\rb x\n",
    "VTFF": b"a\vb a\fc\n",
    # A NUL truncates the token; a token that is only a NUL is the empty name.
    "NUL": b"a\0b c\n",
    "NULONLY": b"\0 x\ny \0\n",
    # Case and high bytes, to show the sort is over `unsigned char`.
    "CASE": b"B q\na q\nA q\nb q\n",
    "HIGH": b"\xff q\n\x01 q\n\x80 q\n",
    # An odd token count.
    "ODD": b"a b c\n",
}


def run(args, stdin=b""):
    return subprocess.run(
        [b"tsort"] + args, input=stdin, capture_output=True, env=ENV
    )


def show(label, args, stdin=b""):
    p = run(args, stdin)
    print(f"$ tsort {' '.join(a.decode('latin1') for a in args)}   # {label}")
    print(f"    status {p.returncode}")
    if p.stdout:
        print(f"    out {p.stdout!r}")
    if p.stderr:
        print(f"    err {p.stderr!r}")


def main():
    with tempfile.TemporaryDirectory() as d:
        os.chdir(d)
        for name, body in FIXTURES.items():
            with open(name, "wb") as f:
                f.write(body)
        os.mkdir("sub")
        cases = [
            ("successors are walked newest-first", [b"SUCC"]),
            ("ready items enter the queue in name order", [b"DIAMOND"]),
            ("the sort is bytewise", [b"CASE"]),
            ("high bytes sort as unsigned", [b"HIGH"]),
            ("a loop is stderr; stdout still lists everything", [b"CYC2"]),
            ("a loop beside an acyclic part", [b"CYC3"]),
            ("the backward walk's order", [b"CYCREV"]),
            ("two loops, two reports", [b"CYC2X"]),
            ("carriage return is content, so this is an even count", [b"CR"]),
            ("vertical tab and form feed likewise", [b"VTFF"]),
            ("a token stops at its first NUL", [b"NUL"]),
            ("a token that is only a NUL", [b"NULONLY"]),
            ("odd token count", [b"ODD"]),
            ("odd token count on stdin is named '-'", [], b"a b c\n"),
            ("no operand reads stdin", [], b"a b\n"),
            ("a lone dash reads stdin", [b"-"], b"a b\n"),
            ("empty stdin", [], b""),
            # The option parser: one getopt_long call, no short options,
            # permutation on, and everything reaching the operand check is an
            # operand.
            ("the whole long-option table, in declaration order", [b"--=x"]),
            ("there is no -h", [b"-h"]),
            ("nor any other short option", [b"-x"]),
            ("a cluster is blamed on its first byte", [b"-qz"]),
            ("unrecognized long", [b"--nope"]),
            ("--help takes no argument", [b"--help=x"]),
            ("an abbreviation is named by its resolution", [b"--hel=x"]),
            ("options permute past an operand", [b"SUCC", b"--version"]),
            ("-- ends the options", [b"--", b"SUCC"]),
            ("-- alone leaves stdin", [b"--"], b"a b\n"),
            ("after --, an option spelling is a file name", [b"--", b"--help"]),
            ("the second operand is the one blamed", [b"SUCC", b"CYC2", b"ODD"]),
            ("an operand that will not open", [b"nosuch"]),
            ("an operand that opens and will not read", [b"sub"]),
        ]
        for case in cases:
            show(case[0], case[1], case[2] if len(case) > 2 else b"")
            print()


main()
