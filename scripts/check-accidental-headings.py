#!/usr/bin/env python3
"""Refuse to build when a `---` separator has silently become an `<h2>`.

In Markdown a line of three or more hyphens means one of two completely
different things, and which one it means is decided by the line *above* it:

    ...and that is why it matters.        <- blank line above the `---`
                                             => the `---` is a horizontal rule
    ---

    ...and that is why it matters.        <- no blank line above the `---`
    ---                                      => the whole sentence becomes a
                                                giant `<h2>` heading and the
                                                rule disappears

The second form is a "setext heading" -- Markdown's original, pre-`#` way of
writing a heading, where you underline the text instead of prefixing it.  It is
still in the spec, so no tool reports it, and in a plain-text editor the two
forms differ by exactly one blank line.  Nobody proof-reads a 116 000-line
document in a renderer, so the mistake survives indefinitely.

**This is not hypothetical.**  `known-issues.md` held sixteen of them when this
checker was written, contributed by all three lanes over several weeks.  Each
one takes the last sentence of an entry -- typically the final clause of an
"**If it is never fixed:**" paragraph -- and renders it as a section heading
the size of the document's own titles, while deleting the separator that was
supposed to close the entry.  The rendered document therefore has sixteen
headings that no author wrote, saying things like "than having no setting at
all", and sixteen missing dividers.

Scope is **every tracked `*.md`**, not a list of documents.  That is a direct
inheritance from `design-decisions.md` §769: the `check-eol` gate took its
reading list from `.gitattributes` and was consequently blind to `*.rs` for its
entire existence.  A scope that is a list is a scope that goes stale, and the
cost of reading all 308 tracked Markdown files is a fraction of a second.

Under `--head <rev>` the documents are read from that commit rather than from
the working tree, because a push gate must judge what is being pushed.  A
worktree-scoped document gate can be silenced by an uncommitted edit, which is
the defect found in gates 8, 9 and 13 -- each of which was correctly wired and
still returned the wrong verdict.

Exit codes: 0 clean (warnings do not fail), 1 a document is wrong, 2 the
checker could not reach a verdict.
"""

from __future__ import annotations

import argparse
import contextlib
import io
import os
import pathlib
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gitenv  # noqa: E402  (must follow the path insert)

#: A thematic-break / setext-underline candidate.  Up to three leading spaces:
#: four or more makes it an indented code block, where it is literal text.
#: Trailing whitespace is allowed, as the spec allows it.
DASH_RULE_RE = re.compile(r"^ {0,3}-{3,}[ \t]*$")

#: The `=` form.  There is no `===` thematic break in Markdown, so a line of
#: equals signs after a paragraph is *always* a heading and can never be a
#: mistyped separator.  It is reported as a warning so the count is visible,
#: never as a failure -- see `analyse`.
EQUALS_RULE_RE = re.compile(r"^ {0,3}={1,}[ \t]*$")

#: A fence opens or closes a code block.  Info strings (```sh) are allowed on
#: the opener.  Tracked loosely on purpose: this is used only to *suppress*
#: findings, so an over-eager fence costs a missed report, never a false one.
FENCE_RE = re.compile(r"^ {0,3}(`{3,}|~{3,})")

