"""A script's self-test must run under every spelling of the flag, or refuse.

WHAT THIS CAUGHT WHEN IT WAS WRITTEN (2026-09-10): sixteen scripts in which
`--selftest` and `--self-test` did DIFFERENT THINGS and both exited 0.

    python scripts/rustscan.py --selftest              ran nothing
    python scripts/check-recursive-locks.py --selftest scanned 807 files
    python scripts/check-option-refusal.py --selftest  ran the real scan
    ... and thirteen more

Each took one spelling literally -- `if "--self-test" in argv:` -- so the other
missed the branch and fell through to the script's default action. The default
action of a checker is its scan, which passes, so the mistyped command printed
a success line and exited 0. Lane A read one of those and spent five minutes
believing a file was clean while the boot test's log said it was not.

THE RULE IS NOT "BE FORGIVING ABOUT SPELLING". It is that a self-test's exit
status must mean the self-test ran. A checker earns its authority by testing
itself; a self-test that can silently not happen returns that authority to
nobody, and the failure is invisible precisely because the output looks right.
This is the same defect as a gate that sets `fail=1` and never reads it, and as
a read that turns an unreadable file into an empty one: SUCCESS AND
NOT-RUNNING MUST NOT BE THE SAME OBSERVATION.

# What is required

A script with a self-test mode must do one of:

  * call `selftestflag.wants_selftest(...)`, which accepts every spelling; or
  * declare every spelling in one `add_argument(...)` call.

and must not test flag membership by hand (`"--self-test" in argv`), because
that is the construct that admits exactly one spelling.

# What this deliberately does not check

That the script REFUSES an unrecognised option. That is the stronger and more
general rule -- `selftestflag.unknown_options` exists for it, and the next trap
of this kind will be `--dry-run` on a script that only knows `--dry`, not a
spelling of self-test. It is not enforced here because detecting "falls through
to the default action" statically means understanding each script's argument
handling, and a gate that can only be approximated is a gate that is argued
with. Running every script twice to compare would take minutes and this runs in
under a second. Recorded as the known gap rather than half-enforced.

    python scripts/check-selftest-flag-spellings.py --selftest
    python scripts/check-selftest-flag-spellings.py
"""

import pathlib
import re
import sys

import selftestflag

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCRIPTS = ROOT / "scripts"

# The two spellings a person actually types. `selftestflag` also accepts
# `--self_test`, but REQUIRING it of every add_argument call produced eight
# findings against scripts that already handle the real hyphen-vs-none trap,
# and a gate that manufactures work is one that gets bypassed -- which this
# file's own docstring warns about. The rule is the trap, not the vocabulary.
REQUIRED = ("--self-test", "--selftest")

# A hand-rolled membership test: the construct that admits one spelling.
_MEMBERSHIP = re.compile(r'"(--self[-_]?test)"\s+in\s')
# An `add_argument(...)` call naming at least one spelling.
#
# THE QUOTES ARE LOAD-BEARING. Without them this pattern matched ITS OWN
# SOURCE -- the line below contains `add_argument\(` and `--self[-_]?test`, so
# the checker reported itself as declaring one spelling. A real declaration
# quotes the flag and a regex source does not, which is the whole difference.
_ADD_ARG = re.compile(r'add_argument\(([^)]*"--self[-_]?test"[^)]*)\)', re.S)


def verdict(src: str) -> str:
    """One of: `ok`, `no-selftest`, or a sentence naming what is wrong.

    THE QUESTION IS "CAN BOTH SPELLINGS REACH THE SELF-TEST?", not "is a
    forbidden construct present". The first version of this asked the second,
    and four of its first five findings were wrong: `check-libc-shape.py` reads

        if "--self-test" in sys.argv[1:] or "--selftest" in sys.argv[1:]:

    which handles both spellings correctly using the construct the rule
    banned. Banning a construct outlaws the correct uses along with the
    incorrect ones; asking what the caller can actually reach does not. The
    empirical probe -- run it twice and compare -- is what caught this, and a
    static gate that disagrees with the behaviour is wrong by definition.
    """
    if "wants_selftest(" in src:
        return "ok"

    # Every spelling the file makes reachable, by whichever construct.
    reachable = {m.group(1) for m in _MEMBERSHIP.finditer(src)}
    declared = set()
    for call in _ADD_ARG.findall(src):
        declared |= {s for s in REQUIRED if f'"{s}"' in call}
    reachable |= declared

    if not reachable:
        return "no-selftest"

    missing = [s for s in REQUIRED if s not in reachable]
    if not missing:
        return "ok"
    return (
        f"reaches its self-test only via {', '.join(sorted(reachable))}; "
        f"{', '.join(missing)} falls through to the default action and exits 0 "
        f"without testing anything"
    )


