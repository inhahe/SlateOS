#!/usr/bin/env python3
"""Regression tests for `scripts/ki_dupes.py` and its entry/subsection split.

Run: `python scripts/test-ki-dupes.py` (0 = pass, 1 = fail). No pytest
dependency, matching the other suites in this directory, so it runs from a bare
checkout and from `scripts/boot-test.sh`.

What went wrong, and why it needs a test rather than just a fix
--------------------------------------------------------------

`ki_dupes.py` enforces one property: no entry appears in both
`known-issues.md` and its resolved archive. It got that by comparing every
heading `ki_split.parse` returned -- and `parse` returns *every* heading,
including the `###` subsection headings inside entries.

Those subsection titles are ordinary prose, and they repeat constantly across
unrelated entries: "Test", "The shape of the fix", "What was and was not
reachable", "Why this one and not the other three". So the tool reported
`### Test` as an entry living in both files -- one copy a subsection of a PCI
interrupt entry here, the other a subsection of an HTML-escaping entry in the
archive. Two headings that share a word, and nothing more.

**A permanent false positive is worse than no check.** `ki_dupes.py` exits 1
when it finds anything, and `known-issues.md`'s own instructions say to run it
after any merge that touched either file. Exiting 1 unconditionally means a
*real* resurrection -- the thing it was written to catch, which git performs
silently during a merge -- arrives as one more line in output the reader has
already learned to skim past.

The split cannot be done by heading level, which is why it needs testing
rather than an obvious one-liner. Both files mix `###` entries with `###`
subsections: lane B's `TD-OILS-*` entries live at `###`, as does everything
archived under the older `F19` / `W2` / `B-CWD1` numbering, while `##` entries
own `###` subsections. The only thing that separates them is whether the
heading opens with an entry *id*, and ids come in two shapes from two eras.

So the tests below are mostly a table of real heading strings taken from the
two live files, asserted in both directions. The cases that matter are the
near-misses at the boundary:

* `B-VFS-…-IS-12x-OVER-TARGET` -- an id containing a lower-case letter, which
  a strict `[A-Z-]+` id pattern rejects.
* `TD-OILS-A-BUILTIN-DOES-NOT-ANSWER-ITS-OWN---HELP` -- an id ending in a run
  of hyphens, which a pattern anchored on a clean delimiter rejects.
* `Follow-up 2026-08-16: …` -- prose that *is* hyphenated, and so is admitted
  by any rule that tests only for a hyphen.
* `AUDIT 2026-08-14 — …` -- a subsection opening with an all-caps word, which
  is admitted by any rule that tests only for upper case.

Each of those four was a wrong answer from an earlier version of the rule.
"""

from __future__ import annotations

import inspect
import os
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO_ROOT, "scripts"))

import ki_dupes  # noqa: E402
import ki_split  # noqa: E402

_FAILURES: list[str] = []


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def check_true(label, got):
    return check(label, bool(got), True)


# --------------------------------------------------------------------------
# The id/prose split, against real headings from the two live files.
# --------------------------------------------------------------------------

# Headings that open with an entry id. Drawn from both numbering eras.
_ID_HEADINGS = [
    # Current era: long hyphen-joined upper-case ids.
    "B-MOUNT-ACCEPTS-UNREACHABLE-MOUNT-POINTS. `Vfs::mount` succeeds when",
    "TD-POSIX-WAITID-IS-NARROWER-THAN-THE-KERNEL-COULD-MAKE-IT. `waitid`",
    "TD-OILS-A-BUILTIN-DOES-NOT-ANSWER-ITS-OWN---HELP. `cd --help` says",
    "B-VFS-STAT-ROOT-IS-12x-OVER-TARGET-AND-THE-DCACHE-IS-NOT-WHY \u2014 2026-08-14",
    "TOOLING-BASH-5.2.37-SOURCE. A local copy of the reference shell's source",
    # A lane tag in front of the id.
    "[A] TD-CONSOLE-ECHO-RUNS-IN-HARD-IRQ-CONTEXT. Keyboard echo renders",
    "[A] B-CONSOLE-LOCK-IS-TAKEN-FROM-A-HARD-IRQ-WITH-A-PLAIN-LOCK. The key",
    # Older era: short letter-and-digit tags.
    "F19. rmap self-test used low fake frame addresses",
    "W2. Deferred benchmark suite livelocks in `bench_pick_next`",
    "B-COMPACT1. Memory-compaction self-test panicked",
    "B-CWD1. Linux-ABI relative path resolution ignored the per-process cwd",
]