#: The preceding line is not a plain paragraph -- it is a construct after which
#: a `---` is legitimately a thematic break.
#:
#: Every alternative here is anchored precisely, because the first draft used a
#: set of one-character *prefixes* and that was wrong in both directions.  A
#: bare "`" prefix (meant for code fences) also suppressed every paragraph line
#: that merely *starts* with an inline code span, and a bare "*" prefix (meant
#: for bullets) suppressed every line starting with `**bold**` -- which is how
#: nearly every entry in `known-issues.md` begins its paragraphs.  Both were
#: found by reading the documents rather than by re-reading the regex:
#: `design-decisions.md:65450` is a real accidental heading that the prefix
#: version silently passed, because the line above it opens with a `TD-...`
#: identifier in backticks.
#:
#: So: a list marker must be followed by whitespace (`- x`, not `**bold**`), a
#: fence must be three or more marks (not one inline backtick), and a setext
#: underline must be the whole line.
NOT_A_PARAGRAPH_RE = re.compile(
    # `[ ]` and not a bare space: `re.VERBOSE` discards literal whitespace, so
    # ` {0,3}` compiles to a repeat with nothing to repeat.
    r"""^[ ]{0,3}(
          \#{1,6}(\s|$)          # ATX heading
        | >                      # blockquote
        | \|                     # table row
        | [-*+](\s|$)            # bullet: the marker must be followed by space
        | \d+[.)](\s|$)          # ordered item
        | (`{3,}|~{3,})          # code fence, three or more marks
        | -{3,}[ \t]*$           # a thematic break / setext underline
        | ={1,}[ \t]*$           # a setext underline
        | _{3,}[ \t]*$           # the underscore thematic break
    )""",
    re.VERBOSE,
)

#: A plausible parse.  A checker that discovers nothing reports no failures,
#: which reads exactly like a pass, so refuse to return a verdict if the corpus
#: comes back implausibly small.  308 `*.md` files were tracked when this was
#: written; the floor is set far below that so it catches "the enumeration
#: broke", not "somebody deleted a document".
MIN_DOCS = 20


class Findings:
    """Failures stop the build; warnings are counted and shown."""

    def __init__(self) -> None:
        self.failures: list[str] = []
        self.warnings: list[str] = []
        #: `(path, 1-based line of the `---`)` for each failure, so `--fix`
        #: does not have to parse its own report back out of prose.
        self.sites: list[tuple[str, int]] = []
        self.docs_read = 0
        self.lines_read = 0
        self.rules_seen = 0

    def ok(self) -> bool:
        return not self.failures


def analyse_one(path: str, text: str, f: Findings) -> None:
    """Append this document's findings to `f`.

    Split out from the corpus walk so the self-test can drive a single
    document, and so `--head` and the worktree walk share one implementation
    rather than two that can drift.
    """
    lines = text.split("\n")
    f.docs_read += 1
    f.lines_read += len(lines)

    in_fence = False
    fence_marker = ""

    # YAML front matter: a `---` on the very first line opens it, and its
    # closing `---` is legitimately preceded by a non-blank line.  Without this
    # every document carrying front matter would report one false finding.
    start = 0
    if lines and lines[0].rstrip() == "---":
        for i in range(1, len(lines)):
            if lines[i].rstrip() in ("---", "..."):
                start = i + 1
                break

    for i in range(start, len(lines)):
        line = lines[i]

        if FENCE_RE.match(line):
            marker = line.strip()[0]
            if not in_fence:
                in_fence, fence_marker = True, marker
            elif marker == fence_marker:
                in_fence, fence_marker = False, ""
            continue
        if in_fence:
            continue

        is_dash = bool(DASH_RULE_RE.match(line))
        is_equals = bool(EQUALS_RULE_RE.match(line))
        if not (is_dash or is_equals):
            continue
        if is_dash:
            f.rules_seen += 1
        if i == 0:
            continue

        prev = lines[i - 1]
        if not prev.strip():
            continue                       # blank above: a real thematic break
        stripped = prev.lstrip()
        if NOT_A_PARAGRAPH_RE.match(prev):
            continue
        if len(prev) - len(stripped) >= 4:
            continue                       # indented code block, not prose

        excerpt = prev.strip()
        if len(excerpt) > 78:
            excerpt = excerpt[:75] + "..."
        if is_dash:
            f.sites.append((path, i + 1))
            f.failures.append(
                f"{path}:{i + 1}: `{line.strip()}` directly under a paragraph, "
                f"so that paragraph's last line renders as an <h2> and the "
                f"separator disappears. Insert a blank line above it.\n"
                f"      renders as a heading: {excerpt}"
            )
        else:
            # Never a mistyped separator, so never a failure -- but a setext
            # H1 in a tree that writes every other heading as `#` is worth a
            # look, and counting it costs nothing.
            f.warnings.append(
                f"{path}:{i + 1}: `{line.strip()}` makes the line above a "
                f"setext <h1>; this tree writes headings as `#`.\n"
                f"      renders as a heading: {excerpt}"
            )


