#!/usr/bin/env python3
"""Structural tests for `scripts/hooks/pre-push`.

Run: `python scripts/test-pre-push-gates.py` (0 = pass, 1 = fail). No pytest
dependency, matching the other suites in this directory.

What this tests, and why it is worth testing
--------------------------------------------

Not the gates' logic -- each gate's checker has its own `--selftest`, and
several have their own suite. This tests the *shape* of the hook, which is
where it has actually gone wrong.

The hook opens with a numbered list of what it refuses to publish. That list is
a second copy of a fact the code already states, and it rotted: it read "Seven
gates" for as long as eight were implemented, so a reader counting on it was
told the wrong thing about a security boundary. The list is short and genuinely
useful at the top of a 1000-line file, so the answer is not to delete it but to
stop trusting it -- hence this suite.

The other properties here are ones whose violation is silent:

* **A gate with no bypass** cannot be got past when it is wrong, and a gate
  that cannot be got past when it is wrong gets deleted instead.
* **Two gates sharing a bypass** means silencing one silently silences the
  other, which the hook's own header calls out as the thing to avoid.
* **A bypass the failure message never names** leaves the blocked author to
  grep a thousand lines for the variable, at the moment they are most likely
  to reach for `--no-verify` instead -- which turns off all nine.
* **`ALLOW_EVERYTHING`** is argued against in the header ("one variable away
  from turning the push boundary off entirely"). An argument in a comment does
  not stop anyone implementing it later; this does.
"""

from __future__ import annotations

import inspect
import os
import re
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HOOK = os.path.join(REPO_ROOT, "scripts", "hooks", "pre-push")

# Spelled-out counts, because that is how the header writes it. Only as many as
# a hook could plausibly have.
NUMBER_WORDS = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6,
    "seven": 7, "eight": 8, "nine": 9, "ten": 10, "eleven": 11, "twelve": 12,
}

# `^#   4. ...` -- an entry in the header's numbered list.
#
# The indent is `\s+` rather than the three literal spaces this used to
# require, because the header right-aligns its numbers: gate 10 is written
# `#  10. `, with two spaces, so a fixed three-space indent silently stopped
# seeing the list's last entry the moment a second digit arrived. That failed
# in the least useful way available -- the suite reported the *header* as wrong
# ("nine gates listed, ten implemented") when the header was right and the
# regex reading it was not, which is a false accusation aimed at exactly the
# line a reader would then have "fixed" by renumbering a correct list.
HEADER_ENTRY_RE = re.compile(r"^#\s+(\d+)\. ", re.MULTILINE)

# `# Gate 4: ...` or `# Gate 7 - ...`. The separator is required: the file also
# contains the sentence "Gate 9 needs them and cannot re-read stdin", which is
# prose about a gate rather than the start of one.
GATE_SECTION_RE = re.compile(r"^# Gate (\d+)\s*[:\u2014-]", re.MULTILINE)

# A bypass that is actually wired up, as opposed to one merely named in prose.
BYPASS_IMPL_RE = re.compile(r'\$\{(ALLOW_[A-Z0-9_]+):-0\}"\s*=\s*"1"')

_FAILURES: list[str] = []

for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(errors="replace")
    except (AttributeError, ValueError):
        pass


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def hook_text():
    with open(HOOK, encoding="utf-8") as fh:
        return fh.read()


def code_only(text):
    """The hook with whole-line comments removed.

    Needed because this file explains itself at length, and an assertion that
    a flag is present will happily match the comment that describes the flag.
    That is not hypothetical: the `--head "$sha"` check below passed against a
    mutant that had removed the flag from the actual invocation, because the
    paragraph above it quotes the flag while arguing for it. A structural test
    satisfied by prose tests the prose.

    Whole-line comments only: trailing `#` inside a heredoc or a string is not
    a comment, and stripping it would corrupt the very text some of the
    assertions above are about.
    """
    return "\n".join(ln for ln in text.splitlines()
                     if not ln.lstrip().startswith("#"))


def test_the_hook_exists_and_is_a_shell_script(text):
    check("pre-push has a shebang", text.startswith("#!"), True)
    check("pre-push runs under set -u", "\nset -u\n" in text, True)


def test_the_header_count_matches_the_numbered_list(text):
    """The rot that prompted this file: "Seven gates" over eight of them."""
    m = re.search(r"^# (\w+) gates at the push boundary", text, re.MULTILINE)
    if not check("the header states a count", m is not None, True):
        return
    word = m.group(1).lower()
    if not check(f"the count {word!r} is a number word", word in NUMBER_WORDS, True):
        return
    entries = [int(n) for n in HEADER_ENTRY_RE.findall(text)]
    check("the stated count matches the numbered list",
          NUMBER_WORDS[word], len(entries))


def test_the_numbered_list_matches_the_implemented_gates(text):
    entries = [int(n) for n in HEADER_ENTRY_RE.findall(text)]
    sections = [int(n) for n in GATE_SECTION_RE.findall(text)]
    check("the list is numbered 1..n with no gaps or repeats",
          entries, list(range(1, len(entries) + 1)))
    check("every listed gate has a section, and vice versa",
          sorted(sections), sorted(entries))


