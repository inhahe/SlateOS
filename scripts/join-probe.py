#!/usr/bin/env python3
"""Ad-hoc measurement of GNU `join`. Not part of the build; kept only so the
rows quoted in `join.rs`'s documentation can be re-derived."""

import os
import subprocess
import tempfile

ENV = {"LC_ALL": "C", "LANG": "C", "PATH": "/usr/bin:/bin"}

FIXTURES = {
    # The canonical pair: one key only on the left, one only on the right, two
    # shared, and a shared key with two lines on one side to force the product.
    "A": b"a 1\nb 2\nd 4\n",
    "B": b"b x\nc y\nd z\n",
    # Repeated keys on both sides, in unequal numbers.
    "M": b"k a\nk b\nm c\n",
    "N": b"k p\nk q\nk r\n",
    # Three fields, so -1/-2/-o have something to choose between.
    "T": b"1 one uno\n2 two dos\n3 three tres\n",
    "U": b"2 II\n3 III\n4 IV\n",
    # Tab separated.
    "C": b"a\t1\nb\t2\n",
    "D": b"a\tx\nb\ty\n",
    # Colon separated, for -t.
    "P": b"a:1:i\nb:2:ii\n",
    "Q": b"a:x\nc:z\n",
    # Ragged: lines with different field counts, for -o auto and -e.
    "R": b"a 1 2 3\nb\nc 9\n",
    "S": b"a x\nb y y y\nc\n",
    # Leading and repeated blanks, which the default splitter folds.
    "W": b"   a   1  2\nb\t\t3\n",
    "X": b"a  9\nb 8\n",
    # Empty, unterminated, and out of order.
    "E": b"",
    "V": b"a 1\nz 9",
    "Z": b"c 3\na 1\nb 2\n",
    "Y": b"a 1\nb 2\nc 3\n",
    # Case differing only in case, for -i.
    "I": b"A 1\nb 2\n",
    "J": b"a x\nB y\n",
    # NUL separated, for -z.
    "G": b"a 1\0b 2\0",
    "H": b"a x\0c z\0",
    # A key that is the empty field (line begins with the separator under -t).
    "K": b":1\na:2\n",
    "L": b":x\na:y\n",
}


def run(args, stdin=b""):
    return subprocess.run(
        [b"join"] + args, input=stdin, capture_output=True, env=ENV
    )


