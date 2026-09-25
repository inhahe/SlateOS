#!/usr/bin/env python3
"""Find lossy byte->text conversions that reach a VALUE, not a message.

`String::from_utf8_lossy` and `OsStr::to_string_lossy` replace every byte they
cannot decode with U+FFFD. CLAUDE.md names the first of them outright -- "No
`from_utf8_lossy` -- that's silent data corruption" -- and on this OS the rule
bites hard, because a file name may hold every byte except `/` and NUL
(design.txt).

WHAT IT COST, in the one case that has been fixed so far:

    diff.rs   `diff_dirs` passed `p1.to_string_lossy()` to `diff_files`, so a
              directory walk decoded the names it had just listed and then
              could not open them. `diff -r` answered `No such file or
              directory` for a file it had itself enumerated. GNU diffs it.

WHY A GREP IS THE WRONG INSTRUMENT, measured rather than asserted. A plain
count of the two names across the 72 binaries on the image gives **131**. The
number that means anything is far smaller, and the difference is four
categories that are not defects at all:

  * 85 of them are inside `#[cfg(test)]`, where a lossy decode formats an
    assertion message;
  * a `#[cfg(not(unix))]` fallback is the WINDOWS HOST path -- the target
    compiles the `as_bytes()` branch beside it, so it cannot corrupt anything
    that runs on SlateOS. `quoting::os_bytes` documents this shape and several
    bins carry a private copy of it;
  * a decode that feeds `format!`/`panic!`/`Err(...)` is producing a MESSAGE.
    Rendering an undecodable name in a diagnostic is a display question, not a
    corruption one -- `quoting::quotef` is the better answer, but it is a
    different defect and a much smaller one;
  * a comment that merely names the function is not a call.

Ranking what is left by intuition does not work either, and that is worth
recording because it was tried: `sed` (25 raw) and `realpath` (9 raw) were
named as the two to open first, on the reasoning that both are "fundamentally
about transforming text and paths". `realpath` has ZERO outside its tests and
`sed` has one, in an error message. The bin that actually had the defect was
`diff`.

WHAT THE DEFAULT RUN DOES NOT COVER, learned by planting a probe in `cat.rs`
and watching the checker not notice. The default scope is
`scripts/rootfs-bin-manifest.txt`, which is what the IMAGE ships -- and that
list deliberately omits the thirteen names fastpy's compiled utilities own
(`cat`, `ls`, `grep`, `sort`, `wc` and the rest, per design-decisions.md 108).
Their Rust implementations exist in this tree and are NOT scanned by default,
because they are not on the image. `--all` covers them, along with every other
crate under `userspace/`, and reports a much larger and wholly unaudited
number.

    python scripts/lossy-decode.py                # the image's binaries
    python scripts/lossy-decode.py --all          # every crate under userspace
    python scripts/lossy-decode.py --selftest

WHAT THE DEFAULT SCOPE ALSO DID NOT COVER, and for four months nobody noticed:
`gui/` and `apps/`, which is to say lane C's entire tree. The scope was a bare
`os.walk(ROOT/"userspace")` written for the coreutils audit this tool was built
for. That default was right for that job and became the WHOLE scope once the
tool was wired into the shared pre-push gate -- so it ran on every push,
reported a number, passed, and the number was about a directory the pushed
change had not touched. On 2026-09-16 five lossy decodes reaching a value were
found in `apps/` and `gui/` BY HAND, in code this checker had run past for
months. A green check about the wrong directory is indistinguishable from a
green check.

    python scripts/lossy-decode.py --under gui --under apps
    python scripts/lossy-decode.py --under gui --under apps --write-baseline
    python scripts/lossy-decode.py --under gui --under apps --check

`--under` scans any subtree. `--check` compares against
`scripts/lossy-decode-baseline.txt` and fails on a file that is new to it, on
an EXTRA site in a file already listed (the count is part of the key, unlike
the sibling `argv-utf8-baseline.txt` which keys on path alone), and on a line
that has come true again -- so the backlog cannot silently accumulate dead
entries, which is what the sibling ratchet was found doing, 17 lines deep.

WHY `userspace/` IS A HARD FAILURE AND `gui/`+`apps/` IS A RATCHET. Not
squeamishness: userspace was audited down to six sites, every one IGNORE-exempt
with a written reason, so zero is its expected state and any finding there is
genuinely new. The lane C trees have 74 sites and have never been audited. A
hard failure would block every push on a pre-existing backlog, and a gate that
has to be bypassed to get work done stops being read at all.

WHAT THIS RATCHET CANNOT SEE, measured rather than argued, because lane B
asked the right question about a gate of their own on 2026-09-15: the test of a
ratchet is not "is the number going down" but "IS THERE AN EDIT THAT MOVES THE
NUMBER WITHOUT MOVING THE THING". There is one here, and it is worth stating
plainly rather than discovering later:

    // three sites                     // one site, same three conversions
    fn a(p: &Path) -> String {         fn lossy(p: &Path) -> String {
        p.to_string_lossy().into()         p.to_string_lossy().into()
    }                                  }
    fn b(p: &Path) -> String { ... }   fn a(p: &Path) -> String { lossy(p) }
    fn c(p: &Path) -> String { ... }   fn b(p: &Path) -> String { lossy(p) }
                                       fn c(p: &Path) -> String { lossy(p) }

Run against both, this checker reports 3 and then 1. Every call site still
performs exactly the same lossy conversion; only the count moved. The detector
is line-based and has no call graph, so routing conversions through one helper
is indistinguishable, to it, from removing two of them.

This is NOT an argument against centralising. Doing precisely that is often the
right fix -- `gui/toolkit/src/osbytes.rs` was hoisted on 2026-09-16 to give a
crate one `unsafe` proof instead of two, and its count went down for a good
reason. The point is that the NUMBER cannot tell the two apart, so a fall in it
is evidence and not proof, and a large fall deserves a look at the diff rather
than a note that the backlog is shrinking.

The second such edit is adding an IGNORE entry, which is by design -- but it is
the same shape, so the entry has to carry a reason a reader can check. An
exemption whose reason is "display" and nothing more is the no-op wearing the
uniform of an audit.

AND THE BACKLOG IS NOT A LIST TO ADD TO. A site that is genuinely safe belongs
in the IGNORE table below, which records WHY. The baseline records only THAT,
and a long list of `that` is precisely what stops anyone reading it. Of the
seven worst-looking of the 74, opened one at a time, ONE was a defect: four
were displays that are correct as written, one was behaviour-identical, and one
looked like a certain match for an already-fixed bug until sabotaging the fix
showed the test passed against the old code too. The tool classifies by what
the result is USED for; it cannot tell whether the bytes came from the
filesystem or from a format that defines its own encoding, and that question is
the whole of the triage.
"""