def _git_raw(args: list[str], cwd: str, stdin: bytes | None = None) -> bytes:
    """Run git with a scrubbed environment and return raw stdout.

    `git -C <dir>` does **not** name a repository: an inherited `GIT_DIR`
    outranks both `-C` and the working directory, and git exports `GIT_DIR`
    into every hook's environment.  This checker runs from a pre-push hook, so
    without the scrub it would read whichever repository the hook was invoked
    for -- which, in a four-worktree tree, is not necessarily this one.
    """
    proc = subprocess.run(
        ["git", "-C", cwd] + args, input=stdin,
        capture_output=True, env=gitenv.clean_env(), check=False,
    )
    if proc.returncode != 0:
        raise ValueError(
            "git " + " ".join(args) + " failed: "
            + proc.stderr.decode("utf-8", "replace").strip()
        )
    return proc.stdout


def _git(args: list[str], cwd: str) -> str:
    return _git_raw(args, cwd).decode("utf-8", "replace")


# `<mode> <type> <oid>\t<path>`, which is what `ls-tree -r -z` emits. `-z` also
# turns off git's path quoting, so the path after the tab is the literal bytes.
LS_TREE_RE = re.compile(r"^\d+ (\w+) ([0-9a-f]+)\t(.*)$", re.DOTALL)


def changed_md_paths(root: str, rev: str) -> list[str]:
    """The `*.md` paths `rev` adds or modifies, relative to its first parent.

    `--diff-filter=d` drops deletions: a document the commit removed is not in
    `rev`'s tree, so asking for its blob would fail, and a deleted document
    cannot carry a heading anyone will read.

    `--root` is what makes a root commit list its whole tree instead of nothing.
    That case is not hypothetical for this checker -- `test-checkers-honour-head.py`
    builds scratch repositories whose first commit is a root commit, and without
    `--root` every one of those fixtures would report an empty corpus and pass.
    """
    out = _git_raw(["diff-tree", "-r", "--root", "--no-commit-id",
                    "--name-only", "-z", "--diff-filter=d", rev], root)
    return [p for p in out.decode("utf-8", "replace").split("\0")
            if p.endswith(".md")]


def collect_from_head(root: str, rev: str,
                      paths: list[str] | None = None) -> list[tuple[str, str]]:
    """`(path, text)` for every `*.md` in `rev`, or just `paths` if given.

    WHY `cat-file --batch` AND NOT A `git show` PER FILE. The obvious loop is
    one subprocess per document, and it was what this function shipped with. On
    this tree that is ~310 process spawns, measured at **43 seconds** for a
    single revision (2026-09-05, the filesystem of open-questions A-Q7). One
    `--batch` process streams every blob down one pipe and brought the same
    revision in 22 s.

    WHY THAT WAS STILL NOT ENOUGH, AND WHAT `paths` IS FOR. Measuring where the
    remaining 22 s went says something worth writing down: the cost here is
    **per object, not per byte**. The two largest documents in the tree are
    13 MB together and `cat-file` returns them in 3 s; the other 306, totalling
    6 MB, take 47 s. That is ~150 ms of pure lookup latency per object, which no
    amount of streaming removes -- it is the same filesystem pathology as A-Q7.

    So the push gate does not read the corpus. It reads the documents the commit
    changed, which is also the more correct question for a gate to ask: gate 14
    exists to refuse to *publish a new* accidental heading, and a lane must not
    be blocked from pushing by a pre-existing defect in another lane's document
    that it has no business editing. The whole-corpus sweep is still what
    `scripts/boot-test.sh` and a bare invocation run.
    """
    try:
        _git(["rev-parse", "--verify", f"{rev}^{{commit}}"], root)
    except ValueError as exc:
        raise ValueError(f"{rev!r} is not a commit: {exc}") from exc

    if paths is None:
        # Whole corpus: the tree has to be listed before it can be read.
        listing = _git_raw(["ls-tree", "-r", "-z", rev], root)
        wanted = []
        for entry in listing.split(b"\0"):
            if not entry:
                continue
            m = LS_TREE_RE.match(entry.decode("utf-8", "replace"))
            # A tree entry this regex cannot parse is a shape change in git's
            # output, not a document to skip quietly: reporting a clean corpus
            # over a listing we failed to read is the failure the floor exists
            # to catch, and it would be reached before the floor if this
            # silently dropped entries.
            if m is None:
                raise ValueError(f"cannot parse ls-tree entry {entry!r}")
            kind, oid, path = m.group(1), m.group(2), m.group(3)
            if kind == "blob" and path.endswith(".md"):
                wanted.append((oid, path))
    else:
        # Scoped: `cat-file --batch` takes any revision syntax, so `<rev>:<path>`
        # fetches the blob without a listing step. One fewer git spawn, which on
        # this filesystem is worth about a second of a gate's budget.
        wanted = [(f"{rev}:{p}", p) for p in paths]

    return _read_blobs(root, wanted)


