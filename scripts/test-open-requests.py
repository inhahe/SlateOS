#!/usr/bin/env python3
"""Regression tests for `scripts/open-requests.py`.

Run: `python scripts/test-open-requests.py` (0 = pass, 1 = fail). No pytest
dependency, matching the other suites in this directory, so it runs from a bare
checkout and from `scripts/boot-test.sh`.

Why this report needs tests, and why they are shaped the way they are
--------------------------------------------------------------------

`open-requests.py` answers "what is still open for my lane?", and every lane
reads it at the start of every task. Its two failure directions do not cost the
same, and the tests are weighted accordingly:

* A finished request reported **open** costs one glance at a file.
* An open request reported **done** *disappears*. Nothing else in the tree
  looks for it -- that is the entire premise of the report -- so the work is
  not delayed, it is lost, and no reader can tell the difference between "the
  queue is clear" and "the queue is broken".

So the suite is mostly a list of ways the second thing has happened or could.
Three of them are regressions from real files in `requests/`, not inventions:

* **A status that wraps.** The classifier used to bound its search with
  ``[^\\n]{0,80}`` -- a *line*. `landed for ask 1, but ask 2 is blocked` was
  read as open, and the identical sentence with a newline before `blocked` was
  read as **done**. A verdict that depends on typesetting is not a verdict.
* **A negated done word.** `**Status:** not fixed` and `**Status:** not yet
  resolved` both matched `fixed`/`resolved` and reported done, which is a
  status line saying in plain English that the work is unfinished being read as
  finished. `b-a-raw-nic-claim-tests-race-and-the-reader-is-the-writer.md` was
  sitting in the tree in exactly that state.
* **A filename that contains a vocabulary word.** `\\bopen\\b` matches inside
  `open-questions.md`, which this project writes constantly, so a request
  stamped `FOLDED IN` that merely said where the value went was reported open.

The asymmetry between the two vocabularies is itself under test, in both
directions: `test_negation_does_not_apply_to_open_words` exists because making
the guard symmetric is the obvious "cleanup" and it is wrong -- it would let
`no longer blocked` clear a request, trading the cheap failure for the
expensive one.

Two tests run against the **real** `requests/` directory rather than a fixture,
because two of the properties here are facts about the corpus that a fixture
cannot notice changing: that the dropbox still parses at all, and that the
vocabulary still covers the markers lanes actually write. A gate whose fixtures
all pass while the real directory has drifted out from under it is the specific
way this kind of tool goes quietly wrong.
"""

from __future__ import annotations

import importlib.util
import inspect
import os
import re
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "open-requests.py")
REQUESTS_DIR = os.path.join(REPO_ROOT, "requests")
ROADMAP = os.path.join(REPO_ROOT, "roadmap.md")

CHECK = "\u2705"        # the tick lanes put in front of a landed marker
HOURGLASS = "\u23f3"    # and in front of a partial one

# The fixtures below quote the ticks, hourglasses and em dashes lanes really
# write, and several labels echo the fixture back. `scripts/boot-test.sh`
# captures this suite's output with `$(...)`, and on Windows a pipe gets the
# *locale* encoding -- cp1252 here, which has no U+2705 -- so printing a label
# raised `UnicodeEncodeError` and the suite exited 1 with a charmap traceback
# where its report should have been. That is the worst direction for a harness
# to fail in: indistinguishable from a real failure, and it destroys the
# diagnosis at the one moment the diagnosis is what you came for.
#
# `errors="replace"` rather than forcing UTF-8: the tick becomes `?`, readable
# on any console, where UTF-8 bytes sent to a cp1252 terminal are mojibake. A
# suite's job is to report, so it must never be the thing that cannot print.
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(errors="replace")
    except (AttributeError, ValueError):
        pass  # not a real text stream (redirected to something exotic)

_FAILURES: list[str] = []


