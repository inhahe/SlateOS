#!/usr/bin/env python3
"""Ad-hoc measurement of GNU `paste`. Not part of the build; kept only so the
rows quoted in `paste.rs`'s documentation can be re-derived."""

import os
import subprocess
import sys
import tempfile

ENV = {"LC_ALL": "C", "LANG": "C", "PATH": "/usr/bin:/bin"}

FIXTURES = {
    "A": b"a1\na2\na3\n",
    "B": b"b1\nb2\n",
    "C": b"c1\n",
    "E": b"",
    "U": b"x1\nx2",
}


def run(args, stdin=b""):
    return subprocess.run(
        [b"paste"] + args, input=stdin, capture_output=True, env=ENV
    )


def show(label, args, stdin=b""):
    p = run(args, stdin)
    print(f"$ paste {' '.join(a.decode('latin1') for a in args)}   # {label}")
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
        cases = [
            ("no operand, stdin", [], b"p\nq\n"),
            ("dash operand", [b"-"], b"p\nq\n"),
            ("two dashes", [b"-", b"-"], b"p\nq\nr\n"),
            ("mixed dash", [b"A", b"-"], b"p\nq\n"),
            ("missing -d argument", [b"-d"]),
            ("missing --delimiters argument", [b"--delimiters"]),
            ("--serial takes no argument", [b"--serial=x", b"A"]),
            ("unknown short", [b"-Q", b"A"]),
            ("unknown long", [b"--nope", b"A"]),
            ("ambiguous empty prefix", [b"--=x", b"A"]),
            ("abbreviations", [b"--s", b"--d", b",", b"A", b"B"]),
            ("trailing backslash", [b"-d", b"a\\", b"A"]),
            ("trailing backslash preempts missing file", [b"-d", b"\\", b"nosuch"]),
            ("even backslashes are fine", [b"-d", b"a\\\\", b"A", b"B"]),
            ("odd run of three", [b"-d", b"a\\\\\\", b"A"]),
            ("backslash escapes", [b"-d", b"\\b\\f\\n\\r\\t\\v\\\\\\q", b"A", b"B"]),
            ("parallel missing file is fatal", [b"A", b"nosuch", b"B"]),
            ("parallel missing file, two", [b"nosuch", b"nosuch2"]),
            ("serial missing file continues", [b"-s", b"nosuch", b"A"]),
            ("serial missing files, two", [b"-s", b"nosuch", b"nosuch2"]),
            ("directory operand parallel", [b"."]),
            ("directory operand serial", [b"-s", b"."]),
            ("-d last wins", [b"-d", b",", b"-d", b":", b"A", b"B"]),
            ("-d empty", [b"-d", b"", b"A", b"B"]),
            ("-d empty serial", [b"-s", b"-d", b"", b"A"]),
            ("-d single char cycles", [b"-d", b",;", b"A", b"B", b"C"]),
            ("unterminated last file", [b"U"]),
            ("unterminated first of two", [b"U", b"A"]),
            ("serial unterminated", [b"-s", b"U"]),
            ("serial empty", [b"-s", b"E"]),
            ("serial empty then A", [b"-s", b"E", b"A"]),
            ("empty operand parallel", [b"E"]),
            ("-z parallel", [b"-z", b"A", b"B"]),
            ("-z serial", [b"-sz", b"A", b"B"]),
            ("-z unterminated", [b"-z", b"U"]),
            ("-z serial unterminated", [b"-zs", b"U"]),
            ("-z empty serial", [b"-zs", b"E"]),
            ("-- ends options", [b"--", b"-d"]),
            ("operand named -d after --", [b"-s", b"--", b"A"]),
        ]
        for case in cases:
            show(case[0], case[1], case[2] if len(case) > 2 else b"")
            print()


main()