def _read_blobs(root: str, wanted: list[tuple[str, str]]) -> list[tuple[str, str]]:
    """Fetch `[(request, path)]` through one `cat-file --batch`.

    `--batch` answers each request with `<oid> <type> <size>\\n`, then exactly
    `size` bytes, then one `\\n`. Walked by byte offset rather than split on
    newlines because a document's own content contains both newlines and lines
    that look like headers -- splitting would resynchronise on the first line of
    prose that happened to have three space-separated words.
    """
    if not wanted:
        return []
    stdin = ("\n".join(req for req, _ in wanted) + "\n").encode("utf-8")
    blob = _git_raw(["cat-file", "--batch"], root, stdin=stdin)

    out: list[tuple[str, str]] = []
    pos = 0
    for req, path in wanted:
        nl = blob.find(b"\n", pos)
        if nl < 0:
            raise ValueError(f"cat-file output ended before {path}")
        header = blob[pos:nl].decode("utf-8", "replace").split()
        # `<name> missing` is what an unresolvable request returns, and it is two
        # fields, not three. Raising rather than skipping matters: a silently
        # skipped document is a document reported clean.
        if len(header) != 3 or header[1] != "blob":
            raise ValueError(
                f"cat-file answered {' '.join(header)!r} for {path}")
        size = int(header[2])
        start = nl + 1
        out.append((path, blob[start:start + size].decode("utf-8", "replace")))
        pos = start + size + 1                  # the trailing newline
    return out


def collect_from_worktree(root: str) -> list[tuple[str, str]]:
    """`(path, text)` for every tracked `*.md` in the working tree.

    Tracked, not globbed: an untracked scratch document is nobody's problem,
    and a `target/` full of vendored Markdown would swamp the report.
    """
    listing = _git(["ls-files", "-z", "--", "*.md"], root)
    paths = [p for p in listing.split("\0") if p]
    out = []
    for p in paths:
        try:
            text = (pathlib.Path(root) / p).read_text(encoding="utf-8",
                                                      errors="replace")
        except OSError:
            # Tracked but unreadable: skip rather than fail the build, and let
            # the floor below notice if this ever happens at scale.
            continue
        out.append((p, text))
    return out