import argparse
import io
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rustlex  # noqa: E402

CALLS = ("from_utf8_lossy", "to_string_lossy", "to_str_lossy")

# A decode whose result is going into one of these is producing text for a
# human to read, not a value for the program to act on.
DIAGNOSTIC = re.compile(
    r"format!|panic!|diag!|eprintln!|println!|write!|writeln!|"
    r"unreachable!|assert|expect\(|Err\(|map_err|fatal|Fail::|"
    r"\.detail|usage|help|warn"
)

NL = chr(10)

# Sites that ARE a lossy decode reaching a value, and are still correct. Each
# is anchored on a substring of the line rather than a line number, so that
# editing the line re-opens the question -- which is the point: the exemption
# is for the code as audited, not for the file forever.
#
# This table records WHY. There is deliberately no baseline file recording
# merely THAT, which is the split `scripts/raced-globals-baseline.txt` states
# in its own header: "A genuine false positive belongs in the IGNORE table in
# the script, which records *why*, not here, which records only *that*."
#
# Audited 2026-09-14, when the image's raw count of 131 came down to these six.
IGNORE = (
    # --- lane C, audited 2026-09-16, one file at a time ---
    #
    # Two shapes account for all of these, and both are cases where a lossy
    # rendering is the CORRECT thing rather than a tolerated one.
    #
    # The first is the drawn half of the keep-exact-bytes pattern. A text
    # field, a breadcrumb pill and a list row are all *text*; bytes that are
    # not text cannot be drawn, so the rendering has to be lossy. What makes it
    # safe is that the bytes are kept beside it -- `PathBar::edit_exact`,
    # `FileDialog::filename_exact`, `RunDialog::command_exact` -- and it is the
    # kept bytes, never the rendering, that reach the filesystem. Deleting the
    # lossy call in these places does not fix anything; it makes the widget
    # undrawable.
    #
    # The second is a comparison against what is currently DISPLAYED, used to
    # decide whether the user edited the field. The question being asked is
    # literally "does this text still read as the rendering of those bytes",
    # so rendering is what it must compare, and comparing the bytes instead
    # would answer a different question and always say "edited".
    #
    # A site is exempted here only when its line is distinctive enough to
    # anchor on. `ignored()` is a substring match scoped to one file, so a
    # generic anchor like a bare `.to_string_lossy()` would silently cover
    # every future lossy call in that file as well. Those stay in
    # `lossy-decode-baseline.txt`, where the per-file COUNT still notices a new
    # one -- a weaker claim, and the honest one.
    #
    # 2026-09-25: the toolkit's and the shell's sites of both shapes -- the path
    # bar, the file dialog, the folder tree, the Run box -- now draw a name
    # through `pathcodec::display_os`, which spells each byte that is not text
    # as an octal escape instead of U+FFFD (`design-decisions.md` §873), so
    # their entries are gone. The applications' sites below remain until they
    # move to it too.
    # --- lane C, audited 2026-09-17 ---
    #
    # A third shape, and the first of it: the contents of a file being shown
    # to somebody. The two above are about a *path* drawn as text with the
    # bytes kept beside it. This one is not a path at all -- it is the first
    # 4 KiB of whatever file the preview pane is pointed at, and a file's
    # contents have no declared encoding to honour.
    #
    # It replaced `BufRead::lines().filter_map(|l| l.ok())`, which silently
    # *dropped* every line that was not UTF-8. A dropped line is the worse
    # failure: the reader sees plausible text with no sign that anything is
    # missing, where a replacement character says exactly where the bytes
    # stopped being text. Nothing downstream consumes the rendering -- it is
    # drawn and dropped, and the path beside it is untouched.
    # Moved 2026-09-17 from `apps/explorer/src/thumbs.rs`, which became the
    # `thumbs` crate when the photo library needed the same machinery. The
    # code is unchanged; only its address is, and the exemption is anchored on
    # the line's text so it still describes exactly what was audited.
    # A file's name drawn as a label, three times in one viewer: the window
    # title, a recent-files menu row, and a tab. In all three the `path` it was
    # taken from is held in the same struct and is what anything opens -- these
    # produce the text a person reads, and nothing is ever derived back from
    # them. The three are listed separately rather than under one generic
    # anchor because the anchor is the line's text: if one of them changes, its
    # exemption should expire and be looked at again on its own.
    ("apps/pdfviewer/src/main.rs",
     ".map(|n| n.to_string_lossy().into_owned())",
     "a document's file name drawn as the window title, a recent-files row "
     "and a tab label; `path` beside it is what is opened"),

    # The launcher's fallback label for a program it has no desktop entry for.
    # The input is already a `&str`, so any byte that was not text stopped
    # being one before this function was called -- there is nothing left here
    # to lose. What it produces is a caption.
    ("gui/desktop/src/lib.rs",
     ".map_or_else(|| exec.to_string(), |n| n.to_string_lossy().into_owned())",
     "a launcher caption for an executable with no desktop entry; the input "
     "is already `&str`, and `executable_path` is what is run"),

    # Searching is not opening. This is the one place a photograph's path is
    # rendered as text, and it is rendered to be compared against what the user
    # typed -- the result is a yes/no, never a name anything tries to open. A
    # byte that is not text therefore costs a search hit, not a file. The path
    # is a `PathBuf` and stays one; it was a `String` built by `to_string_lossy`
    # until 2026-09-17, and that is the defect this line is deliberately not.
    ("apps/photomanager/src/main.rs",
     "if self.file_path.to_string_lossy().to_lowercase().contains(&q) {",
     "matching a search query against a path, never opening it; the path is "
     "held as a `PathBuf` and is rendered only here"),
    ("gui/thumbs/src/lib.rs",
     "let text = String::from_utf8_lossy(&bytes);",
     "the first 4 KiB of a previewed file, drawn as text; a file's contents "
     "declare no encoding, and dropping the undecodable lines instead -- "
     "which is what this replaced -- hid them from the reader entirely"),
    ("apps/explorer/src/main.rs",
     "shown.to_string_lossy().into_owned()",
     "the label a search row is drawn with, qualified by its folder; "
     "`FileEntry::path` beside it is what opening the row uses"),
    ("apps/filesearch/src/main.rs",
     "&root.to_string_lossy(),",
     "reaches `describe_index_pass`, whose result is `self.status_message`; "
     "the crate already tracks unrepresentable names separately"),
    ("apps/jsonviewer/src/main.rs",
     ".map_or_else(|| shown.clone(), |n| n.to_string_lossy().into_owned())",
     "the tab's label; the document's path is held separately"),
    ("apps/procexplorer/src/main.rs",
     ".map(|a| String::from_utf8_lossy(a).into_owned())",
     "a process's argv rendered for a table cell, already joined with spaces "
     "-- a flattening no encoding would undo"),
    ("apps/procexplorer/src/main.rs",
     "name: String::from_utf8_lossy(&stat.comm)",
     "the process name column"),
    ("apps/editor/src/highlight.rs",
     "Language::from_extension(&ext.to_string_lossy())",
     "decoded only to MATCH an extension against a table of ASCII names, the "
     "same shape as `od`'s long-option lookup below; a name that does not "
     "decode matches nothing either way, which is the Plain it would get"),

    ("stat", "from_utf8_lossy(TERSE_FILE)",
     "TERSE_FILE is a const format string in this file; ASCII by construction"),
    ("stat", "from_utf8_lossy(TERSE_FS)",
     "TERSE_FS likewise"),
    ("expr", "BigInt::from_str(&String::from_utf8_lossy(v))",
     "guarded by looks_like_integer(v), which admits only ASCII digits and a "
     "leading `-`, so the decode is provably lossless"),
    ("od", "let typed = String::from_utf8_lossy(typed_bytes)",
     "decoded only to MATCH a long-option name, all of which are ASCII; the "
     "raw bytes are passed alongside and are what the diagnostic uses"),
    ("split", "let shown = String::from_utf8_lossy(name)",
     "`shown` reaches only println!/format!; the child's FILE= environment "
     "variable is set from os_from_bytes(name) on the next line"),
    ("strings", "let text = String::from_utf8_lossy(&bytes)",
     "`text` reaches only STRINGS.usage(format!(...))"),
)


