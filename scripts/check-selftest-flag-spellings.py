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
# An equality test in a hand-rolled argv loop:
#
#     elif arg == "--self-test":
#         return self_test()
#
# The third construct, and the one this checker could not see until
# 2026-09-15. `find-claimed-acts.py` and `find-overstated-records.py` both
# reach their self-test this way and both were reported as having none, so
# neither was ever asked whether it accepts the other spelling. They do not.
_EQUALITY = re.compile(r'==\s*"(--self[-_]?test)"|"(--self[-_]?test)"\s*==')
# Membership in a tuple or list of spellings:
#
#     if any(a in ("--selftest", "--self-test", "--self_test") for a in argv):
#
# The fourth construct, and correct -- it names every spelling in one place,
# which is the thing this gate is asking for. Matched as "an `in` followed by a
# bracket containing at least one spelling", then every spelling inside that
# bracket is reachable.
_TUPLE_MEMBERSHIP = re.compile(r'\bin\s*[\(\[]([^)\]]*"--self[-_]?test"[^)\]]*)[\)\]]')
# The shared splat: `add_argument(*selftestflag.SPELLINGS, ...)`. Reaching for
# the shared constant is the best answer available, so recognising it is not a
# courtesy -- a gate that reports the recommended fix as a finding teaches
# people to stop using it.
_SPELLINGS_SPLAT = re.compile(r"selftestflag\.SPELLINGS")
# A bracketed literal naming EVERY required spelling, wherever it sits:
#
#     selftest_spellings = ("--self-test", "--selftest", "--self_test")
#     if arg in selftest_spellings: ...
#
# The fifth construct. `_TUPLE_MEMBERSHIP` above wanted the bracket to follow
# `in` directly, so a tuple given a NAME first was invisible --
# `check-recursive-locks.py` and `rustscan.py` both do that and both are
# correct, confirmed by running each with both spellings.
#
# Requires all of REQUIRED inside one bracket, deliberately. A bracket holding
# just one spelling is usually an argv being BUILT rather than matched --
# `[sys.executable, checker, "--selftest"]` in
# `test-selftests-are-repo-safe.py` is that, and reading it as a declaration
# would report the file as accepting only the spelling it hands to somebody
# else.
_SPELLING_GROUP = re.compile(r"[\(\[]([^)\]]*)[\)\]]", re.S)

# Files whose self-test wiring this checker cannot read, each with the reason
# and the evidence. Same principle as the IGNORE table in
# `scripts/raced-globals.py`: a line here says *why* and can be argued with,
# where widening a pattern to silence one file quietly widens it for every
# file.
BLIND_SPOT_OK: dict[str, str] = {
    # Takes no flags at all: two `if __name__ == "__main__"` blocks, because
    # the file EMBEDS a sample checker as a string to test the safety of
    # self-tests that build throwaway repositories. The `def self_test` the
    # detector sees is inside that fixture, not a function this file runs, and
    # the `"--selftest"` it contains is the argument it passes to the checkers
    # it exercises. Verified 2026-09-15 by reading both __main__ blocks.
    "test-selftests-are-repo-safe.py": "no flags of its own; the self-test it appears to define is inside an embedded fixture string",
}

