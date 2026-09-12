#!/usr/bin/env python3
"""Refuse an undeclared difference between a gate's push and boot invocations.

WHY THIS EXISTS. Lane A read their own push output, saw `cfg-unix` in the list
of gates that ran, and concluded the boot test's check had been pre-run. It had
not: the push invocation checked 62 crates on default targets, while boot-test
runs the whole workspace with `--all-targets`. Same gate name, two different
questions, and the narrower one reported success. **A widened hook that repeats
that is worse than a narrow one, because the reassurance is what does the
damage.**

WHAT THIS DOES NOT DO, and the first draft got this wrong. It does not demand
that the two invocations match. Four of the six shared gates differ today and
every one of them is right to: the hook passes `--head <sha>` and
`--changed-only` because it is checking the commits being pushed, while
boot-test scans everything. That division is the design -- incremental at push,
exhaustive at boot -- and a gate that flagged it would be telling the truth
about something nobody should change.

WHAT IT DOES. Every difference must be DECLARED, with a reason, in `NARROWER`
below. An undeclared one is refused, because it is how a gate quietly becomes
narrower at push time. And a declaration that has stopped being true -- the two
invocations now agree -- is *also* refused, because a stale declaration is the
failure this tree has spent a day pulling out of its documents: prose that was
correct when written, read later as though it still were.

The pairing is per-SCRIPT, not per-label. `run_checker` takes a label that
often differs between the two files (`check-one-libc` against
`check-one-libc-selftest`), and a label is a name someone chose. The script path
is the thing that actually runs.

Self-test invocations are excluded. Whether a self-test runs in both places is a
real question and `check-gates-are-wired.py` already answers it; mixing the two
would make this report about two things at once.
"""

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import selftestflag

ROOT = Path(__file__).resolve().parent.parent
BOOT = ROOT / "scripts" / "boot-test.sh"
PUSH = ROOT / "scripts" / "hooks" / "pre-push"

# `checker="${repo_root:-.}/scripts/multicall-aliases.py"` — the hook keeps its
# script paths in locals, so a parser that only understood a literal path found
# zero invocations in it and reported perfect parity across an empty set.
VAR_ASSIGN = re.compile(
    r"^\s*(?:local\s+)?([A-Za-z_][A-Za-z0-9_]*)="
    r"\"?\$\{[^}]*\}/scripts/([A-Za-z0-9_.\-]+\.(?:py|sh))\"?\s*$",
    re.M,
)
LITERAL_PATH = re.compile(r"scripts/([A-Za-z0-9_.\-]+\.(?:py|sh))$")
SELFTEST_FLAGS = {"--selftest", "--self-test"}

# Every difference between the two invocations, with why it is correct.
#
# A name here is a promise that the hook's narrower check is DELIBERATE, not
# that it is equivalent. The reason is what a reader needs when the hook reports
# the gate as having run.
NARROWER = {
    "check-accidental-headings.py":
        "hook checks only the pushed commits (--head/--changed-only); boot-test "
        "scans every document, so a heading broken by an older commit is caught "
        "at boot and not at push.",
    "check-design-decisions-bands.py":
        "hook judges the pushed revision (--head); boot-test judges the "
        "worktree, which is what the boot itself reads.",
    "check-read-defaults.py":
        "hook judges the pushed revision (--head); boot-test judges the "
        "worktree.",
    "check-requests-not-deleted.py":
        "hook compares against the pushed revision (--head); boot-test compares "
        "the worktree, so a request deleted without committing is caught at "
        "boot.",
}


def script_variables(text):
    """Map shell variable name -> script file name."""
    return {m.group(1): m.group(2) for m in VAR_ASSIGN.finditer(text)}


def invocations(path):
    """Map script file name -> set of argument tuples it is invoked with."""
    raw = path.read_text(encoding="utf-8", errors="replace")
    variables = script_variables(raw)
    joined = raw.replace(chr(92) + chr(10), " ")
    found = {}
    for m in re.finditer(r"run_checker\s+(.+)", joined):
        line = m.group(1)
        for cut in (";", "||", "&&"):
            line = line.split(cut)[0]
        tokens = [t.strip('"') for t in line.split() if t.strip()]
        script = None
        index = None
        for i, tok in enumerate(tokens):
            hit = LITERAL_PATH.search(tok)
            if hit:
                script, index = hit.group(1), i
                break
            name = tok.lstrip("$").strip("{}")
            if name in variables:
                script, index = variables[name], i
                break
        if script is None:
            continue
        args = tuple(a for a in tokens[index + 1 :] if a != "then")
        if SELFTEST_FLAGS & set(args):
            continue
        found.setdefault(script, set()).add(args)
    return found