def ignored(name, text):
    """Whether this site is in the audited-exempt table."""
    for bin_name, anchor, _why in IGNORE:
        if bin_name == name and anchor in text:
            return True
    return False


def strip_comments(text):
    """Blank out comments, keeping line numbering intact.

    A comment that names `from_utf8_lossy` -- this file's own docstring does
    it, and so do three of the binaries -- is not a call. Replacing rather
    than deleting keeps every reported line number the one a reader will find
    in their editor.
    """
    out = []
    in_block = False
    for line in text.split(NL):
        if in_block:
            end = line.find("*/")
            if end < 0:
                out.append("")
                continue
            line = " " * (end + 2) + line[end + 2:]
            in_block = False
        # A `/* ... */` that CLOSES on the same line leaves the rest of the
        # line live -- the self-test pins this, because the first draft threw
        # the remainder away and so hid any call after an inline comment.
        while True:
            start = line.find("/*")
            if start < 0:
                break
            end = line.find("*/", start + 2)
            if end < 0:
                in_block = True
                line = line[:start]
                break
            line = line[:start] + " " * (end + 2 - start) + line[end + 2:]
        # `//` outside a string literal. Good enough here: the false positive
        # is a `//` inside a string, which would only ever HIDE a call, and a
        # hidden call is caught by the self-test rather than shipped.
        at = line.find("//")
        if at >= 0:
            line = line[:at]
        out.append(line)
    return NL.join(out)