# Headings that are prose subsections inside an entry.
_PROSE_HEADINGS = [
    "Test",
    "Fixed",
    "The shape of the fix",
    "The proper fix",
    "What was and was not reachable",
    "Why this one and not the other three",
    "Two lessons from the break-testing, not the fix",
    "Also fixed: the boundary was a constant",
    "Measured, both sides",
    "clipmanager \u2014 the worst of the three, because of what the field holds",
    "mindmap \u2014 no importer, so the reader is a person",
    "The same bug in `apps/backup`'s manifest reader",
    # Hyphenated prose: admitted by any rule that only looks for a hyphen.
    "Follow-up 2026-08-16: the gate is calibrated",
    # Opens all-caps: admitted by any rule that only looks for upper case.
    "[A] AUDIT 2026-08-14 \u2014 the softirq / hard-IRQ shared-lock class is clean",
    # Opens with a capital, no hyphen, no digit.
    "A trailing `| tail` swallows the exit code too",
]


def test_entry_ids_are_recognised():
    wrong = [h for h in _ID_HEADINGS if not ki_split.opens_with_entry_id(h)]
    check("every entry id is recognised as one", wrong, [])


def test_prose_headings_are_not_entry_ids():
    wrong = [h for h in _PROSE_HEADINGS if ki_split.opens_with_entry_id(h)]
    check("no prose subsection is read as an entry id", wrong, [])


def test_a_level_two_heading_is_always_an_entry():
    """`##` counts regardless of its title, which is often plain prose."""
    prose = ki_split.Entry(
        level=2,
        title="`cargo test -p indexer` tests lane B's crate, not lane C's (lane C)",
        start=0,
    )
    check_true("a ## heading with a prose title is still an entry", prose.is_entry)


def test_a_level_three_prose_heading_is_not_an_entry():
    sub = ki_split.Entry(level=3, title="Test", start=0)
    check("a ### prose heading is not an entry", sub.is_entry, False)


def test_a_level_three_id_heading_is_an_entry():
    ent = ki_split.Entry(
        level=3, title="TD-OILS-A-BUILTIN-DOES-NOT-ANSWER-ITS-OWN---HELP. `cd`",
        start=0,
    )
    check_true("a ### id heading is an entry", ent.is_entry)


# --------------------------------------------------------------------------
# find_duplicates, against synthetic files.
#
# Synthetic rather than the real pair: the real files are ~96 000 and ~53 000
# lines and are edited by all three lanes, so a test asserting anything about
# their contents would fail for reasons unrelated to this code. What the real
# files are for is `ki_dupes.py` itself, which the boot gate runs.
# --------------------------------------------------------------------------

def _write(tmpdir, name, body):
    path = os.path.join(tmpdir, name)
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(body)
    return path


# Two unrelated entries that happen to share a subsection title. This is the
# exact shape that produced the false positive.
_LIVE_WITH_SHARED_SUBSECTION = """# Known Issues

## `A-PCI-INTX-STORM-IS-NOT-CLEARED` (lane A)

**Status:** OPEN

Some prose about PCI.

### Test

`pci::self_test_intx`, run from `pci::self_test` on every boot.
"""

_ARCHIVE_WITH_SHARED_SUBSECTION = """# Known Issues (resolved)

## `C-EXPORT-DOES-NOT-ESCAPE-TAGS` (lane C)

**Status:** FIXED 2026-08-01

Some prose about HTML escaping.

### Test

`no_text_field_can_inject_a_tag_into_the_export` drives one payload.
"""


