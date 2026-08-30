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

import inspect
import os
import re
import sys
import types

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


def load_module(path=SCRIPT, name="openrequests"):
    """Import open-requests.py by path (the name is not an identifier).

    Compiled from the source *text* rather than loaded through
    `spec.loader.exec_module`, and the difference is not stylistic.

    A `SourceFileLoader` consults `__pycache__`, and decides the cache is
    current by comparing the source's `(mtime, size)` against the pair recorded
    in the `.pyc` header. The recorded mtime has **one-second** resolution. So
    two writes to the script inside the same second, producing the same size,
    leave the second one invisible: the loader finds a `.pyc` whose stamp still
    matches and runs the *previous* version.

    That is not theoretical. It was found by mutation-testing this very suite:
    two mutants of `open-requests.py` were reported as surviving -- the suite
    printed all-pass against code that was not on disk -- and both were caught
    the moment they were run one at a time, slowly. The staleness window is
    small, but a suite that can silently validate the wrong bytes is exactly the
    failure this file's docstring is about, only pointed at itself. `compile()`
    on freshly-read text has no cache to be stale.
    """
    with open(path, encoding="utf-8") as fh:
        source = fh.read()
    module = types.ModuleType(name)
    module.__file__ = path
    sys.modules[name] = module
    exec(compile(source, path, "exec"), module.__dict__)  # noqa: S102
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


def test_the_loader_sees_an_edit_made_in_the_same_second(mod):  # noqa: ARG001
    """The suite must test the bytes on disk, not a cached compile of them.

    Reproduces the staleness window described on `load_module`: write a module,
    import it so a `.pyc` gets written, then rewrite it *in the same second and
    at the same size*. A `SourceFileLoader` reuses the cache and returns the old
    value; `load_module` re-reads and returns the new one.

    The same-size requirement is why the two bodies differ only in a digit. If
    this test ever starts passing for the wrong reason, it will be because the
    sizes drifted apart -- so the sizes are asserted equal, and the elapsed time
    is asserted to be under a second, rather than trusted.
    """
    import importlib.util
    import pathlib
    import tempfile
    import time

    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, "stalecheck.py")
        before, after = "VALUE = 111\n", "VALUE = 222\n"
        check("the two bodies are the same size",
              len(before), len(after))

        pathlib.Path(path).write_text(before, encoding="utf-8")
        start = time.time()

        # Prime the cache the way the old loader did, which also writes the
        # .pyc that the second load would go on to reuse.
        spec = importlib.util.spec_from_file_location("stalecheck", path)
        primed = importlib.util.module_from_spec(spec)
        sys.modules["stalecheck"] = primed
        spec.loader.exec_module(primed)
        check("the primed load sees the first version", primed.VALUE, 111)

        pathlib.Path(path).write_text(after, encoding="utf-8")
        elapsed = time.time() - start

        got = load_module(path, name="stalecheck_reloaded").VALUE
        check("load_module sees the rewrite", got, 222)

        # If the rewrite landed in a later second the cache would have been
        # invalidated honestly and this test proved nothing. Say so rather than
        # passing quietly -- a test that only sometimes tests its subject is
        # worse than one that does not exist, because it is believed.
        if elapsed >= 1.0:
            print(f"NOTE  same-second window missed ({elapsed:.2f}s); the stale"
                  " path was not exercised this run")
        sys.modules.pop("stalecheck", None)
        sys.modules.pop("stalecheck_reloaded", None)


def test_the_colon_may_sit_outside_the_bold(mod):
    """`**Status**: FIXED` is the same line as `**Status:** FIXED` to a reader.

    Four files in the dropbox typeset it the second way, including both copies
    of lane B's `known-issues.md was binary to git` notice, which says `FIXED on
    main as of 3ccbfcb99` in plain English. All four reported *no status marker*
    -- the phrasing reserved for a file that never said anything at all -- so
    the reader was told to go and stamp a file that was already stamped.

    Asserted as an equivalence rather than as four literal strings: the claim is
    that the two typesettings classify identically, and a test that says exactly
    that cannot be satisfied by a fix that only handles the example.
    """
    for body in ("FIXED on `main` as of `3ccbfcb99`.",
                 "OPEN \u2014 waiting on lane C.",
                 f"{CHECK} **TAKEN by lane A.**",
                 "needs a decision from you first"):
        inside = verdict(mod, f"**Status:** {body}")
        outside = verdict(mod, f"**Status**: {body}")
        check(f"colon inside == outside for {body[:34]!r}", outside, inside)
        check(f"and it is not 'none' for {body[:34]!r}", outside != "none", True)