def is_test_file(rel):
    """Whether this whole file is test code by virtue of where it sits.

    A test module does not have to be `#[cfg(test)] mod tests { ... }` inside
    the file it tests. It can equally be `#[cfg(test)] mod tests;` beside a
    `tests.rs`, and then the attribute is in the PARENT -- the test file itself
    contains no `#[cfg(test)]` at all, so a scanner that looks only inside it
    reads the entire module as production code.

    That is not hypothetical: `gui/desktop/src/session/tests.rs` is declared
    exactly that way, and every lossy decode in it was being reported as a
    live defect.

    The rule is the same one `scripts/check-config-turn-guards.py` already
    uses, and it is deliberately the same rule rather than a second opinion --
    two checkers disagreeing about what counts as test code is how one of them
    ends up trusted for an answer the other would have given differently.
    """
    parts = rel.replace(os.sep, "/").split("/")
    return parts[-1] == "tests.rs" or "tests" in parts[:-1]


def production_part(text, rel=None):
    """`text` up to its test module, and the line it was cut at.

    `rel` is the file's path, needed because a file can be test code without
    containing the attribute that says so -- see [`is_test_file`].

    **Nothing is cut. Test items are blanked and the scan runs to the end.**

    This used to be `text.find("#[cfg(test)]")` followed by a truncation,
    which is only right when the first such attribute is the test module. A
    `#[cfg(test)]` on a single helper, indented inside an `impl`, is ordinary
    -- and everything after it was discarded along with the tests. In
    `apps/photomanager/src/main.rs` that attribute is on line 392 and the test
    module is on line 4640, so **4,251 lines of a 6,100-line application were
    invisible** while this checker reported a confident number about the file.
    Fifty-seven files under `gui/` and `apps/` were cut that way, hiding seven
    lossy calls.

    The work is delegated to [`rustlex.live_code`], which brace-matches every
    `#[cfg(test)]` item over `strip_noise` output -- so a brace inside a string
    or a comment cannot close an item early -- and blanks rather than cuts, so
    offsets still index the original and a reported line number is still the
    line it was on.

    **This was a solved problem before it was found here.** `rustlex`'s own
    note records the same bug measured across `userspace/`: 35,706 lines, 7% of
    that lane, with `oils/src/interp.rs` cut at line 3,348 of 109,742 by a
    `#[cfg(test)] mod stderr_tee` helper. This checker simply never adopted the
    fix. An intermediate version here cut at the first *column-0* attribute,
    which is better and still a convention rather than a rule; it is gone.

    The second return value is kept for the callers that unpack it, and is
    always `None` now: there is no cut to report the line of.
    """
    if rel is not None and is_test_file(rel):
        return "", 1
    live, _ = rustlex.live_code(text)
    return live, None


def enclosing_fn(lines, at):
    """The index of the `fn` line enclosing `at`, or None."""
    for i in range(at, -1, -1):
        if re.match(r"\s*(pub\s+)?(const\s+|async\s+|unsafe\s+|extern\s+\S+\s+)*fn\s", lines[i]):
            return i
    return None


def is_host_only(lines, fn_at):
    """Whether the function at `fn_at` is compiled only off-target.

    The `#[cfg(not(unix))]` half of a pair like `quoting::os_bytes` never runs
    on SlateOS -- the target is `unix` -- so a lossy decode inside it cannot
    corrupt anything the OS does. It is a developing-on-Windows concession and
    is documented as one.
    """
    for i in range(fn_at - 1, max(-1, fn_at - 6), -1):
        line = lines[i].strip()
        if not line.startswith("#["):
            if line and not line.startswith("///"):
                break
            continue
        if "cfg(not(unix))" in line or "cfg(windows)" in line:
            return True
    return False


def enclosing_cfgs(lines, at):
    """Every `#[cfg(...)]` on a block or item that ENCLOSES line `at`.

    An attribute on the function is only one of the two shapes this has to
    catch. The other is a bare block inside a function:

    ```text
    #[cfg(unix)]
    { PathBuf::from(OsStr::from_bytes(bytes)) }
    #[cfg(not(unix))]
    { PathBuf::from(String::from_utf8_lossy(bytes).into_owned()) }
    ```

    `test.rs` writes `path_of` exactly that way, and the first draft of this
    checker reported the second block as a VALUE defect -- a false positive on
    the one file whose shape most resembled the real bug it was built to find.
    Walking out by brace depth catches both, and does not catch a cfg block
    that has already CLOSED above the call.
    """
    found = []
    depth = 0
    for i in range(at, -1, -1):
        depth += lines[i].count("}") - lines[i].count("{")
        if depth < 0:
            for j in (i, i - 1, i - 2):
                if j >= 0 and "#[cfg(" in lines[j]:
                    found.append(lines[j])
                    break
            depth = 0
    return found


