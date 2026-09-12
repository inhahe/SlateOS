#!/usr/bin/env python3
"""Refuse a differential harness that does work before sourcing the preamble.

``diff-wsl.sh`` bounds each harness by re-execing it under ``timeout`` (section
1b, design-decisions.md 1020). The re-exec restarts the script FROM THE TOP, so
anything the harness ran before sourcing the preamble runs a second time. For a
variable assignment that is harmless; for a command it is not, and the second
run is silent -- a fixture built twice, a file appended to twice, a counter that
starts at the wrong value.

This is a gate rather than a note because the cost lands on whoever writes the
NEXT harness, who has no reason to know the preamble re-execs at all.

WHAT COUNTS AS SAFE. Assignments, ``export``, ``unset``, ``set`` and comments:
running them twice leaves the same state as running them once. Anything else is
refused unless it is in the baseline below.

THE ONE BASELINED EXCEPTION is ``df-diff.sh``, and it is worth reading before
adding a second. It re-execs itself under ``unshare -mUr`` to get a private
mount namespace, which it must do before the preamble because the preamble's
work belongs inside that namespace. Running that twice would nest a second
namespace -- so it guards itself with an exported ``DF_DIFF_NS``, which is
exactly the shape ``diff-wsl.sh`` uses for ``SLATEOS_DIFF_BOUNDED``. The two
compose: unshare re-execs first and sets its flag, the preamble re-execs second
and sets its own, and neither repeats. A new exception has to argue the same
thing -- that its second execution is a no-op -- not merely that it looks small.
"""

import re
import sys
from pathlib import Path

import selftestflag

SCRIPTS = Path(__file__).resolve().parent

# Re-running any of these leaves the same state as running them once.
IDEMPOTENT = re.compile(
    r"^(?:"
    r"[A-Za-z_][A-Za-z0-9_]*=" 
    r"|export\s"
    r"|unset\s"
    r"|set\s"
    r"|shellcheck\b"
    r")"
)

SOURCE = re.compile(r"^(?:\.|source)\s")  # plus a diff-wsl.sh mention; see is_source()

# See the module docstring. A name here is a promise that its second execution
# is a no-op, not that it is short.
BASELINE = {"df-diff.sh"}


def unquoted_prefix(line, in_quote):
    """Walk a line tracking quote state, so a continuation of a multi-line
    assignment is not mistaken for a command.

    ``interleave-diff.sh`` and ``write-error-diff.sh`` both carry a DIFF_BINS
    list spread over three lines; a line-at-a-time scan reads lines two and
    three as bare words and reports them. That was this checker's first result
    and it was wrong.
    """
    out = []
    for ch in line:
        if in_quote:
            if ch == in_quote:
                in_quote = None
            continue
        if ch in (chr(34), chr(39)):
            in_quote = ch
            continue
        out.append(ch)
    return "".join(out), in_quote


def offenders(path):
    return offenders_in(path.read_text(encoding="utf-8", errors="replace"))


def offenders_in(text):
    in_quote = None
    found = []
    for n, raw in enumerate(text.splitlines(), 1):
        # The source line is matched on RAW text, before any quote handling.
        # It is spelled `. "$(dirname "$0")/diff-wsl.sh"` -- nested quotes --
        # and the stripper below reads the inner pair as a close and a reopen,
        # leaving a fragment with no `diff-wsl.sh` in it. The checker's first
        # run reported that 0 harnesses source the preamble, which is the
        # failure the summary line exists to expose: without it, "0 offenders"
        # and "inspected nothing" print the same word.
        if SOURCE.match(raw.strip()) and "diff-wsl.sh" in raw:
            return found, True
        visible, next_quote = unquoted_prefix(raw, in_quote)
        was_in_quote = in_quote is not None
        in_quote = next_quote
        if was_in_quote:
            continue
        s = visible.strip()
        if not s or s.startswith("#"):
            continue
        if IDEMPOTENT.match(s):
            continue
        found.append((n, raw.strip()[:72]))
    return found, False



# Cases are LISTS OF LINES joined at run time. Writing them as one string
# with newline escapes is how the first attempt at this block died: the
# escapes were interpreted a layer too early and the literals broke open.
SELFTEST = [
    ('a command before the source line is refused',
     ['DIFF_PROG=x', 'mkdir -p fixtures', '. "$(dirname "$0")/diff-wsl.sh"'],
     1, True),
    ('assignments alone are accepted',
     ['DIFF_PROG=x', 'DIFF_BINS="a b"', '. "$(dirname "$0")/diff-wsl.sh"'],
     0, True),
    ('export and unset are accepted',
     ['export A=1', 'unset B C', '. "$(dirname "$0")/diff-wsl.sh"'],
     0, True),
    ('a multi-line quoted assignment is not read as commands',
     ['DIFF_BINS="cat cut', '  nl paste', '  tsort wc"', '. "$(dirname "$0")/diff-wsl.sh"'],
     0, True),
    ('a command AFTER the source line is not this gate\'s business',
     ['DIFF_PROG=x', '. "$(dirname "$0")/diff-wsl.sh"', 'mkdir -p fixtures'],
     0, True),
    ('a script that never sources the preamble is outside the gate',
     ['DIFF_PROG=x', 'mkdir -p fixtures'],
     1, False),
    ('the source spelling counts too',
     ['DIFF_PROG=x', 'source "$(dirname "$0")/diff-wsl.sh"'],
     0, True),
    ('a comment mentioning diff-wsl.sh is not a source line',
     ['# see diff-wsl.sh', 'mkdir -p x', '. "$(dirname "$0")/diff-wsl.sh"'],
     1, True),
]


def selftest():
    bad = 0
    for name, lines, want_n, want_src in SELFTEST:
        found, sourced = offenders_in(chr(10).join(lines) + chr(10))
        ok = (len(found) == want_n) and (sourced == want_src)
        bad += 0 if ok else 1
        print('%-4s %s' % ('ok' if ok else 'FAIL', name))
        if not ok:
            print('       wanted %d offender(s), sources=%s; got %d, sources=%s' % (want_n, want_src, len(found), sourced))
    print()
    print('check-diff-preamble-order selftest: %d case(s), %d failed' % (len(SELFTEST), bad))
    return 1 if bad else 0

def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    # Both spellings, through the shared helper: a mistyped `--self-test`
    # must not fall through to the scan and exit 0, because then the command
    # that asks "is this checker still correct?" answers yes without asking.
    if selftestflag.wants_selftest(argv):
        return selftest()
    harnesses = sorted(SCRIPTS.glob("*-diff.sh"))
    checked = []
    no_preamble = []
    bad = {}
    for h in harnesses:
        found, sourced = offenders(h)
        if not sourced:
            no_preamble.append(h.name)
            continue
        checked.append(h.name)
        if found and h.name not in BASELINE:
            bad[h.name] = found

    for name, found in sorted(bad.items()):
        print("%s runs a command before sourcing the preamble:" % name)
        for n, text in found:
            print("    line %d: %s" % (n, text))
        print("    The preamble re-execs the harness, so each of these runs twice.")
        print()

    # Name the population. A gate that says `ok` without saying what it looked
    # at reads the same whether it inspected 58 files or none, which is how a
    # regex that stopped matching after a refactor goes unnoticed.
    print("check-diff-preamble-order: %d harness(es) source the preamble, "
          "%d baselined, %d offender(s)."
          % (len(checked), len(BASELINE & set(checked)), len(bad)))
    if no_preamble:
        print("  %d do not source it and are outside this gate: %s"
              % (len(no_preamble), " ".join(n[:-len("-diff.sh")] for n in no_preamble)))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