def scan() -> int:
    findings = []
    checked = 0
    for path in sorted(SCRIPTS.glob("*.py")):
        src = path.read_text(encoding="utf-8", errors="surrogateescape")
        v = verdict(src)
        if v == "no-selftest":
            continue
        checked += 1
        if v != "ok":
            findings.append((path.name, v))

    # A FLOOR, because the scan finding nothing is also what a broken scan
    # produces. 20 is well under the count when this was written; it exists to
    # catch the glob or the detector silently matching nothing, not to be
    # tight.
    if checked < 20:
        print(
            f"check-selftest-flag-spellings: only {checked} script(s) appear to "
            f"have a self-test mode, which is too few to believe -- the "
            f"detector or the glob is broken, not the tree.",
            file=sys.stderr,
        )
        return 2

    for name, why in findings:
        print(f"  {name}: {why}")
    if findings:
        print(
            f"\ncheck-selftest-flag-spellings: {len(findings)} script(s) accept "
            f"one spelling of the self-test flag and silently do something else "
            f"for the rest.\n"
            f"\nUse `selftestflag.wants_selftest(argv)`, or name every spelling "
            f"in one add_argument call. An exit status of 0 from a self-test "
            f"that did not run is worth less than no self-test at all, because "
            f"somebody believes it.",
            file=sys.stderr,
        )
        return 1
    print(f"ok -- {checked} script(s) with a self-test mode accept every spelling")
    return 0


def _self_test() -> int:
    failures = 0

    def case(src, want, what):
        nonlocal failures
        got = verdict(src)
        ok = (got == "ok") if want == "ok" else (got != "ok" and got != "no-selftest")
        if want == "no-selftest":
            ok = got == "no-selftest"
        if not ok:
            print(f"FAIL {what}\n  got {got!r}\n  wanted {want!r}")
            failures += 1
        else:
            print(f"  ok    {what}")

    case(
        'if "--self-test" in argv:\n',
        "bad",
        "one membership test reaches one spelling",
    )
    case('if "--selftest" in sys.argv[1:]:\n', "bad", "...whichever one it is")
    # THE CASE THAT MADE THE FIRST VERSION OF THIS CHECKER WRONG four times out
    # of its first five findings. `check-libc-shape.py` is written exactly like
    # this and is correct; the rule as first stated -- "never test membership by
    # hand" -- outlawed it anyway. Banning a construct catches its correct uses
    # too, so the question had to become what the caller can reach.
    case(
        'if "--self-test" in sys.argv[1:] or "--selftest" in sys.argv[1:]:\n',
        "ok",
        "TWO MEMBERSHIP TESTS REACH BOTH, so the construct is not the defect",
    )
    case(
        "if selftestflag.wants_selftest(argv):\n",
        "ok",
        "the helper is accepted",
    )
    case(
        'ap.add_argument("--self-test", "--selftest", "--self_test", action="s")\n',
        "ok",
        "an add_argument naming all three is accepted",
    )
    case(
        'ap.add_argument("--self-test", action="store_true")\n',
        "bad",
        "an add_argument naming one is refused",
    )
    case(
        'ap.add_argument("--selftest", "--self-test", action="store_true")\n',
        "ok",
        "naming both required spellings passes, in either order",
    )
    case(
        'ap.add_argument("--selftest", action="store_true")\n',
        "bad",
        "...but naming only the unhyphenated one is refused too",
    )
    case("print('hello')\n", "no-selftest", "a script with no self-test is skipped")
    # The docstring of this very file names the flags dozens of times. A
    # detector that matched prose would report every script that DISCUSSES the
    # rule, which is how a gate becomes noise and then becomes bypassed.
    case(
        '"""A doc that says --self-test and --selftest a lot."""\nprint(1)\n',
        "no-selftest",
        "PROSE MENTIONING THE FLAGS IS NOT A SELF-TEST MODE",
    )
    # The real shape of the fix applied across the tree: helper call plus the
    # import, with the old membership test gone.
    case(
        "import selftestflag\n\nif selftestflag.wants_selftest(sys.argv[1:]):\n",
        "ok",
        "the converted shape is accepted",
    )
    # A file that has BOTH -- mid-conversion -- must not pass on the strength
    # of the helper it also contains.
    case(
        "import selftestflag\n"
        "if selftestflag.wants_selftest(argv):\n    pass\n"
        '# was: "--self-test" in argv\n',
        "ok",
        "A CONVERTED FILE STILL PASSES with the old literal left in a comment",
    )
    # This file's own docstring quotes `"--self-test" in argv` while explaining
    # why that construct is banned. A detector that read prose would report
    # every script that DISCUSSES the rule -- including this one, which is how
    # the regex below came to match its own source before the quotes were
    # tightened.
    case(
        '"""Do not write `"--self-test" in argv`."""\n'
        "import selftestflag\n"
        "if selftestflag.wants_selftest(argv):\n    pass\n",
        "ok",
        "a file that DOCUMENTS the banned construct is not accused of it",
    )

    if failures:
        print(f"\ncheck-selftest-flag-spellings: {failures} self-test failure(s)")
        return 1
    print("check-selftest-flag-spellings: self-test passed (0 failure(s))")
    return 0


def main(argv) -> int:
    unknown = selftestflag.unknown_options(argv[1:])
    if unknown:
        print(
            f"check-selftest-flag-spellings.py: unrecognised option "
            f"{unknown[0]!r}",
            file=sys.stderr,
        )
        return 2
    if selftestflag.wants_selftest(argv[1:]):
        return _self_test()
    return scan()


if __name__ == "__main__":
    sys.exit(main(sys.argv))