def compare(boot, push, declared):
    """Return (undeclared_differences, expired_declarations, shared_count)."""
    shared = sorted(set(boot) & set(push))
    undeclared = []
    expired = []
    for script in shared:
        differs = boot[script] != push[script]
        if differs and script not in declared:
            undeclared.append((script, sorted(boot[script]), sorted(push[script])))
        elif not differs and script in declared:
            expired.append(script)
    for script in declared:
        if script not in shared:
            expired.append(script)
    return undeclared, sorted(set(expired)), len(shared)


SELFTEST = [
    (
        "an undeclared difference is refused",
        {"a.py": {("--full",)}},
        {"a.py": {("--head", "X")}},
        {},
        (1, 0),
    ),
    (
        "...and a declared one is not",
        {"a.py": {("--full",)}},
        {"a.py": {("--head", "X")}},
        {"a.py": "checked incrementally at push"},
        (0, 0),
    ),
    (
        "identical invocations need no declaration",
        {"a.py": {("--full",)}},
        {"a.py": {("--full",)}},
        {},
        (0, 0),
    ),
    (
        "a declaration that has stopped being true is refused",
        {"a.py": {("--full",)}},
        {"a.py": {("--full",)}},
        {"a.py": "checked incrementally at push"},
        (0, 1),
    ),
    (
        "a declaration naming a gate that is no longer shared is refused",
        {"a.py": {("--full",)}},
        {"b.py": {("--full",)}},
        {"a.py": "checked incrementally at push"},
        (0, 1),
    ),
    (
        "a gate only one side runs is not this check's business",
        {"a.py": {("--full",)}, "solo.py": {()}},
        {"a.py": {("--full",)}},
        {},
        (0, 0),
    ),
]


def selftest():
    bad = 0
    for name, boot, push, declared, want in SELFTEST:
        undeclared, expired, _ = compare(boot, push, declared)
        got = (len(undeclared), len(expired))
        ok = got == want
        bad += 0 if ok else 1
        print("%-4s %s" % ("ok" if ok else "FAIL", name))
        if not ok:
            print("       wanted %s, got %s" % (want, got))
    print()
    print(
        "check-gate-invocation-parity selftest: %d case(s), %d failed"
        % (len(SELFTEST), bad)
    )
    return 1 if bad else 0


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if selftestflag.wants_selftest(argv):
        return selftest()

    if not BOOT.is_file() or not PUSH.is_file():
        print("check-gate-invocation-parity: boot-test.sh or pre-push missing", file=sys.stderr)
        return 2

    boot = invocations(BOOT)
    push = invocations(PUSH)
    undeclared, expired, shared = compare(boot, push, NARROWER)

    for script, b, p in undeclared:
        print("%s runs differently at push than at boot, undeclared:" % script)
        print("    boot-test: %s" % " | ".join(" ".join(a) or "(no arguments)" for a in b))
        print("    pre-push : %s" % " | ".join(" ".join(a) or "(no arguments)" for a in p))
        print("    If the narrower one is deliberate, name it in NARROWER with")
        print("    the reason. The hook reports this gate as having run, and a")
        print("    reader has no other way to learn it checked less.")
        print()

    for script in expired:
        print("%s is declared in NARROWER and no longer needs to be." % script)
        print("    The two invocations now agree, or the gate is no longer run")
        print("    by both. A declaration outliving its reason is the failure")
        print("    this gate exists to prevent, one level up.")
        print()

    # Name the population: "0 mismatches" and "found no invocations to compare"
    # must not print the same thing. The first version of this checker parsed
    # zero invocations out of the hook, because the hook keeps its script paths
    # in shell locals, and reported perfect parity across an empty set.
    print(
        "check-gate-invocation-parity: %d gate(s) in boot-test, %d in pre-push, "
        "%d in both; %d declared narrower, %d undeclared, %d expired."
        % (len(boot), len(push), shared, len(NARROWER), len(undeclared), len(expired))
    )
    return 1 if (undeclared or expired) else 0


if __name__ == "__main__":
    sys.exit(main())