def classify(lines, at):
    """`HOST`, `DIAG` or `VALUE` for the call on line index `at`."""
    fn_at = enclosing_fn(lines, at)
    if fn_at is not None and is_host_only(lines, fn_at):
        return "HOST"
    for cfg in enclosing_cfgs(lines, at):
        if "cfg(not(unix))" in cfg or "cfg(windows)" in cfg:
            return "HOST"
    # The statement around the call, not a fixed window: a `format!` opening
    # four lines above its argument is still that argument's destination.
    lo = at
    while lo > 0 and not re.search(r"[;{}]\s*$", lines[lo - 1]):
        lo -= 1
        if at - lo > 12:
            break
    hi = at
    while hi < len(lines) - 1 and not re.search(r"[;{}]\s*$", lines[hi]):
        hi += 1
        if hi - at > 12:
            break
    if DIAGNOSTIC.search(NL.join(lines[lo:hi + 1])):
        return "DIAG"

    # A BINDING is followed to its uses. `let shown = from_utf8_lossy(name);`
    # printed three lines later is a message, and the statement window above
    # cannot see that. Five of the nine sites this checker first reported were
    # exactly that shape -- including `split`'s, where the binding is printed
    # while the real bytes go to the child's environment beside it.
    m = re.match(r"\s*let\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::[^=]*)?=", lines[at])
    if m:
        name = m.group(1)
        fn_at = enclosing_fn(lines, at) or 0
        end = fn_at
        depth = 0
        seen_open = False
        for i in range(fn_at, len(lines)):
            depth += lines[i].count("{") - lines[i].count("}")
            if lines[i].count("{"):
                seen_open = True
            if seen_open and depth <= 0:
                end = i
                break
        else:
            end = len(lines) - 1
        uses = [l for l in lines[at + 1:end + 1]
                if re.search(r"\b" + re.escape(name) + r"\b", l)]
        if uses and all(DIAGNOSTIC.search(u) for u in uses):
            return "DIAG"
    return "VALUE"


def scan(path, name=None):
    """Every lossy call in `path`, as `(line_number, kind, text)`.

    A site in [`IGNORE`] is reported as `OK` rather than dropped, so that
    `--show all` still shows it and an exemption cannot quietly cover a line
    that has since changed -- the anchor stops matching and the site comes
    back as a VALUE.
    """
    raw = io.open(path, encoding="utf-8", errors="replace").read()
    prod, _ = production_part(raw, name)
    prod = strip_comments(prod)
    lines = prod.split(NL)
    found = []
    for i, line in enumerate(lines):
        if any(c in line for c in CALLS):
            kind = classify(lines, i)
            if kind == "VALUE" and name and ignored(name, line):
                kind = "OK"
            found.append((i + 1, kind, line.strip()[:74]))
    return found


def image_binaries():
    """The binaries the image actually ships, from the rootfs manifest."""
    names = []
    p = os.path.join(ROOT, "scripts", "rootfs-bin-manifest.txt")
    for line in io.open(p, encoding="utf-8"):
        line = line.strip()
        if line and not line.startswith("#") and "=" not in line:
            names.append(line)
    paths = []
    base = os.path.join(ROOT, "userspace", "coreutils", "src", "bin")
    for n in names:
        for cand in (os.path.join(base, n + ".rs"),
                     os.path.join(base, n, "main.rs")):
            if os.path.isfile(cand):
                paths.append((n, cand))
                break
    return paths


def all_sources(roots=("userspace",)):
    """Every `.rs` under each of `roots`.

    Parameterised rather than hardcoded to `userspace/` because that default
    silently scoped this checker to one lane's tree. The defect it looks for is
    not a userspace defect -- it is a consequence of `design.txt` allowing every
    byte but `/` and NUL in a name, which binds equally to `gui/` and `apps/`.
    Five instances were found there by hand on 2026-09-16, in a tree this
    checker could not see. See `known-issues.md`
    `TD-C-THE-LOSSY-DECODE-CHECKER-NEVER-LOOKED-AT-TWO-THIRDS-OF-THE-TREE`.
    """
    out = []
    seen = set()
    for root in roots:
        base = os.path.join(ROOT, *root.split("/"))
        if not os.path.isdir(base):
            continue
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames[:] = [d for d in dirnames if d != "target"]
            for f in filenames:
                if f.endswith(".rs"):
                    full = os.path.join(dirpath, f)
                    # Forward slashes always, whatever the host separator
                    # happens to be. The IGNORE table and the baseline are
                    # both checked into git and read on three machines, so
                    # a key spelled with the host's separator matches on
                    # that host and silently nowhere else.
                    #
                    # This is not theoretical: the first fifteen
                    # exemptions written for these trees had no effect at
                    # all. They were spelled with forward slashes, the
                    # scan produced the Windows form, the comparison was
                    # equality, and nothing anywhere reported a mismatch
                    # -- the run simply came back with the same count it
                    # started with. An exemption that matches nothing and
                    # an exemption that was never written look identical
                    # from the outside, which is why the count was checked
                    # rather than assumed.
                    rel = os.path.relpath(full, ROOT).replace(os.sep, "/")
                    if rel in seen:
                        continue
                    seen.add(rel)
                    out.append((rel, full))
    return out