def show(label, args, stdin=b""):
    p = run(args, stdin)
    print(f"$ join {' '.join(a.decode('latin1') for a in args)}   # {label}")
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
            # --- the merge itself ---
            ("plain", [b"A", b"B"]),
            ("plain reversed", [b"B", b"A"]),
            ("cartesian product", [b"M", b"N"]),
            ("product reversed", [b"N", b"M"]),
            ("no keys in common", [b"A", b"K"]),
            ("self", [b"A", b"A"]),
            ("empty right", [b"A", b"E"]),
            ("empty left", [b"E", b"B"]),
            ("both empty", [b"E", b"E"]),
            ("unterminated", [b"V", b"Y"]),
            ("ragged", [b"R", b"S"]),
            # --- -a and -v ---
            ("-a1", [b"-a1", b"A", b"B"]),
            ("-a2", [b"-a2", b"A", b"B"]),
            ("-a1 -a2", [b"-a1", b"-a2", b"A", b"B"]),
            ("-a 1 separated", [b"-a", b"1", b"A", b"B"]),
            ("-v1", [b"-v1", b"A", b"B"]),
            ("-v2", [b"-v2", b"A", b"B"]),
            ("-v1 -v2", [b"-v1", b"-v2", b"A", b"B"]),
            ("-a1 -v2", [b"-a1", b"-v2", b"A", b"B"]),
            ("-v1 -a2", [b"-v1", b"-a2", b"A", b"B"]),
            ("-a1 twice", [b"-a1", b"-a1", b"A", b"B"]),
            ("-a3 is invalid", [b"-a3", b"A", b"B"]),
            ("-a0 is invalid", [b"-a0", b"A", b"B"]),
            ("-a x is invalid", [b"-a", b"x", b"A", b"B"]),
            ("-a empty", [b"-a", b"", b"A", b"B"]),
            ("-a with sign", [b"-a", b"+1", b"A", b"B"]),
            ("-v3 is invalid", [b"-v3", b"A", b"B"]),
            ("-a1 with unpairable tail", [b"-a1", b"A", b"E"]),
            ("-a2 with unpairable tail", [b"-a2", b"E", b"B"]),
            ("-a1 -a2 ragged", [b"-a1", b"-a2", b"R", b"S"]),
            # --- -e, and the empty field ---
            ("-e with -a", [b"-e", b"NULL", b"-a1", b"-a2", b"A", b"B"]),
            ("-e with -o", [b"-e", b"-", b"-o", b"0,1.2,2.2", b"-a1", b"A", b"B"]),
            ("-e empty string", [b"-e", b"", b"-o", b"1.9", b"A", b"B"]),
            ("-e twice same", [b"-e", b"x", b"-e", b"x", b"A", b"B"]),
            ("-e twice different", [b"-e", b"x", b"-e", b"y", b"A", b"B"]),
            ("-e without -o or -a", [b"-e", b"NULL", b"R", b"S"]),
            ("-e with -o auto", [b"-e", b".", b"-o", b"auto", b"-a1", b"-a2", b"R", b"S"]),
            # --- -o ---
            ("-o 0", [b"-o", b"0", b"A", b"B"]),
            ("-o 1.1 2.2", [b"-o", b"1.1,2.2", b"A", b"B"]),
            ("-o blank separated", [b"-o", b"1.1 2.2", b"A", b"B"]),
            ("-o tab separated", [b"-o", b"1.1\t2.2", b"A", b"B"]),
            ("-o out of range", [b"-o", b"1.9", b"A", b"B"]),
            ("-o repeated field", [b"-o", b"1.1,1.1,1.1", b"A", b"B"]),
            ("-o 0 with -v1", [b"-o", b"0,2.2", b"-v1", b"A", b"B"]),
            ("-o 0 with -a2", [b"-o", b"0,1.2", b"-a2", b"A", b"B"]),
            ("-o auto", [b"-o", b"auto", b"R", b"S"]),
            ("-o auto with -a", [b"-o", b"auto", b"-a1", b"-a2", b"R", b"S"]),
            ("-o auto empty first", [b"-o", b"auto", b"E", b"S"]),
            ("-o twice accumulates", [b"-o", b"1.1", b"-o", b"2.2", b"A", b"B"]),
            ("-o 0.1 is invalid", [b"-o", b"0.1", b"A", b"B"]),
            ("-o 3.1 is invalid", [b"-o", b"3.1", b"A", b"B"]),
            ("-o 1 without dot", [b"-o", b"1", b"A", b"B"]),
            ("-o 1. is invalid", [b"-o", b"1.", b"A", b"B"]),
            ("-o 1.0 is invalid", [b"-o", b"1.0", b"A", b"B"]),
            ("-o 1.x is invalid", [b"-o", b"1.x", b"A", b"B"]),
            ("-o empty", [b"-o", b"", b"A", b"B"]),
            ("-o auto uppercase", [b"-o", b"AUTO", b"A", b"B"]),
            ("-o huge field", [b"-o", b"1.99999999999999999999", b"A", b"B"]),
            # --- -o swallows following operands (obsolescent) ---
            ("-o then three operands", [b"-o", b"1.1", b"2.2", b"A", b"B"]),
            ("-o then four operands", [b"-o", b"1.1", b"2.2", b"0", b"A", b"B"]),
            ("-o then bad extra", [b"-o", b"1.1", b"9.9", b"A", b"B"]),
            ("-o auto then operands", [b"-o", b"auto", b"A", b"B"]),
            ("extra operand after plain", [b"A", b"B", b"E"]),
            ("extra operand four", [b"A", b"B", b"E", b"Y"]),
            # --- -1 -2 -j and the -j1/-j2 ambiguity ---
            ("-1 2", [b"-1", b"2", b"T", b"U"]),
            ("-2 1", [b"-2", b"1", b"T", b"U"]),
            ("-1 2 -2 1", [b"-1", b"2", b"-2", b"1", b"T", b"U"]),
            ("-j 2", [b"-j", b"2", b"T", b"T"]),
            ("-j1 attached", [b"-j1", b"A", b"B"]),
            ("-j2 attached", [b"-j2", b"T", b"U"]),
            ("-j1 with three operands", [b"-j1", b"2", b"T", b"U"]),
            ("-j2 with three operands", [b"-j2", b"2", b"T", b"U"]),
            ("-j3 attached", [b"-j3", b"A", b"B"]),
            ("-j 1 separated", [b"-j", b"1", b"A", b"B"]),
            ("-j0 is invalid", [b"-j", b"0", b"A", b"B"]),
            ("-j x is invalid", [b"-j", b"x", b"A", b"B"]),
            ("-1 0 is invalid", [b"-1", b"0", b"A", b"B"]),
            ("-1 -1 is invalid", [b"-1", b"-1", b"A", b"B"]),
            ("-1 x is invalid", [b"-1", b"x", b"A", b"B"]),
            ("-1 empty", [b"-1", b"", b"A", b"B"]),
            ("-1 huge", [b"-1", b"99999999999999999999", b"A", b"B"]),
            ("incompatible -1", [b"-1", b"1", b"-1", b"2", b"A", b"B"]),
            ("compatible -1 twice", [b"-1", b"2", b"-1", b"2", b"T", b"U"]),
            ("-j then -1 incompatible", [b"-j", b"1", b"-1", b"2", b"A", b"B"]),
            ("-1 then -j incompatible", [b"-1", b"2", b"-j", b"1", b"A", b"B"]),
            ("-j after -2 compatible", [b"-2", b"1", b"-j", b"1", b"A", b"B"]),
            ("-j1 with missing operand", [b"-j1", b"A"]),
            # --- -t ---
            ("-t colon", [b"-t", b":", b"P", b"Q"]),
            ("-t colon attached", [b"-t:", b"P", b"Q"]),
            ("-t tab", [b"-t", b"\t", b"C", b"D"]),
            ("-t empty means whole line", [b"-t", b"", b"Y", b"Y"]),
            ("-t backslash zero", [b"-t", b"\\0", b"A", b"B"]),
            ("-t multi-char", [b"-t", b"ab", b"A", b"B"]),
            ("-t multi-char backslash", [b"-t", b"\\n", b"A", b"B"]),
            ("-t twice same", [b"-t", b":", b"-t", b":", b"P", b"Q"]),
            ("-t twice different", [b"-t", b":", b"-t", b",", b"P", b"Q"]),
            ("-t empty twice", [b"-t", b"", b"-t", b"", b"Y", b"Y"]),
            ("-t empty then colon", [b"-t", b"", b"-t", b":", b"P", b"Q"]),
            ("-t newline", [b"-t", b"\n", b"Y", b"Y"]),
            ("-t colon empty key", [b"-t", b":", b"K", b"L"]),
            ("-t colon with -a", [b"-t", b":", b"-a1", b"-a2", b"P", b"Q"]),
            ("default folds blanks", [b"W", b"X"]),
            ("-t space", [b"-t", b" ", b"W", b"X"]),
            # --- -i ---
            ("-i", [b"-i", b"I", b"J"]),
            ("without -i", [b"I", b"J"]),
            ("--ignore-case", [b"--ignore-case", b"I", b"J"]),
            # --- --header ---
            ("--header", [b"--header", b"T", b"U"]),
            ("--header with -a", [b"--header", b"-a1", b"-a2", b"A", b"B"]),
            ("--header empty left", [b"--header", b"E", b"B"]),
            ("--header both empty", [b"--header", b"E", b"E"]),
            ("--header with -o", [b"--header", b"-o", b"1.1,2.2", b"T", b"U"]),
            ("--header disordered", [b"--header", b"Z", b"Y"]),
            # --- -z ---
            ("-z", [b"-z", b"G", b"H"]),
            ("-z -a1 -a2", [b"-z", b"-a1", b"-a2", b"G", b"H"]),
            ("--zero-terminated", [b"--zero-terminated", b"G", b"H"]),
            ("-z on newline data", [b"-z", b"A", b"B"]),
            # --- order checking ---
            ("disorder default", [b"Z", b"Y"]),
            ("disorder default reversed", [b"Y", b"Z"]),
            ("disorder both", [b"Z", b"Z"]),
            ("disorder check-order", [b"--check-order", b"Z", b"Y"]),
            ("disorder nocheck-order", [b"--nocheck-order", b"Z", b"Y"]),
            ("check-order on sorted", [b"--check-order", b"A", b"B"]),
            ("check-order both disordered", [b"--check-order", b"Z", b"Z"]),
            ("last one wins", [b"--check-order", b"--nocheck-order", b"Z", b"Y"]),
            ("last one wins other way", [b"--nocheck-order", b"--check-order", b"Z", b"Y"]),
            ("disorder with -a1", [b"-a1", b"Z", b"Y"]),
            ("disorder with -v1", [b"-v1", b"Z", b"Y"]),
            ("abbrev check", [b"--check", b"Z", b"Y"]),
            ("abbrev noc", [b"--noc", b"Z", b"Y"]),
            ("abbrev header", [b"--head", b"T", b"U"]),
            # --- operands ---
            ("missing operand none", []),
            ("missing operand one", [b"A"]),
            ("missing operand after option", [b"-i", b"A"]),
            ("nonexistent first", [b"nosuch", b"nosuch2"]),
            ("nonexistent second", [b"A", b"nosuch"]),
            ("directory operand", [b".", b"A"]),
            ("stdin dash", [b"-", b"B"], b"a 1\nb 2\nd 4\n"),
            ("stdin dash second", [b"A", b"-"], b"b x\nc y\nd z\n"),
            ("two dashes", [b"-", b"-"], b"a 1\n"),
            # --- getopt ---
            ("unknown short", [b"-Q", b"A", b"B"]),
            ("unknown long", [b"--nope", b"A", b"B"]),
            ("ambiguous empty", [b"--=x", b"A", b"B"]),
            ("header takes no arg", [b"--header=x", b"A", b"B"]),
            ("-a needs arg", [b"-a"]),
            ("-o needs arg", [b"-o"]),
            ("-t needs arg", [b"-t"]),
            ("-- ends options", [b"--", b"A", b"B"]),
            ("-- then dash operand", [b"--", b"-", b"B"], b"a 1\n"),
            ("option after operand", [b"A", b"-a1", b"B"]),
            ("option after two operands", [b"A", b"B", b"-a1"]),
        ]
        for case in cases:
            show(case[0], case[1], case[2] if len(case) > 2 else b"")
            print()


main()
