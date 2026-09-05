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

#: The preceding line, when it starts with one of these, is not a plain
#: paragraph, and a following `---` is then either a thematic break or part of
#: another construct.  Flagging those would be a false positive, and a false
#: finding costs more than a missed one -- it teaches the reader to skim.
NOT_A_PARAGRAPH_PREFIXES = ("#", ">", "|", "-", "*", "+", "=", "`", "~", "_")

#: An ordered-list marker: `1.` / `12)`.  Same reasoning as above.
ORDERED_ITEM_RE = re.compile(r"^ *\d+[.)]\s")

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
        if stripped.startswith(NOT_A_PARAGRAPH_PREFIXES):
            continue
        if ORDERED_ITEM_RE.match(prev):
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


def _git(args: list[str], cwd: str) -> str:
    """Run git with a scrubbed environment.

    `git -C <dir>` does **not** name a repository: an inherited `GIT_DIR`
    outranks both `-C` and the working directory, and git exports `GIT_DIR`
    into every hook's environment.  This checker runs from a pre-push hook, so
    without the scrub it would read whichever repository the hook was invoked
    for -- which, in a four-worktree tree, is not necessarily this one.
    """
    proc = subprocess.run(
        ["git", "-C", cwd] + args,
        capture_output=True, env=gitenv.clean_env(), check=False,
    )
    if proc.returncode != 0:
        raise ValueError(
            "git " + " ".join(args) + " failed: "
            + proc.stderr.decode("utf-8", "replace").strip()
        )
    return proc.stdout.decode("utf-8", "replace")


def collect_from_head(root: str, rev: str) -> list[tuple[str, str]]:
    """`(path, text)` for every `*.md` in `rev`."""
    try:
        _git(["rev-parse", "--verify", f"{rev}^{{commit}}"], root)
    except ValueError as exc:
        raise ValueError(f"{rev!r} is not a commit: {exc}") from exc
    listing = _git(["ls-tree", "-r", "--name-only", "-z", rev], root)
    paths = [p for p in listing.split("\0") if p.endswith(".md")]
    out = []
    for p in paths:
        blob = _git(["show", f"{rev}:{p}"], root)
        out.append((p, blob))
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


def analyse_corpus(docs: list[tuple[str, str]]) -> Findings:
    f = Findings()
    for path, text in docs:
        analyse_one(path, text, f)
    if f.docs_read < MIN_DOCS:
        raise ValueError(
            f"only {f.docs_read} Markdown document(s) found, below the floor "
            f"of {MIN_DOCS}. Either the enumeration broke or the tree lost its "
            "documentation; both want a human, and reporting 'no failures' "
            "over a corpus this thin is the failure this checker exists to "
            "prevent"
        )
    return f


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

    if failures:
        print(f"\n{len(failures)} of {count} self-test(s) FAILED")
        return 1
    print(f"check-accidental-headings: self-test passed ({count} checks)")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Find `---` separators that Markdown renders as headings.")
    parser.add_argument("--self-test", action="store_true",
                        help="run the built-in fixtures and exit")
    parser.add_argument("--head", metavar="REV", default=None,
                        help="read the documents from REV instead of the "
                             "working tree (what a push gate must do)")
    parser.add_argument("--fix", action="store_true",
                        help="insert the missing blank lines in place")
    args = parser.parse_args(argv)

    if args.fix and args.head is not None:
        print("check-accidental-headings: --fix writes the working tree and "
              "--head reads a commit; pick one", file=sys.stderr)
        return 2

    if args.self_test:
        if args.head is not None:
            print("check-accidental-headings: --self-test and --head are "
                  "different jobs; pick one", file=sys.stderr)
            return 2
        return self_test()

    root = str(pathlib.Path(__file__).resolve().parent.parent)
    try:
        docs = (collect_from_head(root, args.head) if args.head
                else collect_from_worktree(root))
        f = analyse_corpus(docs)
    except ValueError as exc:
        print(f"check-accidental-headings: cannot reach a verdict: {exc}",
              file=sys.stderr)
        return 2

    for w in f.warnings:
        print(f"  warning: {w}")

    if args.fix and f.sites:
        try:
            n = fix_sites(root, f.sites)
        except (OSError, ValueError) as exc:
            print(f"check-accidental-headings: --fix failed: {exc}",
                  file=sys.stderr)
            return 2
        print(f"check-accidental-headings: inserted {n} blank line(s); "
              "re-run without --fix to confirm")
        return 0

    if f.failures:
        print(f"\n{len(f.failures)} accidental heading(s):", file=sys.stderr)
        for d in f.failures:
            print(f"  - {d}", file=sys.stderr)
        return 1

    tail = f", {len(f.warnings)} warning(s)" if f.warnings else ", no warnings"
    print(f"check-accidental-headings: OK ({f.docs_read} document(s), "
          f"{f.lines_read} line(s), {f.rules_seen} separator(s){tail})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