# Evidence that a file HAS a self-test, independent of how its flag is wired.
# This is the control, and it is the point of the change rather than a detail:
# without it, "this script has no self-test" and "I could not see this
# script's self-test" are the same observation, and the second is silent.
# That is the exact failure this file's own header forbids -- success and
# not-running must not be indistinguishable -- reproduced one level up, in the
# checker instead of in the checked.
_DEFINES_SELFTEST = re.compile(r"^def _?self_?test\b", re.M)


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
    if _SPELLINGS_SPLAT.search(src):
        return "ok"

    reachable = {m.group(1) for m in _MEMBERSHIP.finditer(src)}
    reachable |= {g for m in _EQUALITY.finditer(src) for g in m.groups() if g}
    for group in _TUPLE_MEMBERSHIP.findall(src):
        reachable |= {s for s in REQUIRED if f'"{s}"' in group}
    for group in _SPELLING_GROUP.findall(src):
        if all(f'"{s}"' in group for s in REQUIRED):
            reachable |= set(REQUIRED)
    declared = set()
    for call in _ADD_ARG.findall(src):
        declared |= {s for s in REQUIRED if f'"{s}"' in call}
    reachable |= declared

    if not reachable:
        # Nothing reachable AND a self-test function present means this
        # checker failed to parse how the flag gets there -- a fourth
        # construct, or a refactor of one of the three. Reported rather than
        # skipped: a script dropped here is a script never asked the question,
        # and it would be dropped SILENTLY, which is how the two scripts that
        # prompted this went eleven days unexamined.
        # A file with no flag at all cannot spell one wrongly. `rustlex.py`
        # and `rustrungs.py` run their self-test unconditionally from
        # `__main__`, so every spelling reaches it, including none. Checked
        # before the blind-spot report below, or the absence of a flag would
        # be reported as an unreadable flag.
        if '"--self' not in src:
            return "no-selftest"
        if _DEFINES_SELFTEST.search(src):
            return (
                "defines a self-test function but this checker cannot see how "
                "any flag reaches it -- a construct it does not know. Teach it "
                "the construct, or switch the script to "
                "`selftestflag.wants_selftest(argv)`"
            )
        return "no-selftest"

    missing = [s for s in REQUIRED if s not in reachable]
    if not missing:
        return "ok"
    return (
        f"reaches its self-test only via {', '.join(sorted(reachable))}; "
        f"{', '.join(missing)} falls through to the default action instead. "
        f"Where that action exits 0 -- a real scan, a usage notice -- the "
        f"mistyped command reports success having tested nothing; where it "
        f"errors, the self-test simply cannot be invoked by the name somebody "
        f"remembers. This checker cannot tell which from the source, so it "
        f"names the reachable spellings and not the consequence"
    )


def scan() -> int:
    findings = []
    checked = 0
    for path in sorted(SCRIPTS.glob("*.py")):
        src = path.read_text(encoding="utf-8", errors="surrogateescape")
        v = verdict(src)
        if path.name in BLIND_SPOT_OK and v.startswith("defines a self-test"):
            continue
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

    # --- the constructs added 2026-09-15 ----------------------------------
    #
    # Each was found by RUNNING a script with both spellings and comparing,
    # after this checker had reported 93 scripts clean while skipping seven it
    # could not parse. A static detector that disagrees with the behaviour is
    # wrong by definition, so each case below pins a shape the behaviour
    # showed was already correct -- or, for the first pair, one it showed was
    # not.
    case(
        'elif arg == "--self-test":\n    return self_test()\n',
        "bad",
        "an equality test naming ONE spelling is a finding",
    )
    case(
        'elif arg in ("--self-test", "--selftest"):\n    return self_test()\n',
        "ok",
        "...and naming both in the same test is not",
    )
    case(
        'SPELLINGS = ("--self-test", "--selftest", "--self_test")\n'
        'if arg in SPELLINGS:\n    return self_test()\n',
        "ok",
        "a spelling tuple given a NAME first is still a declaration",
    )
    case(
        'ap.add_argument(*selftestflag.SPELLINGS, dest="selftest")\n',
        "ok",
        "the shared splat is the recommended fix and must not be a finding",
    )
    # The blind spot, and its control. These two differ only in whether a
    # flag exists at all, and they must not produce the same verdict: one is
    # "I cannot read this file's wiring" and the other is "there is no wiring
    # to read". Collapsing them is what let seven scripts be skipped in
    # silence, which is this file's own header applied to itself.
    case(
        'def self_test():\n    pass\n'
        'if SOME_CONSTANT == "--self-test-ish":\n    self_test()\n',
        "bad",
        "a self-test whose wiring cannot be parsed is reported, not skipped",
    )
    case(
        'def self_test():\n    pass\n'
        'if __name__ == "__main__":\n    sys.exit(self_test())\n',
        "no-selftest",
        "...while a file with no flag at all cannot spell one wrongly",
    )

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