def test_a_bare_unbolded_status_word_is_not_a_marker(mod):
    """`Status: done` in running prose must not clear a request.

    The counterweight to the test above. Loosening the marker is the direction
    that costs, because every newly-matched line is a line that might say
    "done" about something else -- and this dropbox is full of prose *about*
    statuses, being a dropbox whose rule 2 is on the subject. The two bold forms
    are unambiguous punctuation a lane chose; an unemphasised `Status:` is a
    word, and a request that quotes one in a sentence would clear itself.
    """
    check("an unbolded marker is not a marker",
          verdict(mod, "The old checker looked for Status: done and missed it."),
          "none")
    check("nor is a single-asterisk one",
          verdict(mod, "*Status:* done"), "none")


def test_the_reason_quotes_the_marker_the_file_actually_wrote(mod):
    """A reason line must be greppable in the file it describes.

    Reporting `**Status:** FIXED …` for a file that says `**Status**: FIXED …`
    sends the reader searching for a string that is not there -- a small lie,
    but this report exists to be acted on, and the act is opening the file.
    """
    got = mod.status_verdict("**Status**: FIXED on `main` as of `3ccbfcb99`.")
    check("the verdict is done", got[0], False)
    check("the reason opens with the marker as written",
          got[1].startswith("**Status**:"), True)
    got2 = mod.status_verdict("**Status:** FIXED on `main` as of `3ccbfcb99`.")
    check("and the usual form still quotes itself",
          got2[1].startswith("**Status:**"), True)

    # Both branches, because they build the string separately. Mutation-testing
    # found this gap: hardcoding the marker in the *open* return survived a
    # suite that only checked the done one.
    got3 = mod.status_verdict("**Status**: OPEN \u2014 waiting on lane C.")
    check("the open verdict is open", got3[0], True)
    check("and the open reason quotes the marker as written",
          got3[1].startswith("**Status**:"), True)

    # And the unrecognised branch, which quotes the block without the marker --
    # asserted so that a change there is a deliberate one.
    got4 = mod.status_verdict("**Status**: needs a decision from you first")
    check("the unrecognised reason names the wording",
          got4[1], "unrecognised status: needs a decision from you first")


def test_the_status_glyphs_are_documented_and_classify(mod):
    """The glyph rule must be in rule 2, and must be the glyphs the code reads.

    Same contract as the word table, for the same reason: a marker the tool
    honours in silence is a marker only its author can use. It is checked
    separately because the glyphs are prose in rule 2 rather than a table row --
    a tick is not a "word" and putting it in the table would make the word test
    assert that `✅` normalises like `landed`.

    The two directions are asserted against the *coded* regexes, not against a
    second copy of the list, so adding a glyph to the code without documenting
    it fails here rather than passing quietly.
    """
    with open(ROADMAP, encoding="utf-8") as fh:
        roadmap = fh.read()
    anchor = roadmap.find(ROADMAP_ANCHOR)
    if not check("roadmap rule 2 is still findable", anchor >= 0, True):
        return
    region = roadmap[anchor:anchor + 1500]

    if not check("rule 2 documents the glyphs at all",
                 "glyphs count as words" in region, True):
        return

    # Every glyph quoted in the paragraph, and every glyph the code knows.
    documented = set(re.findall(r"[\u2705\u23f3\u26d4]", region))
    coded = {"\u2705", "\u23f3", "\u26d4"}
    check("no coded glyph is missing from rule 2",
          sorted(coded - documented), [])
    check("no glyph in rule 2 is unknown to the code",
          sorted(documented - coded), [])

    # And they classify the way the paragraph says they do. Asserting through
    # the classifier is the half that matters: the paragraph could name the
    # right glyph and still describe the wrong verdict.
    for glyph, want in (("\u2705", "done"), ("\u23f3", "open"),
                        ("\u26d4", "open")):
        check(f"a bare {glyph!r} status classifies as {want}",
              verdict(mod, f"**Status:** {glyph} TAKEN by lane A."), want)
        check(f"{glyph!r} is matched by the coded regex",
              bool((mod.DONE_MARK_RE if want == "done"
                    else mod.OPEN_MARK_RE).search(glyph)), True)