def fix_sites(root: str, sites: list[tuple[str, int]]) -> int:
    """Insert the missing blank line above each reported `---`.

    Bytes in, bytes out, and never Python's default text mode: on Windows that
    mode turns every `\\n` into `\\r\\n`, which is precisely how this tree
    acquired 168 CRLF files from its own scripts (`known-issues.md` →
    `A-27-...`).  Rewriting a document to fix its formatting is a bad moment to
    corrupt every line ending in it.

    Applied per file from the bottom up, so an insertion never invalidates the
    line numbers of the sites still to be applied.
    """
    by_file: dict[str, list[int]] = {}
    for path, line_no in sites:
        by_file.setdefault(path, []).append(line_no)

    fixed = 0
    for path, line_nos in sorted(by_file.items()):
        full = pathlib.Path(root) / path
        raw = full.read_bytes()
        lines = raw.split(b"\n")
        for line_no in sorted(set(line_nos), reverse=True):
            idx = line_no - 1
            if idx < 0 or idx >= len(lines):
                raise ValueError(f"{path}: line {line_no} is out of range")
            # Match the line ending already in use rather than imposing one:
            # a lone LF inserted into a CRLF document is a new mixed-ending
            # file, which is the neighbouring defect.
            blank = b"\r" if lines[idx].endswith(b"\r") else b""
            lines.insert(idx, blank)
            fixed += 1
        full.write_bytes(b"\n".join(lines))
        print(f"  fixed {len(set(line_nos))} in {path}")
    return fixed


def analyse_corpus(docs: list[tuple[str, str]],
                   floor: int = MIN_DOCS) -> Findings:
    """Analyse every document, refusing a verdict over an implausibly thin one.

    `floor` is a parameter and not a constant because the two callers ask
    different questions. A whole-tree sweep that comes back with four documents
    has a broken enumeration, and saying "no failures" about it would be the
    exact false green this checker exists to prevent -- so it gets the real
    floor. A push gate scoped to *the documents this commit changed* legitimately
    sees one, or zero; there the enumeration is `git diff-tree`, whose failure
    mode is a non-zero exit rather than a short list, so a floor there would
    reject correct pushes and teach people to bypass the gate.
    """
    f = Findings()
    for path, text in docs:
        analyse_one(path, text, f)
    if f.docs_read < floor:
        raise ValueError(
            f"only {f.docs_read} Markdown document(s) found, below the floor "
            f"of {floor}. Either the enumeration broke or the tree lost its "
            "documentation; both want a human, and reporting 'no failures' "
            "over a corpus this thin is the failure this checker exists to "
            "prevent"
        )
    return f


def report(f: Findings, quiet: bool) -> int:
    """Print the verdict and return the exit status.

    Split out of `main` for one reason: so the self-test can prove that
    `--quiet` silences *only* the pass. A `--quiet` that also swallowed a
    finding would make gate 14 — the only caller that passes it — report a
    clean push over a broken document, which is the exact shape of failure
    every gate in `scripts/hooks/pre-push` exists to make impossible. Left
    inline in `main` the claim would be untestable without a 30-second corpus
    scan (measured 2026-09-05; see open-questions A-Q7 on this filesystem), so
    it would not have been tested.
    """
    for w in f.warnings:
        print(f"  warning: {w}")

    if f.failures:
        print(f"\n{len(f.failures)} accidental heading(s):", file=sys.stderr)
        for d in f.failures:
            print(f"  - {d}", file=sys.stderr)
        return 1

    if not quiet:
        tail = (f", {len(f.warnings)} warning(s)" if f.warnings
                else ", no warnings")
        print(f"check-accidental-headings: OK ({f.docs_read} document(s), "
              f"{f.lines_read} line(s), {f.rules_seen} separator(s){tail})")
    return 0


# ---------------------------------------------------------------------------
# Self-test.  Every rule gets a fixture that makes it fire, and every
# suppression gets one that proves it suppresses, because a rule that has never
# been seen to fire is a rule you are guessing about.
# ---------------------------------------------------------------------------