def selftest():
    bad = 0
    checks = 0

    def ck(ok, msg):
        nonlocal bad, checks
        checks += 1
        if not ok:
            print("selftest FAIL: " + msg, file=sys.stderr)
            bad += 1

    def kinds(src):
        src = strip_comments(production_part(src)[0])
        lines = src.split(NL)
        return [classify(lines, i) for i, l in enumerate(lines)
                if any(c in l for c in CALLS)]

    # A file that is test code by where it sits, not by what it contains.
    # The rule has to be tested through `production_part` rather than through
    # `is_test_file` alone, because the bug it fixes was that the caller never
    # asked -- a correct predicate nothing consults reads exactly like no
    # predicate at all.
    body = "fn f() {" + NL + "    let p = x.to_string_lossy();" + NL + "}"
    ck(production_part(body, "gui/desktop/src/session/tests.rs")[0] == "",
       "a `tests.rs` is entirely test code, whatever it contains")
    ck(production_part(body, "apps/x/tests/helper.rs")[0] == "",
       "a file under a `tests/` directory likewise")
    ck(production_part(body, "apps/x/src/main.rs")[0] == body,
       "an ordinary source file is still production")
    ck(production_part(body, "apps/x/src/attestations.rs")[0] == body,
       "a name merely CONTAINING `tests` is not a test file")
    ck(is_test_file("a/tests.rs") and not is_test_file("a/tests.rs.bak"),
       "the rule matches the file name, not a prefix of it")

    # A plain value conversion is the thing this exists to find.
    ck(kinds("fn f() {" + NL + "    let p = x.to_string_lossy();" + NL + "}") == ["VALUE"],
       "a bare conversion is a VALUE")

    # ...and the real defect's shape, which was an argument to a call.
    ck(kinds("fn f() {" + NL + "    g(&p.to_string_lossy(), 1);" + NL + "}") == ["VALUE"],
       "a conversion passed as an argument is a VALUE")

    # A message is not corruption.
    ck(kinds("fn f() {" + NL + '    panic!("{}", String::from_utf8_lossy(v));' + NL + "}") == ["DIAG"],
       "a panic argument is a DIAG")

    # The destination may be several lines above the argument, which is why
    # this looks at the statement rather than a fixed window.
    multi = ("fn f() {" + NL
             + "    return Err(format!(" + NL
             + '        "bad thing: {}",' + NL
             + "        String::from_utf8_lossy(&value)" + NL
             + "    ));" + NL + "}")
    ck(kinds(multi) == ["DIAG"], "a format! four lines up still claims its argument")

    # The off-target half of a pair cannot corrupt the target.
    host = ("#[cfg(not(unix))]" + NL
            + "fn os_from_bytes(b: &[u8]) -> OsString {" + NL
            + "    OsString::from(String::from_utf8_lossy(b).into_owned())" + NL + "}")
    ck(kinds(host) == ["HOST"], "a cfg(not(unix)) fallback is HOST")

    # The other shape of the same thing: a bare cfg BLOCK inside a function,
    # which is how `test.rs` writes `path_of`. The first draft of this checker
    # called that a defect.
    blockform = ("fn path_of(bytes: &[u8]) -> PathBuf {" + NL
                 + "    #[cfg(unix)]" + NL
                 + "    {" + NL
                 + "        PathBuf::from(OsStr::from_bytes(bytes))" + NL
                 + "    }" + NL
                 + "    #[cfg(not(unix))]" + NL
                 + "    {" + NL
                 + "        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())" + NL
                 + "    }" + NL + "}")
    ck(kinds(blockform) == ["HOST"], "a cfg(not(unix)) BLOCK is HOST")

    # ...and a cfg block that has CLOSED above the call does not cover it.
    closed = ("fn f(b: &[u8]) {" + NL
              + "    #[cfg(not(unix))]" + NL
              + "    {" + NL
              + "        let _ = 1;" + NL
              + "    }" + NL
              + "    let p = String::from_utf8_lossy(b).into_owned();" + NL
              + "    use_it(p);" + NL + "}")
    ck(kinds(closed) == ["VALUE"], "a cfg block that already closed does not cover the call")

    # ...but the unix half of the same pair is not exempt by association.
    both = ("#[cfg(unix)]" + NL
            + "fn a(b: &[u8]) -> OsString {" + NL
            + "    OsString::from(String::from_utf8_lossy(b).into_owned())" + NL + "}")
    ck(kinds(both) == ["VALUE"], "a cfg(unix) function is NOT exempt")

    # A binding used only in messages is a message, however far away.
    bound = ("fn f(name: &[u8]) {" + NL
             + "    let shown = String::from_utf8_lossy(name).into_owned();" + NL
             + "    if verbose {" + NL
             + '        println!("executing with FILE={shown}");' + NL
             + "    }" + NL
             + "    go();" + NL + "}")
    ck(kinds(bound) == ["DIAG"], "a binding used only in a println is a DIAG")

    # ...and a binding that reaches real work is still a VALUE. Without this
    # the rule above would excuse every decode that is ALSO printed.
    both_uses = ("fn f(name: &[u8]) {" + NL
                 + "    let shown = String::from_utf8_lossy(name).into_owned();" + NL
                 + '    println!("{shown}");' + NL
                 + "    open_the_file(&shown);" + NL + "}")
    ck(kinds(both_uses) == ["VALUE"], "a binding that also reaches real work is a VALUE")

    # A comment naming the call is not a call. This file's own docstring would
    # otherwise report itself.
    ck(kinds("fn f() {" + NL + "    // from_utf8_lossy would be wrong here" + NL + "}") == [],
       "a comment mentioning the name is not a call")
    ck(kinds("fn f() {" + NL + "    let x = 1; // to_string_lossy" + NL + "}") == [],
       "a trailing comment is not a call")

    # Test code is out of scope: a lossy decode in an assertion message is not
    # a defect, and 85 of the 131 raw hits on the image are exactly that.
    intest = ("fn f() {}" + NL + "#[cfg(test)]" + NL + "mod tests {" + NL
              + "    fn g() { let s = String::from_utf8_lossy(v); }" + NL + "}")
    ck(kinds(intest) == [], "a call inside the test module is not counted")

    # A block comment spanning lines must not swallow real code after it.
    blk = ("fn f() {" + NL + "    /* from_utf8_lossy */ let p = x.to_string_lossy();" + NL + "}")
    ck(kinds(blk) == ["VALUE"], "code after a block comment is still scanned")

    # The manifest must resolve to real files, or this grades nothing.
    bins = image_binaries()
    ck(len(bins) > 50, "the image manifest should resolve to most of its binaries, got "
       + str(len(bins)))

    print("selftest: " + str(checks - bad) + "/" + str(checks) + " cases pass")
    return 1 if bad else 0


