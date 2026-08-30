#!/usr/bin/env python3
"""Ad-hoc measurement of GNU `comm`. Not part of the build; kept only so the
rows quoted in `comm.rs`'s documentation can be re-derived."""

import os
import subprocess
import tempfile

ENV = {"LC_ALL": "C", "LANG": "C", "PATH": "/usr/bin:/bin"}

FIXTURES = {
    "A": b"a\nb\nd\n",
    "B": b"b\nc\nd\n",
    "E": b"",
    "U": b"a\nz",
    "D": b"c\na\nb\n",
    "S": b"a\nb\nc\n",
}


def run(args, stdin=b""):
    return subprocess.run(
        [b"comm"] + args, input=stdin, capture_output=True, env=ENV
    )


def show(label, args, stdin=b""):
    p = run(args, stdin)
    print(f"$ comm {' '.join(a.decode('latin1') for a in args)}   # {label}")
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
            ("plain", [b"A", b"B"]),
            ("-1", [b"-1", b"A", b"B"]),
            ("-12", [b"-12", b"A", b"B"]),
            ("-123", [b"-123", b"A", b"B"]),
            ("empty second", [b"A", b"E"]),
            ("empty first", [b"E", b"B"]),
            ("both empty", [b"E", b"E"]),
            ("unterminated", [b"U", b"S"]),
            ("total", [b"--total", b"A", b"B"]),
            ("total -12", [b"--total", b"-12", b"A", b"B"]),
            ("total empty", [b"--total", b"E", b"E"]),
            ("output-delimiter", [b"--output-delimiter=::", b"A", b"B"]),
            ("output-delimiter empty", [b"--output-delimiter=", b"A", b"B"]),
            ("output-delimiter empty total", [b"--output-delimiter=", b"--total", b"A", b"B"]),
            ("output-delimiter twice same", [b"--output-delimiter=:", b"--output-delimiter=:", b"A", b"B"]),
            ("output-delimiter twice diff", [b"--output-delimiter=:", b"--output-delimiter=;", b"A", b"B"]),
            ("output-delimiter empty then empty", [b"--output-delimiter=", b"--output-delimiter=", b"A", b"B"]),
            ("output-delimiter multi total", [b"--output-delimiter=XY", b"--total", b"A", b"B"]),
            ("-z", [b"-z", b"A", b"B"]),
            ("-z total", [b"-z", b"--total", b"A", b"B"]),
            ("disorder default", [b"D", b"S"]),
            ("disorder default reversed", [b"S", b"D"]),
            ("disorder check-order", [b"--check-order", b"D", b"S"]),
            ("disorder nocheck-order", [b"--nocheck-order", b"D", b"S"]),
            ("disorder but pairable", [b"D", b"D"]),
            ("disorder pairable check-order", [b"--check-order", b"D", b"D"]),
            ("missing operand none", []),
            ("missing operand one", [b"A"]),
            ("missing operand after option", [b"-z", b"A"]),
            ("extra operand", [b"A", b"B", b"E"]),
            ("extra operand four", [b"A", b"B", b"E", b"S"]),
            ("nonexistent first", [b"nosuch", b"nosuch2"]),
            ("nonexistent second", [b"A", b"nosuch"]),
            ("directory operand", [b".", b"A"]),
            ("stdin dash", [b"-", b"A"], b"a\nb\nd\n"),
            ("two dashes", [b"-", b"-"], b"a\nb\n"),
            ("unknown short", [b"-Q", b"A", b"B"]),
            ("unknown long", [b"--nope", b"A", b"B"]),
            ("ambiguous empty", [b"--=x", b"A", b"B"]),
            ("total takes no arg", [b"--total=x", b"A", b"B"]),
            ("output-delimiter needs arg", [b"--output-delimiter"]),
            ("abbrev", [b"--tot", b"--out", b":", b"A", b"B"]),
            ("abbrev check", [b"--check", b"D", b"S"]),
            ("abbrev noc", [b"--noc", b"D", b"S"]),
            ("-- ends options", [b"--", b"A", b"B"]),
            ("digit zero", [b"-0", b"A", b"B"]),
            ("digit four", [b"-4", b"A", b"B"]),
        ]
        for case in cases:
            show(case[0], case[1], case[2] if len(case) > 2 else b"")
            print()


main()