def load_module():
    """Import open-requests.py by path (the name is not an identifier)."""
    spec = importlib.util.spec_from_file_location("openrequests", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["openrequests"] = module
    spec.loader.exec_module(module)
    return module


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def verdict(mod, text):
    """``"open"``, ``"done"``, or ``"none"`` for a status snippet.

    Collapses the tuple so a test reads as the sentence it is asserting, and so
    a failure prints a word rather than a half-line of matched text.
    """
    got = mod.status_verdict(text)
    if got is None:
        return "none"
    return "open" if got[0] else "done"


# --------------------------------------------------------------------------
# The bug that started this: a verdict that depended on line wrapping
# --------------------------------------------------------------------------

def test_wrapped_status_reads_the_same_as_unwrapped(mod):
    """The same sentence, wrapped and not, must classify identically."""
    unwrapped = "**Status:** landed for ask 1, but ask 2 is blocked on lane B."
    wrapped = "**Status:** landed for ask 1, but ask 2 is\nblocked on lane B."
    check("wrapped status: unwrapped form is open",
          verdict(mod, unwrapped), "open")
    check("wrapped status: wrapped form is open too (was 'done')",
          verdict(mod, wrapped), "open")
    check("wrapped status: the two agree",
          verdict(mod, wrapped), verdict(mod, unwrapped))


def test_open_outranks_done_in_the_same_block(mod):
    """A partial status names both outcomes; the unfinished half must win."""
    text = (f"**Status:** {HOURGLASS} ask 1 landed 2026-08-29 by lane A; "
            f"ask 2 blocked on lane B.")
    check("partial status ranks open above landed", verdict(mod, text), "open")


def test_open_outranks_done_across_two_blocks(mod):
    """A header stamp plus an appended reply section is two blocks, not one."""
    text = ("**Status:** landed.\n\nsome prose\n\n"
            "**Status:** reopened, the fix regressed.")
    check("second block can reopen the first", verdict(mod, text), "open")


# --------------------------------------------------------------------------
# Negation: a done word with "not" in front of it is not a completion
# --------------------------------------------------------------------------

def test_negated_done_words_are_not_done(mod):
    for text in ("**Status:** not fixed.",
                 "**Status:** not yet resolved.",
                 "**Status:** never landed.",
                 "**Status:** this has not landed.",
                 "**Status:** cannot be implemented as asked."):
        check(f"negated done word is open: {text[12:40]!r}",
              verdict(mod, text), "open")


def test_negation_stops_at_a_clause_boundary(mod):
    """`never landed; lane B declined` is declined, which is an outcome.

    The guard looks backwards only within the current clause. Without that
    bound a single early "not" would suppress every done word in a paragraph,
    which turns the report into one that never marks anything finished -- safe,
    useless, and indistinguishable from a broken vocabulary.
    """
    check("negation does not leak past ';'",
          verdict(mod, "**Status:** never landed; lane B declined to take it."),
          "done")
    check("negation does not leak past '.'",
          verdict(mod, "**Status:** not urgent. Landed 2026-08-29 by lane A."),
          "done")


def test_negation_does_not_apply_to_open_words(mod):
    """Deliberately asymmetric -- see the module docstring.

    `no longer blocked` stays open. Making the guard symmetric would read it as
    a clearance, which is the expensive failure direction; a spurious open
    costs a glance, a spurious done loses the request.
    """
    check("'no longer blocked' stays open",
          verdict(mod, "**Status:** no longer blocked, landed by lane A."),
          "open")


# --------------------------------------------------------------------------
# Word boundaries: a filename is not a status
# --------------------------------------------------------------------------

def test_open_questions_filename_is_not_an_open_marker(mod):
    text = (f"**Status:** {CHECK} FOLDED IN 2026-08-18 by lane A. The refill "
            f"rate is in `open-questions.md` now.")
    check("`open-questions.md` does not make a request open",
          verdict(mod, text), "done")


def test_hyphen_on_the_left_still_counts(mod):
    """`still-open` means open: the tail of the compound carries the meaning."""
    check("'still-open' is an open marker",
          verdict(mod, "**Status:** still-open, awaiting lane B."), "open")


def test_a_word_inside_a_longer_word_does_not_count(mod):
    check("'open' inside a longer word is not an open marker",
          verdict(mod, "**Status:** landed; the reopening was a false alarm."),
          "done")


# --------------------------------------------------------------------------
# The window: near the marker, measured in characters
# --------------------------------------------------------------------------

def test_prose_about_another_request_is_not_this_ones_status(mod):
    """The real `b-a-cap-enumerating-query-syscall.md` shape.

    Stamped LANDED, then 300-odd characters later it mentions that a *different*
    step is still open. Reading the whole paragraph reported this finished
    request as open on the strength of a word about another one.
    """
    text = (f"**Status:** {CHECK} LANDED 2026-08-15 by lane A, answered in "
            f"`x.md`. " + "filler text. " * 30 +
            "(step 3 is a separate, still open request.)")
    check("far-away 'open' does not reopen a landed request",
          verdict(mod, text), "done")


def test_window_is_characters_not_lines(mod):
    """A done word pushed onto line 2 by wrapping is still inside the window."""
    text = "**Status:** reply to `c-a-expose-block-devices.md` \u2014 all of it\nis done."
    check("done word on the second line still counts",
          verdict(mod, text), "done")


# --------------------------------------------------------------------------
# Vocabulary
# --------------------------------------------------------------------------

def test_markers_the_lanes_actually_write(mod):
    """Every one of these is in `requests/` today."""
    for text, want in (
        (f"**Status:** {CHECK} LANDED 2026-08-29 by lane A.", "done"),
        ("**Status:** fulfilled. Reply to `c-a-two-inflates.md`.", "done"),
        (f"**Status:** {CHECK} **CONSUMED 2026-08-24 by lane C.**", "done"),
        (f"**Status:** {CHECK} FOLDED IN 2026-08-18 by lane A.", "done"),
        (f"**Status:** {CHECK} CLOSED 2026-08-29 by lane A.", "done"),
        ("**Status:** open", "open"),
        ("**Status:** unknown \u2014 restored, awaiting a stamp.", "open"),
    ):
        check(f"vocabulary: {text[12:44]!r}", verdict(mod, text), want)


ROADMAP_ANCHOR = "**Write the status in words the script knows.**"

ROADMAP_ROW_RE = re.compile(r"^\|\s*(still work|finished)\s*\|(.*)\|\s*$",
                            re.MULTILINE)


def _normalise(word):
    """Collapse a word to what the classifier actually distinguishes.

    The code writes `wont ?fix` and `won't ?fix` where the table writes
    `wontfix`; all three are one word to a reader and to the matcher. Spaces,
    apostrophes and the optional-space `?` are therefore not differences.
    """
    return word.replace("?", "").replace("'", "").replace("\u2019", "") \
               .replace(" ", "").lower()


def test_the_roadmap_vocabulary_table_matches_the_code(mod):
    """`roadmap.md` rule 2 lists the words; this asserts it lists *these* words.

    A documented word the tool does not know is worse than no documentation at
    all. The author writes it in good faith, the status reads as *unrecognised*,
    and the request is reported open forever with nothing anywhere saying that
    the word was the problem -- the doc actively caused the failure it was
    written to prevent. Drift the other way is quieter and no better: a word the
    classifier honours that nobody was ever told about is a word only the person
    who added it can use.

    Both directions are checked, and then every documented word is put through
    the classifier, because set equality only proves the two lists agree -- not
    that either one is true. `partial(ly)` in the table was rewritten to
    `partial` `partially` so that this test can take it literally.
    """
    with open(ROADMAP, encoding="utf-8") as fh:
        roadmap = fh.read()

    anchor = roadmap.find(ROADMAP_ANCHOR)
    if not check("roadmap rule 2 still documents the vocabulary",
                 anchor >= 0, True):
        return
    region = roadmap[anchor:anchor + 1500]

    rows = dict(ROADMAP_ROW_RE.findall(region))
    if not check("both vocabulary rows are present",
                 sorted(rows), ["finished", "still work"]):
        return

    raw = {name: re.findall(r"`([^`]+)`", cells) for name, cells in rows.items()}
    documented = {name: {_normalise(w) for w in toks} for name, toks in raw.items()}
    coded = {
        "finished": {_normalise(w) for w in mod.DONE_WORDS.split("|")},
        "still work": {_normalise(w) for w in mod.OPEN_WORDS.split("|")},
    }

    for name in ("finished", "still work"):
        # A row that failed to parse is an empty set, which is a subset of
        # everything and would pass one of the two checks below in silence.
        check(f"the {name!r} row parsed to something",
              len(documented[name]) >= 4, True)
        check(f"no {name!r} word in the code is missing from the table",
              sorted(coded[name] - documented[name]), [])
        check(f"no {name!r} word in the table is unknown to the code",
              sorted(documented[name] - coded[name]), [])

    for name, want in (("finished", "done"), ("still work", "open")):
        wrong = [w for w in raw[name]
                 if verdict(mod, f"**Status:** {w}") != want]
        check(f"every {name!r} word in the table really classifies as {want}",
              wrong, [])

    # The table also quotes the window size. It is the one number in rule 2 a
    # lane could act on -- "keep the verdict in the first line or so" -- so it
    # has to be the number the code uses.
    stated = re.search(r"first ~(\d+)\s+characters after", region)
    if check("the table states a window size", stated is not None, True):
        check("the stated window is the coded one",
              int(stated.group(1)), mod.STATUS_WINDOW)


def test_no_status_line_at_all_is_open(mod):
    """Absence is not evidence of completion."""
    check("a file with no **Status:** is open",
          verdict(mod, "# A title\n\nSome prose and no marker anywhere."),
          "none")


def test_unrecognised_wording_is_open_and_says_so(mod):
    """An unclassifiable status must be open, and must not read as 'no marker'.

    The two call for different fixes -- one needs a stamp, the other needs a
    different word -- and a report that conflates them trains its reader to
    treat both as noise.
    """
    got = mod.status_verdict("**Status:** who can say, really")
    check("unrecognised status is open", got[0], True)
    check("unrecognised status says which it is",
          got[1].startswith("unrecognised status:"), True)


# --------------------------------------------------------------------------
# Against the real corpus
# --------------------------------------------------------------------------

def test_the_real_dropbox_still_classifies(mod):
    """Every well-named file yields a verdict without raising."""
    import pathlib
    files = sorted(pathlib.Path(REQUESTS_DIR).glob("*.md"))
    check("the dropbox is not empty", len(files) > 50, True)
    bad = []
    for path in files:
        try:
            is_open, reason, title = mod.classify(path)
        except Exception as exc:                      # noqa: BLE001 - reported
            bad.append(f"{path.name}: {exc}")
            continue
        if not isinstance(is_open, bool) or not reason:
            bad.append(f"{path.name}: malformed verdict")
    check("every request classifies cleanly", bad, [])


def test_the_corpus_is_not_all_one_verdict(mod):
    """A classifier stuck on one answer passes every fixture above.

    Both of the degenerate classifiers -- "everything is open" and "everything
    is done" -- satisfy a suite made only of positive cases for one side. This
    is the test that notices, and it is why it runs on the real directory: the
    proportions are a property of the corpus, not of a fixture.
    """
    import pathlib
    verdicts = [mod.classify(p)[0]
                for p in sorted(pathlib.Path(REQUESTS_DIR).glob("*.md"))]
    check("some requests are open", any(verdicts), True)
    check("some requests are done", not all(verdicts), True)
    # A report where nearly everything is open is the "vocabulary drifted"
    # failure: lanes kept writing markers and the script stopped knowing them.
    open_share = sum(verdicts) / len(verdicts)
    check(f"open share is plausible ({open_share:.0%} of {len(verdicts)})",
          open_share < 0.5, True)


def main():
    mod = load_module()
    tests = [(name, fn) for name, fn in list(globals().items())
             if name.startswith("test_") and callable(fn)]
    # A discovery mechanism that discovers nothing looks exactly like a suite
    # that passes. Assert a floor, as the sibling suites do.
    if len(tests) < 15:
        print(f"FATAL: test discovery found only {len(tests)} tests; the suite "
              f"has at least 15. Discovery is broken, not the code.")
        return 1
    for name, fn in tests:
        params = inspect.signature(fn).parameters
        avail = {"mod": mod}
        fn(**{p: avail[p] for p in params if p in avail})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all {len(tests)} open-requests tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
