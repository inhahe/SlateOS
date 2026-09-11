"""One spelling rule for `--self-test`, because the alternative reported success.

A checker in this tree has two jobs and one of them is checking itself: the
`--check` scan asks whether the tree still obeys a rule, and the self-test asks
whether the checker still recognises the rule at all. The second is what makes
the first trustworthy, and it is invoked by hand far more often than by the
hook -- which is exactly when a spelling gets mistyped.

WHAT THIS EXISTS TO PREVENT, measured 2026-09-10 across `scripts/`:

    python scripts/rustscan.py --selftest              exit 0, ran nothing
    python scripts/check-recursive-locks.py --selftest exit 0, scanned 807 files
    ... and twelve more checkers with the same shape

Sixteen scripts took `"--self-test" in argv` literally. A `--selftest` missed
the branch and fell through to the script's default action -- a library notice,
or the real scan -- and exited 0. So the command that asks "is this checker
still correct?" answered yes without asking, and the answer was
indistinguishable from a genuine pass. Lane A spent five minutes reading a
green terminal while the boot test's log disagreed, and the two of us had
already spent a day on inert gates that exited 0 having decided nothing. Same
defect: SUCCESS AND NOT-RUNNING MUST NOT BE THE SAME OBSERVATION.

The rule is therefore not "accept both spellings" but the stronger one below.

    An option this script does not recognise is an ERROR, never a
    fall-through to the default action.

That is the half that generalises. The next trap will not be a spelling of
self-test; it will be `--dry-run` on a script that only knows `--dry`, and the
run will do the real thing and report success. `unknown_options` is here so a
script can refuse that in one line.

`scripts/check-selftest-flag-spellings.py` enforces both halves.
"""

# `--self_test` is included because the file is `check_x.py` in some sibling
# projects and the underscore is a natural slip, not because anything uses it.
SPELLINGS = ("--self-test", "--selftest", "--self_test")


def wants_selftest(argv) -> bool:
    """True if `argv` asks for the self-test, in any accepted spelling.

    Pass the argument list WITHOUT the program name (`sys.argv[1:]`) or with it
    (`sys.argv`); a program name never collides with an option spelling, so
    both are safe. Callers in this tree pass all three forms.
    """
    return any(a in SPELLINGS for a in argv)


def unknown_options(argv, known=()) -> list:
    """The options in `argv` that neither this module nor `known` recognises.

    Only tokens starting with `-` are considered, so file operands and values
    are left alone. A caller that takes `-` itself (stdin) should list it in
    `known`.

    Returns a list rather than a bool so the caller can name the offending
    option in its message: "unrecognised option '--selftets'" is actionable and
    "bad arguments" is not.
    """
    allowed = set(SPELLINGS) | set(known)
    return [a for a in argv if a.startswith("-") and a not in allowed]


def _self_test() -> int:
    failures = 0

    def check(got, want, what):
        nonlocal failures
        if got != want:
            print(f"FAIL {what}\n  got  {got!r}\n  want {want!r}")
            failures += 1
        else:
            print(f"  ok    {what}")

    check(wants_selftest(["--self-test"]), True, "the hyphenated spelling")
    check(wants_selftest(["--selftest"]), True, "the unhyphenated spelling")
    check(wants_selftest(["--self_test"]), True, "the underscored spelling")
    check(wants_selftest([]), False, "no arguments is not a self-test request")
    check(wants_selftest(["--check"]), False, "another option is not one either")
    # The program name is present in `sys.argv` and absent in `sys.argv[1:]`;
    # callers in this tree pass both, so neither may change the answer.
    check(
        wants_selftest(["scripts/x.py", "--selftest"]),
        True,
        "a leading program name does not hide the flag",
    )
    check(
        wants_selftest(["scripts/self-test.py"]),
        False,
        "a path that merely contains the word is not the flag",
    )

    check(unknown_options(["--selftest"]), [], "an accepted spelling is not unknown")
    check(unknown_options(["--bogus"]), ["--bogus"], "an unknown option is reported")
    check(
        unknown_options(["--bogus", "--worse"]),
        ["--bogus", "--worse"],
        "every unknown option is reported, not just the first",
    )
    check(unknown_options(["file.rs"]), [], "an operand is not an option")
    check(unknown_options(["--check"], known=("--check",)), [], "`known` is honoured")
    check(
        unknown_options(["-"], known=("-",)),
        [],
        "a bare dash can be declared as stdin",
    )
    # The point of the module: the flag it does not know must not be silently
    # dropped, which is what every fixed caller was doing.
    check(
        unknown_options(["--selftets"]),
        ["--selftets"],
        "A TYPO OF THE FLAG IS UNKNOWN, not a request",
    )

    if failures:
        print(f"\nselftestflag: {failures} self-test failure(s)")
        return 1
    print("selftestflag: self-test passed (0 failure(s))")
    return 0


if __name__ == "__main__":
    import sys

    unknown = unknown_options(sys.argv[1:])
    if unknown:
        print(f"selftestflag.py: unrecognised option {unknown[0]!r}", file=sys.stderr)
        sys.exit(2)
    sys.exit(_self_test())