def test_a_tick_is_read_when_the_words_around_it_are_not(mod):
    """The three real statuses this rule was added for.

    Each is a genuine completion stamped with a verb the table does not have --
    `TAKEN`, `ACKNOWLEDGED`, `REBUILT`. All three were verified by hand against
    the tree before the rule was added; all three were reported open for weeks
    on the strength of the missing word, with the tick sitting right there.

    They are quoted rather than paraphrased because the point is not that a tick
    works in the abstract -- `test_the_status_glyphs_...` covers that -- but that
    it works on the sentences the lanes actually wrote, punctuation and bold
    markers included.
    """
    for text in (
        f"**Status:** {CHECK} **TAKEN by lane A.** `ere = {{ path = \"..\" }}`"
        " is in the workspace.",
        f"**Status:** {CHECK} **ACKNOWLEDGED 2026-08-17 by lane B \u2014 the"
        " argument is accepted.**",
        f"**Status:** {CHECK} **REBUILT 2026-08-21 by lane B**, twice \u2014"
        " `b405fc5c6` cleared the drift you reported.",
    ):
        check(f"tick carries: {text[12:46]!r}", verdict(mod, text), "done")


def test_the_widened_vocabulary_did_not_widen_to_the_words(mod):
    """`taken`/`acknowledged`/`accepted` must NOT be done words on their own.

    This is the test that keeps the fix honest. The cheap version of it was to
    add those three verbs to `DONE_WORDS`, and it would have passed every test
    above -- while clearing requests that are still work, because all three
    have a live reading. `**Status:** acknowledged, fix scheduled for next
    week` is not done, and neither is `TAKEN -- I own it now, starting
    Monday`. The tick has no such reading, which is the whole reason the glyph
    is the marker and the verb is not.

    So: a tick with those words is done (above), and the same words without a
    tick are open (here). If someone later "simplifies" this by moving the
    verbs into the word list, this fails.
    """
    for text in (
        "**Status:** TAKEN by lane A, starting once the sysroot lands.",
        "**Status:** ACKNOWLEDGED \u2014 I agree with the argument.",
        "**Status:** accepted, scheduled behind the ext4 port.",
    ):
        check(f"no tick, no clear: {text[12:46]!r}", verdict(mod, text), "open")


def test_an_open_glyph_outranks_a_tick_in_the_same_status(mod):
    """`✅ ask 1 landed … ⏳ ask 2 blocked` is open, exactly like the words.

    Rule 2 tells the lanes to write a half-done status this way, so the ranking
    that makes it safe has to hold for glyphs and not only for the vocabulary.
    Both orders are checked: the merge in `status_verdict` sorts done positions
    but returns on the first open hit anywhere in the block, so a tick that
    comes *first* must still lose.
    """
    check("tick then hourglass is open",
          verdict(mod, f"**Status:** {CHECK} ask 1 landed; \u23f3 ask 2 sits"
                       " with lane C."),
          "open")
    check("hourglass then tick is open",
          verdict(mod, f"**Status:** \u23f3 ask 2 sits with lane C; {CHECK}"
                       " ask 1 landed."),
          "open")
    check("a lone tick with no open glyph is still done",
          verdict(mod, f"**Status:** {CHECK} ask 1 landed."), "done")