def test_every_gate_has_its_own_bypass(text):
    """One per gate, all distinct.

    The header's argument: "each has its own bypass variable, so silencing one
    never silences another". A shared variable would satisfy a count and break
    that promise, so identity is checked, not just arity.
    """
    entries = HEADER_ENTRY_RE.findall(text)
    bypasses = BYPASS_IMPL_RE.findall(text)
    check("there are as many wired-up bypasses as gates",
          len(bypasses), len(entries))
    check("no two gates share a bypass",
          len(set(bypasses)), len(bypasses))


def test_every_bypass_is_named_in_a_failure_message(text):
    """A blocked author must be told the way out, or they reach for --no-verify.

    `--no-verify` turns off all nine at once, so a gate that fails without
    naming its own escape hatch endangers the other eight.
    """
    missing = []
    for var in sorted(set(BYPASS_IMPL_RE.findall(text))):
        # The invocation form the heredocs use: `ALLOW_X=1 git push ...`.
        if not re.search(rf"{var}=1 git push", text):
            missing.append(var)
    check("every bypass is shown as a `git push` invocation", missing, [])


def test_there_is_no_master_bypass(text):
    """The header argues against ALLOW_EVERYTHING; this is what enforces it."""
    check("ALLOW_EVERYTHING is discussed but never implemented",
          "ALLOW_EVERYTHING" in [m for m in BYPASS_IMPL_RE.findall(text)], False)


def test_the_request_deletion_gate_judges_the_commit(text):
    """Gate 9 specifically: `--head` is the whole reason it is correct here.

    Without it the checker diffs the working tree, and a commit that deletes a
    request passes whenever the file has since been restored and staged -- the
    gate's own purpose, missed. A refactor that "simplified" the call by
    dropping the flag would leave every test above green.
    """
    code = code_only(text)
    check("gate 9 runs the deletion checker",
          "check-requests-not-deleted.py" in code, True)
    check("gate 9 passes --head so it judges the pushed commit",
          re.search(r'--head\s+"\$sha"', code) is not None, True)
    # As a *condition*, not merely present: `--selftest` appears in the gate's
    # prose and its failure message too, so a bare "is the string here"
    # assertion stays green when the guard itself is removed.
    #
    # The invocation goes through `run_checker` (design-decisions.md 746), which
    # is what tells a checker that *found* something from one that fell over.
    # Matching that helper rather than a bare `"$py"` is deliberate: reverting a
    # gate to the direct call is the regression this line should catch, and
    # `test-pre-push-run-checker.py` group 7 asserts the same rule for all
    # eleven gates at once.
    #
    # `"$py"` is matched between the label and the script because the helper
    # moved to `scripts/run-checker.sh`, shared with `boot-test.sh`, and takes
    # the interpreter as an ordinary argument rather than reading an ambient
    # `$py` from the caller's scope. boot-test declares `local py` inside each
    # of its gates, so the ambient form would have worked there only for as long
    # as no call site was a subshell.
    check("gate 9 self-tests the checker before believing it",
          re.search(r'if\s*!\s*run_checker\s+\S+\s+"\$py"\s+"\$reqdel"\s+--selftest',
                    code)
          is not None,
          True)
    check("gate 9 collects the refs being pushed",
          "pushed_shas" in code, True)


def test_no_gate_hands_a_push_sized_list_to_argv(text):
    """A scope derived from the pushed files must not travel as arguments.

    Gate 11 used to expand its crate list into `$doclink_dirs` on the command
    line. That is fine until a push is wide: 2,568 changed `.rs` files name
    2,568 directories, which is 64,862 bytes against Windows' 32,767-character
    limit, and on 2026-09-02 the gate died with "Argument list too long" before
    reading a single file. The hook was right to refuse -- exit 126 is not a
    verdict -- so the effect was a lane that could not push at all.

    The fixed shape is a file: the list is written to `$doclink_list` and read
    with `--paths-from`, which has no such ceiling. Asserted here as well as
    behaviourally in `test-pre-push-doclinks-gate.py`, because the behavioural
    suite needs a fixture of several hundred crates to see it and this one
    catches the reintroduction instantly.

    Gate 5's `$bins` is deliberately not covered: its scope is bounded by
    `userspace/coreutils/src/bin/`, about 124 short names, which cannot reach
    the limit no matter how wide the push. Gate 7's file list is already a
    file, and its rustfmt calls are already batched at 64.
    """
    code = code_only(text)
    check("gate 11 reads its scope from a file",
          re.search(r'--paths-from\s+"\$doclink_list"', code) is not None, True)
    check("gate 11 no longer expands its scope into argv",
          re.search(r'--check\s+\$doclink_dirs', code) is not None, False)
    check("the scope file is cleaned up",
          re.search(r'rm -f "\$doclink_list"', code) is not None, True)


def main():
    text = hook_text()
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    # A discovery mechanism that discovers nothing looks exactly like a suite
    # that passes. Assert a floor, as the sibling suites do.
    if len(tests) < 6:
        print(f"FATAL: test discovery found only {len(tests)} tests; the suite "
              f"has at least 6. Discovery is broken, not the code.")
        return 1
    for name, fn in tests:
        params = inspect.signature(fn).parameters
        avail = {"text": text}
        fn(**{p: avail[p] for p in params if p in avail})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all {len(tests)} pre-push-gate tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