def self_test() -> int:
    failures: list[str] = []
    count = 0

    def one(text: str) -> Findings:
        f = Findings()
        analyse_one("doc.md", text, f)
        return f

    def check(label: str, condition: bool) -> None:
        nonlocal count
        count += 1
        if not condition:
            failures.append(label)
            print(f"FAIL  {label}")
        else:
            print(f"  ok    {label}")

    # The defect itself.
    f = one("Some prose that ends a section.\n---\n\n## Next\n")
    check("a `---` directly under prose is a failure", len(f.failures) == 1)
    check("...and the message quotes the line that becomes a heading",
          any("Some prose that ends a section." in d for d in f.failures))
    check("...and says how to fix it",
          any("Insert a blank line above it" in d for d in f.failures))

    # The correct form must not fire -- this is the common case, and a false
    # positive here would red every lane's build on a correctly written file.
    f = one("Some prose that ends a section.\n\n---\n\n## Next\n")
    check("a blank line above the `---` is clean", f.ok())
    check("...and the separator is still counted", f.rules_seen == 1)

    # The false positive this checker's own first draft produced: a `---`
    # immediately after a closing code fence is a thematic break, because the
    # fence is not a paragraph.  Found by inspecting a real hit, not by theory.
    f = one("Text:\n\n```sh\ngrep -v x file\n```\n---\n\n## Next\n")
    check("a `---` after a closing code fence is not a finding", f.ok())

    # Inside a fence, `---` is literal text.
    f = one("Text:\n\n```\nfoo\n---\n```\n\ndone\n")
    check("a `---` inside a fence is not a finding", f.ok())
    f = one("Text:\n\n~~~\nfoo\n---\n~~~\n\ndone\n")
    check("...with tilde fences either", f.ok())

    # A fence opened with backticks is not closed by tildes; getting this wrong
    # would silently swallow the rest of the document.
    f = one("```\nfoo\n~~~\nbar\n```\n\nprose\n---\n")
    check("a tilde does not close a backtick fence", len(f.failures) == 1)

    # Constructs that legitimately precede a thematic break.
    for label, prev in (
        ("a table row", "| a | b |"),
        ("a bullet", "- an item"),
        ("a star bullet", "* an item"),
        ("a blockquote", "> quoted"),
        ("a heading", "## A heading"),
        ("an ordered item", "1. an item"),
    ):
        f = one(f"intro\n\n{prev}\n---\n\ndone\n")
        check(f"{label} above a `---` is not a finding", f.ok())

    # The two false NEGATIVES the first draft shipped with, both from treating
    # a single character as a prefix.  These are regressions, not hardening:
    # `design-decisions.md:65450` was passed by the shipped version.
    f = one("intro\n\n"
            "with a much wider blast radius -- see\n"
            "`TD-A-AN-ABSENT-OPERAND-IS-THE-SAME-STRING`.\n---\n\ndone\n")
    check("a paragraph line opening with an inline code span is still prose",
          len(f.failures) == 1)
    f = one("intro\n\n**If it is never fixed:** the thing stays broken.\n---\n")
    check("a paragraph line opening with `**bold**` is still prose",
          len(f.failures) == 1)
    # ...while the constructs those rules were actually aimed at still suppress.
    f = one("intro\n\n- a bullet\n---\n\ndone\n")
    check("...and a real bullet still suppresses", f.ok())
    f = one("intro\n\n*a starred bullet\n---\n\ndone\n")
    check("...but a `*` with no space after it is prose, not a bullet",
          len(f.failures) == 1)

    # Four spaces of indent is a code block, where `---` is literal.
    f = one("intro\n\n    literal text\n    ---\n\ndone\n")
    check("an indented code block is not a finding", f.ok())

    # ...but up to three spaces is still a paragraph.
    f = one("intro\n\n  indented prose\n  ---\n\ndone\n")
    check("three spaces or fewer is still prose", len(f.failures) == 1)

    # YAML front matter's closing `---` is preceded by a non-blank line and is
    # not a heading.  Without the special case every such file reports one.
    f = one("---\ntitle: x\nauthor: y\n---\n\n# Doc\n\nprose\n")
    check("front matter's closing `---` is not a finding", f.ok())
    # ...and a real defect *after* front matter is still found.
    f = one("---\ntitle: x\n---\n\n# Doc\n\nprose\n---\n")
    check("...and a real one after front matter is still found",
          len(f.failures) == 1)

    # `===` is never a mistyped separator, so it is a warning and never fails.
    f = one("Heading text\n===\n\nbody\n")
    check("`===` under prose is a warning", len(f.warnings) == 1)
    check("...and does not fail the build", f.ok())

    # A `---` on the very first line of a document with no front matter close.
    f = one("---\n")
    check("a lone leading `---` does not crash", f.ok())

    # The floor: a corpus that came back thin must refuse a verdict rather than
    # report a clean pass.
    count += 1
    try:
        analyse_corpus([("a.md", "x\n")])
    except ValueError as exc:
        if "below the floor" not in str(exc):
            failures.append("thin corpus message")
            print("FAIL  thin corpus: wrong message")
        else:
            print("  ok    a thin corpus refuses to return a verdict")
    else:
        failures.append("thin corpus did not raise")
        print("FAIL  a thin corpus did not refuse")

    # The flag names, because they are an interface with `scripts/hooks/pre-push`
    # and that file cannot be type-checked. Gate 14 invokes `--selftest`,
    # `--head` and `--quiet`; a rename here that left `main` working would break
    # the gate into an exit-2 "checker fell over" on every push.
    p = build_parser()
    check("--selftest is accepted (the spelling the hook writes)",
          p.parse_args(["--selftest"]).self_test is True)
    check("--self-test is accepted too", p.parse_args(["--self-test"]).self_test
          is True)
    check("--head takes a revision",
          p.parse_args(["--head", "abc123"]).head == "abc123")
    check("--quiet is accepted", p.parse_args(["--quiet"]).quiet is True)
    check("...and nothing is quiet by default", p.parse_args([]).quiet is False)

    # `--quiet` must silence only the pass, driven through the real reporting
    # path rather than asserted about it. The gate is the only caller that
    # passes the flag, so if it could swallow a finding nothing else would ever
    # notice.
    def captured(f: Findings, quiet: bool) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            rc = report(f, quiet)
        return rc, out.getvalue(), err.getvalue()

    clean = one("intro\n\n---\n\ndone\n")
    rc, out, err = captured(clean, quiet=True)
    check("--quiet prints nothing on a clean pass", (rc, out, err) == (0, "", ""))
    rc, out, err = captured(clean, quiet=False)
    check("...and without it the OK line is printed", rc == 0 and "OK (" in out)

    dirty = one("prose that becomes a heading\n---\n")
    rc, out, err = captured(dirty, quiet=True)
    check("--quiet still fails on a real finding", rc == 1)
    check("...and still names it, on stderr",
          "prose that becomes a heading" in err)

    warned = one("Heading text\n===\n\nbody\n")
    rc, out, err = captured(warned, quiet=True)
    check("--quiet still prints warnings", rc == 0 and "warning:" in out)

    check("--changed-only is accepted",
          p.parse_args(["--changed-only", "--head", "x"]).changed_only is True)
    # Refused, not ignored: a gate that passed it without `--head` would look
    # scoped and be judging the whole corpus.
    err = io.StringIO()
    with contextlib.redirect_stderr(err):
        rc = main(["--changed-only"])
    check("--changed-only without --head is refused, not ignored",
          rc == 2 and "needs --head" in err.getvalue())

    # The two floors, which are the reason `analyse_corpus` takes the parameter
    # at all. A scoped run legitimately sees one document; a sweep that sees one
    # has a broken enumeration and must refuse.
    count += 1
    try:
        analyse_corpus([("a.md", "x\n")], floor=0)
    except ValueError:
        failures.append("floor=0 refused a legitimately scoped corpus")
        print("FAIL  floor=0 refused a one-document corpus")
    else:
        print("  ok    a scoped corpus of one document is allowed")

    if failures:
        print(f"\n{len(failures)} of {count} self-test(s) FAILED")
        return 1
    print(f"check-accidental-headings: self-test passed ({count} checks)")
    return 0