BASELINE = os.path.join(ROOT, "scripts", "lossy-decode-baseline.txt")


def baseline_key(rel, count):
    """One baselined file: its path, the class, and how many sites it holds.

    The count is part of the key on purpose. The sibling ratchet
    (`argv-utf8-baseline.txt`) keys on path alone, which is right there --
    "does this program read argv as a String" is a property of the program.
    Here it is not: a file with four lossy decodes can grow a fifth without
    changing a path-only key, and that fifth is exactly what this gate exists
    to stop. Carrying the count makes a new site in an already-listed file a
    failure, at the price that a refactor which legitimately moves one has to
    re-run `--write-baseline`.
    """
    return "%s:VALUE:%d" % (rel.replace(os.sep, "/"), count)


def load_baseline():
    """The baselined backlog, and the roots it was taken over.

    Read from disk rather than from the git tree, unlike `argv-utf8.py`, and
    the difference is deliberate. That checker loops over the shas being
    pushed, so it must read a baseline from the same sha or it answers for a
    tree nobody is pushing. This one runs once against the **working tree** --
    the pre-push hook says so where it invokes it -- so disk *is* the tree
    being judged, and reading from git would compare a working-tree scan
    against a committed baseline: a mismatch on every uncommitted edit.
    """
    roots = ()
    known = {}
    if not os.path.isfile(BASELINE):
        return roots, known
    for line in io.open(BASELINE, encoding="utf-8", errors="strict"):
        line = line.strip()
        if line.startswith("# roots:"):
            roots = tuple(line.split(":", 1)[1].split())
            continue
        if not line or line.startswith("#"):
            continue
        rel, _kind, count = line.rsplit(":", 2)
        known[rel] = int(count)
    return roots, known


def value_counts(targets):
    """`{relpath: number of VALUE sites}` for every file that has any."""
    counts = {}
    for name, path in targets:
        n = sum(1 for _, kind, _ in scan(path, name) if kind == "VALUE")
        if n:
            counts[name.replace(os.sep, "/")] = n
    return counts


def write_baseline(roots, counts):
    lines = [
        "# Lossy byte->text conversions that reach a VALUE, in trees",
        "# `lossy-decode.py` does not scan by default. Generated by",
        "# `scripts/lossy-decode.py --under <root> ... --write-baseline`.",
        "#",
        "# This file is a ratchet and only ever shrinks. Do NOT add a line to",
        "# turn a red --check green: a new entry is a new place where a name",
        "# that is not text becomes a name nobody can open.",
        "#",
        "# A site that is genuinely safe does not belong here at all -- it",
        "# belongs in the IGNORE table in the script, which records WHY. This",
        "# file records only THAT, and a backlog of `that` is what stops",
        "# anyone reading the list.",
        "#",
        "# The count on each line is part of the key: a fifth decode in a file",
        "# already listed with four is a new defect and fails --check.",
        "#",
        "# roots: " + " ".join(roots),
        "",
    ]
    for rel in sorted(counts):
        lines.append(baseline_key(rel, counts[rel]))
    with io.open(BASELINE, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(lines) + "\n")
    return len(counts)