def test_a_negated_tick_is_not_a_clear(mod):
    """`not ✅` reads as open, like every other done marker.

    The glyph goes through the same negator guard as the words, which is why
    `status_verdict` merges the two sets of match positions instead of running
    the tick regex as a separate short-circuit. A separate check would have
    skipped the guard and made the tick the one done marker that cannot be
    negated -- in the direction that loses work.
    """
    check("a negated tick does not clear",
          verdict(mod, f"**Status:** not {CHECK} yet \u2014 waiting on the"
                       " sysroot."),
          "open")


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
# Where in the file the classifier looks
# --------------------------------------------------------------------------
#
# The three tests below are one regression with three faces. A request is a
# header, then an essay, then (sometimes) a reply; status lives in the first and
# last of those and must not be read from the middle. The code used to
# approximate "the reply" as the last 25 lines, which is right only while
# replies are short. `b-a-raw-nic-claim-tests-race-...` answers the request at
# line 123 of 178 -- a 55-line reply -- so the answer sat outside the window and
# the request was reported open for six days after it was resolved. Twenty-five
# files in the dropbox had a resolution heading the old window could not see.

def _classify_text(mod, text):
    """`classify()` on a throwaway file, as `(is_open, reason)`."""
    import pathlib
    import tempfile
    with tempfile.TemporaryDirectory() as d:
        p = pathlib.Path(d) / "b-a-fixture.md"
        p.write_text(text, encoding="utf-8")
        is_open, reason, _title = mod.classify(p)
    return is_open, reason


def _padding(n, word="filler"):
    """`n` lines of body prose, to push a reply out of any fixed-size tail."""
    return "\n".join(f"{word} line {i}" for i in range(n))


def test_a_long_reply_section_is_read_however_long_it_is(mod):
    """The reply window must be bounded by the reply, not by a constant.

    The reply here is far longer than TAIL_LINES, so a fixed tail sees only its
    last lines and misses the heading that resolves the request -- which is
    exactly how a resolved request stayed on the queue.
    """
    reply_len = mod.TAIL_LINES * 3
    text = (
        "# B -> A -- something raced\n\n"
        "**Status:** found by a script; baselined, not fixed.\n\n"
        + _padding(60, "essay")
        + "\n\n## Lane A's answer -- RESOLVED (2026-08-23)\n\n"
        + _padding(reply_len, "reply")
        + "\n"
    )
    is_open, reason = _classify_text(mod, text)
    check("a reply longer than the tail window is still seen", is_open, False)
    check("and it is the reply heading that says so",
          "lane a's answer" in reason.lower(), True)


def test_the_essay_body_is_not_read_as_this_requests_status(mod):
    """Between header and reply is prose, and prose is not a status.

    Widening the window to the whole file is the obvious way to fix the test
    above, and it is wrong: eight files in the dropbox discuss `**Status:**`
    markers in running text, because several of the requests are *about* the
    status protocol. Reading the body classifies a request on a sentence
    describing a different one.
    """
    text = (
        "# A -> B -- a request\n\n"
        "**Status:** LANDED 2026-08-21 by lane B.\n\n"
        + _padding(30, "essay")
        + "\n\nThe twelve files still say `**Status:** unknown` and therefore\n"
          "show up as open in `scripts/open-requests.py --lane b`.\n\n"
        + _padding(30, "essay")
        + "\n"
    )
    is_open, _reason = _classify_text(mod, text)
    check("prose about a status marker does not reopen a landed request",
          is_open, False)


def test_a_heading_inside_a_code_fence_is_not_a_heading(mod):
    """`#` inside a fence is a shell comment, not markdown.

    The dropbox carries ~1700 fenced lines, much of it commented bash. No line
    in it reads `# resolved ...` today, which is precisely why this is a test
    and not a bug report -- the same mistake has already been made once in this
    repo, against `known-issues.md`, where `#` comments inside fences were
    parsed as entry headings and tore entries in half.

    Deliberately no `**Status:**` line: a recognised status word is decisive and
    would return before any heading is looked at, so a fixture carrying one
    cannot tell whether fences are handled at all. This one reaches the heading
    search, which is the code under test.
    """
    text = (
        "# B -> A -- still broken\n\n"
        "Here is what I ran:\n\n"
        "```bash\n"
        "# resolved the race by moving the tests\n"
        "## Landed -- lane A, 2026-08-29\n"
        "git commit -m 'fixed'\n"
        "```\n\n"
        "Still fails.\n"
    )
    is_open, reason = _classify_text(mod, text)
    check("a fenced '# resolved' comment does not close a request",
          is_open, True)
    check("and the file reads as unstamped rather than resolved",
          reason, "no status marker")