def build_parser() -> argparse.ArgumentParser:
    """Split out from `main` so the self-test can assert the flag *names*.

    The names are an interface with `scripts/hooks/pre-push`, not an internal
    detail: the hook writes them as literal strings in a shell script, so a
    rename here is a silent break there that nothing else in this file would
    notice.
    """
    parser = argparse.ArgumentParser(
        description="Find `---` separators that Markdown renders as headings.")
    # Both spellings. `--selftest` is what the twelve gate-wired checkers use
    # and therefore what `scripts/hooks/pre-push` writes; `--self-test` is what
    # this file shipped with and what the newer checkers use. A gate that
    # invokes the flag the checker does not have gets argparse's exit status 2,
    # which `run_checker` correctly reports as "the checker fell over" -- but
    # only after the push it was meant to judge has already been refused for the
    # wrong reason. Accepting both costs one word and removes that failure mode.
    parser.add_argument("--self-test", "--selftest", dest="self_test",
                        action="store_true",
                        help="run the built-in fixtures and exit")
    parser.add_argument("--head", metavar="REV", default=None,
                        help="read the documents from REV instead of the "
                             "working tree (what a push gate must do)")
    parser.add_argument("--fix", action="store_true",
                        help="insert the missing blank lines in place")
    # What makes gate 14 affordable, and what makes it ask a gate's question
    # rather than an audit's. See `collect_from_head`'s docstring for both
    # halves of the argument and the measurements behind them.
    parser.add_argument("--changed-only", action="store_true",
                        help="with --head, judge only the documents that "
                             "revision changed (what a push gate wants)")
    # For the push gate, which runs this once per pushed commit: the OK line is
    # worth printing once and not eight times. Failures and warnings are never
    # suppressed -- a `--quiet` that could hide a finding would be a bug, not an
    # option.
    parser.add_argument("--quiet", action="store_true",
                        help="print nothing on success")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    if args.fix and args.head is not None:
        print("check-accidental-headings: --fix writes the working tree and "
              "--head reads a commit; pick one", file=sys.stderr)
        return 2

    # Refused rather than silently ignored. `--changed-only` without `--head`
    # has no revision to diff against, and the tempting reading -- "changed in
    # the working tree" -- is the working-tree-versus-commit confusion that
    # `--head` exists to end. A flag that quietly means nothing is worse than
    # one that is rejected, because the gate that passes it would look wired.
    if args.changed_only and args.head is None:
        print("check-accidental-headings: --changed-only needs --head; there "
              "is no revision to take the change set from", file=sys.stderr)
        return 2

    if args.self_test:
        if args.head is not None:
            print("check-accidental-headings: --self-test and --head are "
                  "different jobs; pick one", file=sys.stderr)
            return 2
        return self_test()

    root = str(pathlib.Path(__file__).resolve().parent.parent)
    try:
        if args.head:
            paths = (changed_md_paths(root, args.head) if args.changed_only
                     else None)
            docs = collect_from_head(root, args.head, paths)
        else:
            paths = None
            docs = collect_from_worktree(root)
        # The floor is a whole-corpus claim; see `analyse_corpus`.
        f = analyse_corpus(docs, floor=0 if paths is not None else MIN_DOCS)
    except ValueError as exc:
        print(f"check-accidental-headings: cannot reach a verdict: {exc}",
              file=sys.stderr)
        return 2

    if args.fix and f.sites:
        for w in f.warnings:
            print(f"  warning: {w}")
        try:
            n = fix_sites(root, f.sites)
        except (OSError, ValueError) as exc:
            print(f"check-accidental-headings: --fix failed: {exc}",
                  file=sys.stderr)
            return 2
        print(f"check-accidental-headings: inserted {n} blank line(s); "
              "re-run without --fix to confirm")
        return 0

    return report(f, args.quiet)


if __name__ == "__main__":
    sys.exit(main())