def check_against_baseline(roots, counts):
    """Fail on a finding not in the baseline, and on a baseline line gone stale.

    Both directions, for the reason `argv-utf8.py` records from experience: a
    ratchet that checks only one of them accumulates dead lines, and a baseline
    carrying already-fixed entries reads as a backlog that is not shrinking
    when in fact it has.
    """
    want_roots, known = load_baseline()
    if want_roots and tuple(roots) != want_roots:
        print("lossy-decode: --check asked for roots %s but the baseline was "
              "taken over %s. Comparing them would report every file in one "
              "and not the other as new."
              % (" ".join(roots), " ".join(want_roots)), file=sys.stderr)
        return 1

    worse = []
    for rel in sorted(counts):
        was = known.get(rel)
        if was is None:
            worse.append("  NEW   %s has %d lossy decode(s) reaching a value"
                         % (rel, counts[rel]))
        elif counts[rel] > was:
            worse.append("  MORE  %s went from %d to %d"
                         % (rel, was, counts[rel]))

    stale = []
    for rel in sorted(known):
        now = counts.get(rel, 0)
        if now == 0:
            stale.append("  GONE  %s is clean now; drop its line" % rel)
        elif now < known[rel]:
            stale.append("  FEWER %s went from %d to %d; lower its line"
                         % (rel, known[rel], now))

    if worse:
        print("lossy-decode: new lossy decode(s) reaching a value:",
              file=sys.stderr)
        for line in worse:
            print(line, file=sys.stderr)
        print("", file=sys.stderr)
        print("Do not add these to the baseline. Either the bytes are a path "
              "and this is a defect, or they come from a format that defines "
              "its own encoding -- in which case the site belongs in IGNORE "
              "in the script, with the reason.", file=sys.stderr)
    if stale:
        print("lossy-decode: the baseline is out of date (this is good news):",
              file=sys.stderr)
        for line in stale:
            print(line, file=sys.stderr)
        print("", file=sys.stderr)
        print("Re-run with --write-baseline to record the improvement.",
              file=sys.stderr)

    if not worse and not stale:
        print("lossy-decode: %d file(s) baselined, none worse." % len(known))
    return 1 if (worse or stale) else 0


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--selftest", "--self-test", dest="selftest", action="store_true")
    ap.add_argument("--all", action="store_true",
                    help="every .rs under userspace/, not just the image's binaries")
    ap.add_argument("--under", action="append", metavar="DIR", default=None,
                    help="scan every .rs under DIR instead (repeatable); "
                         "e.g. --under gui --under apps for lane C's tree")
    ap.add_argument("--write-baseline", action="store_true",
                    help="record the current findings as the ratchet's floor")
    ap.add_argument("--check", action="store_true",
                    help="fail on a finding not in the baseline, or on a "
                         "baseline line that is no longer true")
    ap.add_argument("--show", choices=("value", "diag", "host", "ok", "all"),
                    default="value")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    if args.write_baseline or args.check:
        if not args.under:
            print("lossy-decode: --write-baseline/--check need --under DIR; "
                  "the default scope is already a hard failure, not a ratchet.",
                  file=sys.stderr)
            return 2
        roots = tuple(args.under)
        counts = value_counts(all_sources(roots))
        if args.write_baseline:
            n = write_baseline(roots, counts)
            print("lossy-decode: baseline written -- %d file(s), %d site(s), "
                  "over %s" % (n, sum(counts.values()), " ".join(roots)))
            return 0
        return check_against_baseline(roots, counts)

    if args.under:
        targets = all_sources(tuple(args.under))
    elif args.all:
        targets = all_sources()
    else:
        targets = image_binaries()
    totals = {"VALUE": 0, "DIAG": 0, "HOST": 0, "OK": 0}
    per_bin = []
    for name, path in targets:
        hits = scan(path, name)
        if not hits:
            continue
        counts = {"VALUE": 0, "DIAG": 0, "HOST": 0, "OK": 0}
        for _, kind, _text in hits:
            counts[kind] += 1
            totals[kind] += 1
        per_bin.append((name, counts, hits))

    want = {"value": ("VALUE",), "diag": ("DIAG",), "host": ("HOST",),
            "ok": ("OK",),
            "all": ("VALUE", "DIAG", "HOST", "OK")}[args.show]
    per_bin.sort(key=lambda r: -sum(r[1][k] for k in want))
    for name, counts, hits in per_bin:
        if not sum(counts[k] for k in want):
            continue
        print(name + ":")
        for ln, kind, text in hits:
            if kind in want:
                print("    %-6s line %-5d %s" % (kind, ln, text))

    # A SCAN THAT READ ALMOST NOTHING MUST NOT REPORT SUCCESS. Zero findings
    # is the expected state here, so "clean" and "the manifest resolved to
    # nothing" produce the same number -- and only this tells them apart. The
    # sibling checkers carry the same guard for the same reason.
    if len(targets) < 40:
        print("lossy-decode: REFUSING -- only %d source file(s) found, which is "
              "too few to conclude anything. Is the manifest readable?"
              % len(targets), file=sys.stderr)
        return 2

    print("")
    print("VALUE %d   DIAG %d   HOST %d   OK %d   (in %d file(s), %d scanned)"
          % (totals["VALUE"], totals["DIAG"], totals["HOST"], totals["OK"],
             len(per_bin), len(targets)))
    print("")
    print("VALUE is the only column that means 'silent corruption'. DIAG is a")
    print("name rendered into a message -- `quoting::quotef` is the better")
    print("answer there, but it is a display defect, not a data one. HOST is")
    print("the `cfg(not(unix))` half of a pair whose unix half the target")
    print("compiles instead, so it never runs on SlateOS. OK is a VALUE site")
    print("audited and exempted in the script's IGNORE table, which records")
    print("why; edit the line and the anchor stops matching, so the exemption")
    print("expires with the code it was granted for.")
    if totals["VALUE"]:
        print("", file=sys.stderr)
        print("A VALUE site is a byte sequence being turned into text with "
              "U+FFFD in place of whatever could not be decoded, and then "
              "USED. On this OS a file name may hold every byte but `/` and "
              "NUL, so that is a name nobody can open.", file=sys.stderr)
        print("If it is genuinely safe -- an ASCII constant, a guarded "
              "decode -- add it to IGNORE in the script WITH THE REASON, "
              "not to a list that records only that it exists.", file=sys.stderr)
    return 1 if totals["VALUE"] else 0


if __name__ == "__main__":
    sys.exit(main())