def test_fenced_headings_do_not_move_the_reply_window(mod):
    """A fence deep in the body must not be mistaken for the reply's start.

    If it were, the window would swallow the essay and this test would fail the
    same way `test_the_essay_body_is_not_read...` does -- but via the window
    rather than via a whole-file read, which is a separate way in.
    """
    # The geometry is the test, so it is asserted rather than assumed: the fence
    # must sit past the head, the prose after it, and the tail floor after that.
    # If the fence were taken as the reply's start the window would open at the
    # fence and swallow the prose; with fences blanked it opens at the floor and
    # does not.
    text = (
        "# A -> B -- a request\n\n"
        "**Status:** LANDED 2026-08-21 by lane B.\n\n"
        + _padding(mod.HEAD_LINES + 5, "essay")
        + "\n\n```console\n$ git log\n## Landed\n```\n\n"
        + "The remaining files say `**Status:** open` and are lane C's.\n\n"
        + _padding(mod.TAIL_LINES + 15, "essay")
        + "\n"
    )
    lines = text.splitlines()
    fence_at = next(i for i, l in enumerate(lines) if l.startswith("```"))
    prose_at = next(i for i, l in enumerate(lines) if "remaining files" in l)
    floor = len(lines) - mod.TAIL_LINES
    check("fixture geometry: the fence is past the head",
          fence_at >= mod.HEAD_LINES, True)
    check("fixture geometry: the prose is past the head and before the floor",
          mod.HEAD_LINES <= prose_at < floor, True)

    is_open, _reason = _classify_text(mod, text)
    check("a fenced heading does not drag the window over the essay",
          is_open, False)


def test_a_reply_heading_outranks_an_unreadable_status(mod):
    """An unclassifiable header must not short-circuit past the reply.

    This is the caller-side half of the bug. `status_verdict` correctly reported
    "I found a status and could not read it", and `classify` returned that
    without ever consulting the reply headings -- a *less*-informative signal
    beating a more-informative one. `**Status:** baselined, not fixed` is
    negator-guarded away from done and contains no open word, so it landed in
    exactly that branch.
    """
    text = (
        "# B -> A -- something raced\n\n"
        "**Status:** found by `scripts/raced-globals.py`; baselined, not fixed.\n\n"
        "Body.\n\n"
        "## Lane A's answer -- RESOLVED (2026-08-23)\n\nDone.\n"
    )
    is_open, reason = _classify_text(mod, text)
    check("an answered request is not held open by an unreadable header",
          is_open, False)
    check("and the reason names the heading, not the header",
          reason.lower().startswith("## lane a's answer"), True)


def test_an_open_status_still_outranks_a_reply_heading(mod):
    """Precedence: a *recognised* status word wins over any heading.

    The fix above must not become "a reply heading closes anything". A lane that
    writes `**Status:** OPEN` under its own reply heading is telling us the
    request is live, and that sentence outranks the section it sits in.
    """
    text = (
        "# B -> A -- something raced\n\n"
        "Body.\n\n"
        "## Lane A's answer\n\n"
        "**Status:** OPEN -- I could not reproduce it; over to you.\n"
    )
    is_open, _reason = _classify_text(mod, text)
    check("an explicit OPEN under a reply heading keeps the request open",
          is_open, True)


def test_status_verdict_says_whether_it_recognised_the_wording(mod):
    """The third element is what lets `classify` order the two signals.

    Both branches report open, so the flag cannot be inferred from the verdict;
    without it the caller cannot tell "this status means open" from "I could not
    read this status", and only the second should defer to a heading.
    """
    recognised = mod.status_verdict("**Status:** OPEN, still working")
    unreadable = mod.status_verdict("**Status:** who can say, really")
    done = mod.status_verdict("**Status:** LANDED 2026-08-21")
    check("a recognised OPEN is decisive", recognised[2], True)
    check("a recognised DONE is decisive", done[2], True)
    check("an unrecognised status is not decisive", unreadable[2], False)
    check("but an unrecognised status is still open", unreadable[0], True)


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
