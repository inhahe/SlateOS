#!/usr/bin/env python3
"""Every self-test the push hook runs must leave the real repository alone.

Why this exists
---------------

Twice now, a checker's ``--selftest`` has built a throwaway repository in a
temp directory, driven it with ``git -C <tmp>``, and destroyed the repository
that was being pushed instead.

``git -C <dir>`` does not name a repository. An explicit ``GIT_DIR`` in the
environment outranks both it and ``cwd``, and git *exports* ``GIT_DIR`` into
every hook's environment -- so a self-test that is correct when a human runs it
is wrong the first time ``pre-push`` runs it, and wrong in the worst available
way: it writes to the repository it was meant to leave alone while reporting
success about the fixture it thought it was using.

* **2026-08-29**, ``check-requests-not-deleted.py``: ``git init`` re-initialised
  the repository and set ``core.bare=true`` on the shared config, ``git add -A``
  replaced the index with the fixture's three files, and two commits whose tree
  was a single ``requests/`` directory -- the entire repository deleted -- were
  written to `lane-a` and **published to origin**, because the gate passed.
* **2026-09-04**, ``check-design-decisions-bands.py``: the same sequence, six
  fixture commits onto `lane-a`, plus ``user.email=selftest@example.invalid``
  written into ``os/.git/config``, which all three worktrees read. Nothing
  reached origin only because an unrelated gate refused the push moments later.

``scripts/gitenv.py`` was written as the first post-mortem and documents the
mechanism precisely. The second incident happened anyway, in a self-test
written months later by someone who had not read it. That is the fact this
suite is a response to: the knowledge existed, was written down, and did not
travel to the next author.

Why it is one suite over all the gates, not a case in each gate's suite
----------------------------------------------------------------------

Gate 9 gained a per-gate regression test after the first incident. It worked --
gate 9 has not recurred -- and it protected exactly one gate, so the second
incident landed in the next self-test written. A per-gate test can only cover
gates that already exist, which is never the one that is about to be written.

So the list of gates here is **discovered from the hook**, not enumerated: any
``run_checker <name>-selftest "$py" "$<var>"`` line is picked up, and the
checker it names is covered from the moment it is wired. Gate 14's self-test is
protected before its author has heard of this file, which is the only
arrangement that would have prevented the second incident.

The discovery is asserted against a floor, because a parser that finds nothing
looks exactly like a suite where everything passes.

For the same reason the suite carries a **canary**: a checker written the way
both incidents were written, which this suite must catch damaging its fixture.
On a healthy tree every other assertion here is a PASS, and an all-PASS run
cannot on its own distinguish "the gates are safe" from "the detector is
broken". The canary makes that distinguishable.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(HERE)
HOOK = os.path.join(HERE, "hooks", "pre-push")

sys.path.insert(0, HERE)
import gitenv  # noqa: E402

_FAILURES: list[str] = []


def check(label, got, want):
    ok = got == want
    print(f"{'PASS' if ok else 'FAIL'}  {label}")
    if not ok:
        print(f"        got : {got!r}")
        print(f"        want: {want!r}")
        _FAILURES.append(label)
    return ok


# --------------------------------------------------------------------------
# Discovery
# --------------------------------------------------------------------------

# `run_checker <gate>-selftest "$py" "$var"` — the hook's one spelling for
# "prove the checker before trusting its verdict".
_SELFTEST_CALL = re.compile(
    r'^\s*(?:if\s+!\s+)?run_checker\s+([A-Za-z0-9-]+)-selftest\s+"\$py"\s+"\$(\w+)"',
    re.MULTILINE)

# `var="${repo_root:-.}/scripts/thing.py"` — where that `$var` came from.
_CHECKER_PATH = re.compile(
    r'^(\w+)="\$\{repo_root:-\.\}/(scripts/[^"]+\.py)"\s*$', re.MULTILINE)


def discover_gated_selftests():
    """[(gate, checker_relpath)] for every self-test the hook runs."""
    text = open(HOOK, encoding="utf-8").read()
    paths = dict(
        (m.group(1), m.group(2)) for m in _CHECKER_PATH.finditer(text))
    found = []
    for m in _SELFTEST_CALL.finditer(text):
        gate, var = m.group(1), m.group(2)
        # An unresolvable variable is a finding, not something to skip past:
        # it means the hook's shape changed and this suite has stopped
        # covering a gate while still reporting success.
        found.append((gate, paths.get(var)))
    return found


def test_discovery_finds_the_gated_selftests():
    found = discover_gated_selftests()
    # A floor, as the sibling suites use. Discovery that finds nothing is
    # indistinguishable from a suite in which everything passes.
    if len(found) < 6:
        check("discovery found the hook's self-tests", f"{len(found)} found",
              "at least 6")
        return
    print(f"PASS  discovery found {len(found)} gated self-test(s)")
    for gate, rel in found:
        if rel is None:
            check(f"the checker behind {gate}-selftest is resolvable",
                  None, "a scripts/*.py path")
        elif not os.path.exists(os.path.join(REPO_ROOT, rel)):
            check(f"the checker behind {gate}-selftest exists", rel,
                  "an existing file")


# --------------------------------------------------------------------------
# The victim
# --------------------------------------------------------------------------

def _git(cwd, *args):
    return subprocess.run(["git", *args], cwd=cwd, env=gitenv.clean_env(),
                          capture_output=True, text=True,
                          check=True).stdout.strip()


def _make_victim(tmp):
    """A small, complete repository whose only purpose is to be damaged.

    Built rather than cloned: the assertion is "nothing changed" about every
    ref, the index and the config, which is only tractable for a repository
    this test wrote itself.
    """
    _git(tmp, "init", "--quiet", "-b", "main")
    _git(tmp, "config", "user.email", "victim@example.invalid")
    _git(tmp, "config", "user.name", "victim")
    with open(os.path.join(tmp, "keep-me.txt"), "w",
              encoding="utf-8", newline="") as fh:
        fh.write("this file must survive\n")
    _git(tmp, "add", "-A")
    _git(tmp, "commit", "--quiet", "-m", "the victim's only commit")


def _probe(cwd, *args):
    """One snapshot field, as a string, even when git refuses to answer.

    Raising would be wrong twice over: a damaged repository is precisely what
    this suite hunts, so the probe that hits the damage is the one carrying
    the finding -- and the first observed damage was `core.bare=true`, which
    makes `git status` fail outright rather than answer differently. An
    exception there aborts the run, loses the remaining environment shapes,
    and prints a traceback where a diff belongs.
    """
    proc = subprocess.run(["git", *args], cwd=cwd, env=gitenv.clean_env(),
                          capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        return f"<git {' '.join(args)} failed: {proc.stderr.strip()}>"
    return proc.stdout.strip()


def _snapshot(tmp):
    """Everything the two incidents actually changed, plus the refs."""
    return {
        "head": _probe(tmp, "rev-parse", "HEAD"),
        "branch": _probe(tmp, "rev-parse", "--abbrev-ref", "HEAD"),
        "refs": _probe(tmp, "for-each-ref",
                       "--format=%(refname) %(objectname)"),
        "bare": _probe(tmp, "config", "--get", "core.bare"),
        "user.email": _probe(tmp, "config", "--get", "user.email"),
        "user.name": _probe(tmp, "config", "--get", "user.name"),
        "index": _probe(tmp, "ls-files"),
        "log": _probe(tmp, "log", "--oneline"),
        "status": _probe(tmp, "status", "--porcelain"),
    }


# The environments to run each self-test under. The first is the one that
# actually happened twice: `git push` exports `GIT_DIR` and nothing else, and
# that alone sufficed -- with no `GIT_WORK_TREE` to contradict it, git took the
# fixture's temp directory as the work tree and wrote into the repository being
# pushed.
#
# The later shapes are different failure modes rather than embellishments.
# `GIT_INDEX_FILE` as well makes the fixture's first commit fail outright, so a
# broken checker is caught by its exit code before it can damage anything;
# `GIT_WORK_TREE` makes the fixture write over the victim's own files. A suite
# using only the loudest shape would prove the least, since the quiet one is
# the one that shipped -- twice.
_HOOK_ENVIRONMENTS = (
    ("GIT_DIR only, as `git push` sets it", ("GIT_DIR",)),
    ("GIT_DIR + GIT_WORK_TREE", ("GIT_DIR", "GIT_WORK_TREE")),
    ("GIT_DIR + GIT_WORK_TREE + GIT_INDEX_FILE",
     ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE")),
)


def _run_selftest_against_a_victim(checker, names):
    """Run `checker --selftest` in a hook's environment, over a fresh victim.

    Returns ``(proc, before, after)``. The victim is built, snapshotted, run
    over and snapshotted again inside one temp directory that is then removed,
    so no two calls can observe each other.
    """
    with tempfile.TemporaryDirectory(prefix="selftest-victim-") as tmp:
        _make_victim(tmp)
        before = _snapshot(tmp)

        values = {
            "GIT_DIR": os.path.join(tmp, ".git"),
            "GIT_WORK_TREE": tmp,
            "GIT_INDEX_FILE": os.path.join(tmp, ".git", "index"),
        }
        hostile = gitenv.clean_env()
        for name in names:
            hostile[name] = values[name]

        # `cwd=tmp` on purpose: a hook runs from the repository root, but
        # pointing cwd at the victim as well is the harsher case and costs
        # nothing.
        proc = subprocess.run(
            [sys.executable, checker, "--selftest"],
            cwd=tmp, env=hostile, capture_output=True, text=True, check=False)

        after = _snapshot(tmp)
    return proc, before, after


def test_no_gated_selftest_can_damage_the_repository_it_runs_from():
    """The 2026-08-29 and 2026-09-04 incidents, as a test, for every gate.

    A hook's environment is not approximated: the variables carry the values
    git itself sets, because the entire failure is that they outrank `cwd` and
    `-C`.
    """
    gates = discover_gated_selftests()
    for gate, rel in gates:
        if rel is None:
            continue  # already reported by the discovery test
        checker = os.path.join(REPO_ROOT, rel)
        if not os.path.exists(checker):
            continue
        for label, names in _HOOK_ENVIRONMENTS:
            proc, before, after = _run_selftest_against_a_victim(
                checker, names)

            if not check(f"[{gate}] the self-test still passes under "
                         f"{label}", proc.returncode, 0):
                tail = (proc.stdout + proc.stderr).strip().split("\n")[-12:]
                print("        --- checker output (tail) ---")
                for line in tail:
                    print("        " + line)

            for key in sorted(before):
                check(f"[{gate}] {label}: left {key} alone",
                      after[key], before[key])


# --------------------------------------------------------------------------
# The canary
# --------------------------------------------------------------------------

# A checker written the way both incidents were written: a scratch repository
# in a temp directory, driven with `git -C <tmp>`, inheriting the environment.
# Every line of it is correct when a human runs it from a shell.
#
# It exists because the suite above is all-PASS on a healthy tree, and an
# all-PASS suite has two indistinguishable explanations: the gates are safe, or
# the detector is broken. The detector has more moving parts than it looks --
# `_probe` swallows git's exit status by design, `_snapshot` has to name the
# right nine fields, and `TemporaryDirectory` has to survive a repository being
# re-initialised underneath it. Any one of those failing quietly turns this
# file into 240 assertions that a temp directory exists.
#
# So the canary is damage on demand: run it and the snapshot MUST change. If it
# ever comes back clean, the finding is about this suite, not about the gates.
_UNSAFE_CHECKER = '''\
import os, subprocess, sys, tempfile

def selftest():
    tmp = tempfile.mkdtemp(prefix="canary-fixture-")
    def git(*a):
        # No `env=`. This is the defect, verbatim: `-C` does not outrank an
        # inherited GIT_DIR, so under a hook every one of these lands in the
        # repository being pushed.
        subprocess.run(["git", "-C", tmp, *a], check=True,
                       capture_output=True, text=True)
    git("init", "--quiet", "-b", "main")
    git("config", "user.email", "canary@example.invalid")
    git("config", "user.name", "canary")
    with open(os.path.join(tmp, "fixture.txt"), "w") as fh:
        fh.write("fixture\\n")
    git("add", "-A")
    git("commit", "--quiet", "-m", "the canary's fixture commit")
    return True

if __name__ == "__main__":
    sys.exit(0 if selftest() else 1)
'''


def test_the_suite_can_tell_a_damaged_repository_from_an_intact_one():
    """Prove the detector, in the same environments, before trusting a PASS."""
    with tempfile.TemporaryDirectory(prefix="selftest-canary-") as home:
        canary = os.path.join(home, "unsafe-checker.py")
        with open(canary, "w", encoding="utf-8", newline="") as fh:
            fh.write(_UNSAFE_CHECKER)

        for label, names in _HOOK_ENVIRONMENTS:
            _proc, before, after = _run_selftest_against_a_victim(
                canary, names)
            # Deliberately not asserting *which* fields moved: the third
            # environment makes the fixture's commit fail outright, so the
            # damage stops earlier than under the first. What must hold in
            # every shape is that something moved and this suite saw it.
            changed = sorted(k for k in before if after[k] != before[k])
            if changed:
                print(f"PASS  [canary] {label}: damage seen in "
                      f"{', '.join(changed)}")
            else:
                check(f"[canary] {label}: the suite noticed the damage",
                      "no field changed",
                      "at least one of " + ", ".join(sorted(before)))


# Each snapshot field, and one edit that must move it. The canary above proves
# the harness detects damage, but it only moves four of the nine fields --
# under `GIT_DIR` alone `git init` sets `core.bare=true`, which makes the
# fixture's `git add` fail before it can write a ref, and under the other two
# shapes `git add -A` stages the victim's own unchanged work tree and the
# commit refuses. So `head`, `log`, `refs`, `branch` and `index` are, on the
# evidence of the canary alone, five probes that have never been observed to
# change. A probe that cannot move is not a weak assertion, it is a blind spot
# shaped exactly like a PASS -- and `_probe` swallowing git's exit status by
# design is precisely the sort of thing that would produce one silently.
#
# The fields are grouped, not listed one per mutation: one commit moves both
# `head` and `log`, and splitting it in two would silently pass, because the
# second check would compare a post-commit snapshot against a post-commit
# snapshot and find, correctly, that nothing had changed since.
_FIELD_MUTATIONS = (
    (("status",), "an untracked file appears", ("__WRITE__", "scratch.txt")),
    (("index",), "a file is staged", ("add", "scratch.txt")),
    (("head", "log"), "a commit is written", ("__COMMIT__",)),
    (("refs",), "a branch is created",
     ("branch", "a-branch-nobody-asked-for")),
    (("branch",), "HEAD moves to another branch",
     ("checkout", "--quiet", "a-branch-nobody-asked-for")),
    (("user.email",), "an identity is written to the config",
     ("config", "user.email", "canary@example.invalid")),
    (("user.name",), "an identity is written to the config",
     ("config", "user.name", "canary")),
    # Last, deliberately: it is what makes `git status` fail outright, so any
    # probe run after it reports the failure string rather than an answer.
    (("bare",), "the repository is declared bare",
     ("config", "core.bare", "true")),
)


def test_every_snapshot_field_can_actually_move():
    """No probe in `_snapshot` is a constant.

    Run against one victim, cumulatively: each mutation is applied, the
    snapshot retaken, and every field it is supposed to move asserted to
    differ from the snapshot before it.

    Every field in `_snapshot` must appear, or the coverage claim is the very
    kind of unstated gap this file exists to close.
    """
    covered = {f for fields, _why, _m in _FIELD_MUTATIONS for f in fields}
    with tempfile.TemporaryDirectory(prefix="selftest-probe-") as tmp:
        _make_victim(tmp)
        previous = _snapshot(tmp)

        missing = sorted(set(previous) - covered)
        if missing:
            check("every snapshot field has a mutation that moves it",
                  f"unexercised: {', '.join(missing)}", "none unexercised")

        for fields, why, mutation in _FIELD_MUTATIONS:
            if mutation[0] == "__WRITE__":
                with open(os.path.join(tmp, mutation[1]), "w",
                          encoding="utf-8", newline="") as fh:
                    fh.write("scratch\n")
            elif mutation[0] == "__COMMIT__":
                _git(tmp, "commit", "--quiet", "-m",
                     "a commit nobody asked for")
            else:
                _git(tmp, *mutation)

            current = _snapshot(tmp)
            for field in fields:
                if current[field] != previous[field]:
                    print(f"PASS  [probe] {field} moved when {why}")
                else:
                    check(f"[probe] {field} moved when {why}",
                          f"{field} unchanged at {current[field]!r}",
                          "a different value")
            previous = current


def main():
    if not os.path.exists(HOOK):
        print(f"FATAL: {HOOK} does not exist; this suite reads the hook to "
              f"discover what to cover.")
        return 1

    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    if len(tests) < 4:
        print(f"FATAL: test discovery found only {len(tests)} tests; the "
              f"suite has at least 4. Discovery is broken, not the code.")
        return 1
    for _name, fn in tests:
        fn()

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(sorted(set(_FAILURES)))}")
        return 1
    print("all selftests-are-repo-safe checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
