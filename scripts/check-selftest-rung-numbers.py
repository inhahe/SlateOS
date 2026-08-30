#!/usr/bin/env python3
"""Guard the numbering that the whole self-test log is read by.

Every rung of `kshell::self_test` announces itself on the serial log as

    kshell::self_test <N>: <what this rung pins>

and those numbers are the only handle anyone has on a 103-rung log.  A boot
report says "rung 67 failed"; `known-issues.md` says "rung 79 cleared the 21
`matches!` sites"; a batch entry says "pinned by rung 103".  All of that is
indexing into the log *by number*, so the numbering has to be a real index:
unique, contiguous, and in the order the rungs run.

Three ways it stops being one, all of which have a cheap mechanical signature:

  **duplicate**  Two rungs numbered the same.  This is the likely one, because
  it is what near-simultaneous batches produce: two rungs are drafted against
  the same tip, both take the next free number, and the merge keeps both.  The
  log then has two "rung 103" lines and a failure report naming 103 points at
  either -- so the reference that was supposed to locate the failure is what
  makes it ambiguous.  Nothing else catches this: both rungs compile, both run,
  both pass, and the duplication is visible only by reading 103 banners.

  **gap**  A rung deleted or renumbered leaves a hole.  A hole is not harmful
  on its own, but it silently breaks the one invariant that makes the count
  meaningful -- with no gaps, "the log ended at 103" and "103 rungs ran" are
  the same statement, and a truncated log is obvious.  With a gap they diverge,
  and a boot that died halfway through the rungs looks like a boot that ran
  them all.

  **out of order**  A rung whose number is lower than one above it in the file.
  The log is written in file order, so this makes the printed sequence
  non-monotonic, and "the log reached rung N" stops implying "rungs 1..N ran".

What this gate deliberately does NOT check, because it cannot
--------------------------------------------------------------

It does not check that every block of assertions *has* a banner.  That was the
bug that prompted this script -- rung 103's assertions were committed with no
banner, so they ran and passed while being invisible in the log, and deleting
the entire block would have left the serial output byte-identical.  It is not
statically detectable.  A sibling rung missing its banner and a legitimate
scoping sub-block are the same syntax: a brace-block at the function's top
level containing assertions.  `self_test` has two real ones (a block that saves
and restores `KSHELL_SELFTEST_VAR`, and one that shadows a `PrintfSpec`), and
no rule separates them from an unbannered rung without reading intent.

So the mitigation for that failure mode is not a gate but the log itself: after
adding a rung, read the boot's serial output and confirm the new banner appears
where it should.  A gate that guessed at this would either miss the real cases
or condemn the two legitimate blocks, and DD 635 is explicit that a gate is
narrowed until it can start at zero rather than broadened and given a backlog.

Exit status: 0 clean, 1 findings, 2 could not read the file.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

BANNER = re.compile(r'kshell::self_test (\d+):')

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "kernel" / "src" / "kshell.rs"


def banners(text: str) -> list[tuple[int, int]]:
    """Return [(rung_number, line_number)] in file order."""
    out = []
    for lineno, line in enumerate(text.splitlines(), start=1):
        for m in BANNER.finditer(line):
            out.append((int(m.group(1)), lineno))
    return out


def check(text: str) -> list[str]:
    found = banners(text)
    if not found:
        return ["no `kshell::self_test <N>:` banners found at all -- either the "
                "convention changed or this gate is looking at the wrong file"]

    findings: list[str] = []
    nums = [n for n, _ in found]

    seen: dict[int, int] = {}
    for n, line in found:
        if n in seen:
            findings.append(
                f"rung {n} is announced twice: line {seen[n]} and line {line}. "
                f"A failure report naming rung {n} points at either one."
            )
        else:
            seen[n] = line

    top = max(nums)
    missing = [n for n in range(1, top + 1) if n not in seen]
    if missing:
        shown = ", ".join(str(n) for n in missing[:20])
        more = f" (and {len(missing) - 20} more)" if len(missing) > 20 else ""
        findings.append(
            f"the numbering runs to {top} but {len(missing)} number(s) are "
            f"missing: {shown}{more}. With a gap, 'the log reached rung {top}' "
            f"no longer means 'all {top} rungs ran'."
        )

    for (a, la), (b, lb) in zip(found, found[1:]):
        if b < a:
            findings.append(
                f"rung {b} (line {lb}) is announced after rung {a} (line {la}), "
                f"so the log prints them out of order and reaching rung N stops "
                f"implying rungs 1..N ran."
            )

    return findings


# A fixture, for the same reason every other gate here has one: this checker
# reports a clean tree and a checker that has stopped finding anything in
# identical words.  Each case is one of the three failure modes, plus a control
# that must stay silent.
_FIXTURE_OK = '''
    serial_println!("  kshell::self_test 1: first");
    serial_println!("  kshell::self_test 2: second");
    serial_println!(
        "  kshell::self_test 3: a banner wrapped across lines by rustfmt, which \\
         is how most of the real ones are written"
    );
'''

_FIXTURE_DUP = '''
    serial_println!("  kshell::self_test 1: first");
    serial_println!("  kshell::self_test 2: second");
    serial_println!("  kshell::self_test 2: second again, from a parallel batch");
'''

_FIXTURE_GAP = '''
    serial_println!("  kshell::self_test 1: first");
    serial_println!("  kshell::self_test 3: third, with 2 deleted");
'''

_FIXTURE_ORDER = '''
    serial_println!("  kshell::self_test 1: first");
    serial_println!("  kshell::self_test 3: third");
    serial_println!("  kshell::self_test 2: second, below third");
'''


def self_test() -> int:
    cases = [
        ("clean numbering", _FIXTURE_OK, False),
        ("a duplicated rung number", _FIXTURE_DUP, True),
        ("a hole in the numbering", _FIXTURE_GAP, True),
        ("a rung announced out of order", _FIXTURE_ORDER, True),
    ]
    bad = 0
    for name, text, should_report in cases:
        got = bool(check(text))
        if got != should_report:
            verb = "reported nothing for" if should_report else "reported"
            print(f"SELF-TEST FAIL: {verb} {name}", file=sys.stderr)
            bad += 1
    if bad:
        print(
            f"\n{bad} fixture case(s) disagree with the checker. Its verdict on "
            f"kshell.rs means nothing until they agree.",
            file=sys.stderr,
        )
        return 1
    print("self-test OK: the rung-numbering gate reports all 3 broken fixtures "
          "and not the clean one")
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()

    try:
        text = TARGET.read_text(encoding="utf-8")
    except OSError as exc:
        print(f"cannot read {TARGET}: {exc}", file=sys.stderr)
        return 2

    findings = check(text)
    if not findings:
        total = len(banners(text))
        print(f"self-test rung numbering OK: {total} rung(s), numbered 1..{total} "
              f"with no duplicate, no gap and none out of order")
        return 0

    for f in findings:
        print(f"kshell.rs: {f}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