def test_a_shared_subsection_title_is_not_a_duplicate(tmpdir):
    live = _write(tmpdir, "live.md", _LIVE_WITH_SHARED_SUBSECTION)
    archive = _write(tmpdir, "arch.md", _ARCHIVE_WITH_SHARED_SUBSECTION)
    dupes = ki_dupes.find_duplicates(live, archive)
    check("two entries sharing a `### Test` subsection are not duplicates",
          [d[0].title for d in dupes], [])


# The real thing: one entry present in both files.
_LIVE_WITH_RESURRECTED = """# Known Issues

## `A-PCI-INTX-STORM-IS-NOT-CLEARED` (lane A)

**Status:** OPEN

### Test

Prose.

### B-CWD1. Linux-ABI relative path resolution ignored the per-process cwd

The live copy, which is the stale one.
"""

_ARCHIVE_WITH_RESURRECTED = """# Known Issues (resolved)

### B-CWD1. Linux-ABI relative path resolution ignored the per-process cwd

The live copy, which is the stale one.

And a follow-up paragraph the live copy does not have.
"""


def test_an_entry_in_both_files_is_reported(tmpdir):
    live = _write(tmpdir, "live.md", _LIVE_WITH_RESURRECTED)
    archive = _write(tmpdir, "arch.md", _ARCHIVE_WITH_RESURRECTED)
    dupes = ki_dupes.find_duplicates(live, archive)
    check("a genuinely resurrected entry is reported", len(dupes), 1)
    if dupes:
        check_true("...and it is named by its id",
                   dupes[0][0].title.startswith("B-CWD1."))


def test_the_relation_names_which_copy_is_longer(tmpdir):
    """The report has to say which copy to keep, not just that there are two."""
    live = _write(tmpdir, "live.md", _LIVE_WITH_RESURRECTED)
    archive = _write(tmpdir, "arch.md", _ARCHIVE_WITH_RESURRECTED)
    dupes = ki_dupes.find_duplicates(live, archive)
    if not dupes:
        check("a duplicate was found to describe", False, True)
        return
    relation = ki_dupes._relation(*dupes[0])
    check_true("the archive is reported as the superset",
               relation.startswith("archive is a superset"))


def test_a_level_two_entry_in_both_files_is_reported(tmpdir):
    """`##` entries have prose titles, so they exercise the other branch."""
    body = """# Known Issues

## `cargo test -p indexer` tests lane B's crate, not lane C's (lane C)

Prose.
"""
    live = _write(tmpdir, "live.md", body)
    archive = _write(tmpdir, "arch.md", body.replace("Known Issues",
                                                     "Known Issues (resolved)"))
    dupes = ki_dupes.find_duplicates(live, archive)
    check("a duplicated ## entry with a prose title is reported", len(dupes), 1)


def test_clean_files_report_nothing(tmpdir):
    live = _write(tmpdir, "live.md", _LIVE_WITH_SHARED_SUBSECTION)
    archive = _write(tmpdir, "arch.md", _ARCHIVE_WITH_SHARED_SUBSECTION)
    check("disjoint files have no duplicates",
          ki_dupes.find_duplicates(live, archive), [])


# --------------------------------------------------------------------------

def main():
    tests = [(n, f) for n, f in sorted(globals().items())
             if n.startswith("test_") and callable(f)]
    if len(tests) < 10:
        print(f"FATAL: test discovery found only {len(tests)} tests; the suite "
              f"has at least 10. Discovery is broken, not the code.")
        return 1
    for name, fn in tests:
        params = inspect.signature(fn).parameters
        with tempfile.TemporaryDirectory() as tmpdir:
            avail = {"tmpdir": tmpdir}
            missing = [p for p in params if p not in avail]
            if missing:
                print(f"FATAL: {name} wants {missing}, which the harness does "
                      f"not supply. Fix the harness, do not skip the test.")
                return 1
            fn(**{p: avail[p] for p in params})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all {len(tests)} ki-dupes tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
