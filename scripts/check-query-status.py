#!/usr/bin/env python3
"""Guard the rule that answering a question is *not* reporting a failure.

The rule
--------
**If a kshell block can only be reached by the user asking -- no argument was
given -- and it answers by printing program state, it must not set a failure
status.**

This is the mirror of `check-usage-status.py`, and it exists because that
checker's rule, applied without this one, produces the opposite bug.  "Print a
usage line, set a failure status" is right for a complaint and wrong for a
hint, and the two are written identically:

    if parts.len() < 2 {
        shell_println!("Serial echo level: {} (and above)", level.as_str());
        shell_println!("Usage: elog echo <level>  to change");
        set_exit(1);            // <-- the answer above is correct
        return;
    }

`elog echo` and `fc algo` shipped exactly that for a month: they printed the
right answer and then told the caller they had failed, because a sweep that
was fixing genuine missing statuses could not tell a query from a complaint.
`$(elog echo)` in a script under `set -e` kills the script *after* producing
the value it wanted.

Why a second checker rather than a rule inside the first
--------------------------------------------------------
Because the two rules point in opposite directions and share no machinery
worth sharing.  A single script that decided both would have to hold "this
needs a status" and "this must not have one" in one classifier, and the first
time they disagreed the disagreement would be silent.  Kept apart, each one
states a property, and a site that both flag is a site whose author has to
say which it is -- which is the conversation that should happen.

What is checked
---------------
For each non-zero `set_exit`, find the block that encloses it and ask three
questions:

1. **Is the block guarded on "no argument was given"?**  `parts.is_empty()`,
   `parts.len() < 2`, a `None =>` arm of a `match` on an argument accessor.
   Negated forms (`!x.is_empty()`) mean an argument *was* given and do not
   count.
2. **Does it answer?**  At least one print *directly* in the block (not in a
   nested arm) whose argument list reads program state -- a path like
   `quota::is_enabled()` or a method call -- rather than only literal text.
3. **Does it then fail?**  The `set_exit` that started the walk.

All three, and the site is reported.

Testing it
----------
Pass a path to run against an older revision.  The positive control is the
revision before the fix:

    git show 9251e5a3d^:kernel/src/kshell.rs > /tmp/old.rs
    python scripts/check-query-status.py /tmp/old.rs

which must report `cmd_fcompress` ("Current: {}") and `cmd_elog` ("Serial echo
level: ..."), the two sites that shipped the bug.  A checker nobody has
watched fail is a checker nobody knows works.

``--self-test`` is the same idea without needing the history: it runs the scan
over fixture functions -- one per clause above, each guard form, the negated
form, a ``None`` arm on the user's words and on a lookup, a print in a nested
arm, a status named only in a comment, the ``} else {`` sibling, a print
rustfmt wrapped over several lines, ``set_exit(0)``, and a used and an unused
ALLOWED entry -- and checks each verdict. The push hook and the boot test run
it before trusting the real verdict (design-decisions §974).

Exit status: 0 clean, 1 sites found, 2 bad arguments.
"""

import pathlib
import re
import sys

# `strip_noise` is the directory's one self-tested Rust scanner. The filename's
# hyphens make it un-`import`able normally, hence the load by path -- the same
# check-selftest-skips.py, check-vfs-permission-gate.py and
# check-vfs-under-lock.py already use.
_SIBLING = pathlib.Path(__file__).resolve().parent / "check-recursive-locks.py"
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402
import srcload  # noqa: E402

# Loaded from source rather than through `importlib`: a `SourceFileLoader`
# consults `__pycache__`, whose staleness check is `(mtime, size)` at
# one-second resolution, so two same-size writes to the sibling inside one
# second leave the second one invisible and this script silently runs the
# previous version of it. See `scripts/srcload.py`.
try:
    _rl = srcload.load(str(_SIBLING), "check_recursive_locks")
except OSError as _exc:  # pragma: no cover - packaging error
    print(f"error: cannot load {_SIBLING}: {_exc}", file=sys.stderr)
    raise SystemExit(2) from _exc

PATH = pathlib.Path(__file__).resolve().parent.parent / "kernel" / "src" / "kshell.rs"

# Blocks that match the shape and are *right* to fail.  Keyed (function,
# fragment of the printed text), like ALLOWED in check-usage-status.py, and
# for the same reason: a line number drifts on every edit and would rot the
# list into a rubber stamp.  Each entry needs a reason.
#
# Empty, and deliberately so.  It held two entries from the day this checker
# was written -- `cmd_dpkg_extract` ("dpkg: no data.tar found in") and
# `cmd_archive` ("archive: {}: unknown archive format"), both reasoned as *the
# `None` is a failed lookup, not an argument the user omitted*.  Both sites are
# still in kshell.rs and both readings are still correct, but neither entry
# ever exempted anything: replaying the introducing commit (425d37b27) with
# ALLOWED emptied reports zero findings, so the detector has never reached
# those two blocks.  They were written against a shape the checker does not
# actually match.
#
# An exemption that exempts nothing is precisely the rubber stamp the comment
# above warns about, so they are recorded here as prose instead of carried as
# live entries.  If the guard rule is ever widened and those two blocks start
# being reported, the reasoning to paste back is in this comment.
ALLOWED: dict[tuple[str, str], str] = {}

FN = re.compile(r"(?:pub )?(?:async )?fn ([a-z_0-9]+)")
FAIL = re.compile(r"set_exit\(\s*([1-9]\d*)\s*\)")
PRINT = re.compile(r"(?:console_print(?:ln)?!|shell_print(?:ln)?!)\s*\(\s*(.*)$")

# "No argument was given."  The negated forms are deliberately excluded: the
# lookbehind on the `is_empty` alternative rejects both `!x.is_empty()` and
# `!x.y.is_empty()`, which say an argument *was* given and whose blocks are
# ordinary error paths.
GUARD = re.compile(
    r"""(?x)
    (?:^|[^!\w])(?:parts|args|argv|words|toks|tokens|fields)
        \s*\.\s*len\s*\(\s*\)\s*(?:<\s*\d|==\s*[01]\b)
  | (?<![!.\w])[a-z_][a-z_0-9]*(?:\.[a-z_][a-z_0-9]*)*\.is_empty\(\)
  | \.\s*get\s*\(\s*\d+\s*\)[^;{]*\.\s*is_none\s*\(\s*\)
  | ^\s*None\s*=>
  | ^\s*""\s*=>
    """
)

# A `match` whose scrutinee is the user's words.  Only these make a `None =>`
# arm mean "the user gave no argument"; `match some_lookup() { None => ... }`
# means "it wasn't there", which is a genuine failure.
ARG_MATCH = re.compile(r"\bmatch\b[^{]*\b(?:parts|args|argv|words|sub|arg)\b")

# An argument list that reads program state: a module path or a method/field
# access, rather than only literal text or the user's own words echoed back.
STATE = re.compile(r"::|\.\s*[a-z_][a-z_0-9]*")


def block_start(struct, i):
    """The line holding the brace that opens the block containing line `i`.

    Braces are counted a character at a time, not a line at a time.  `} else {`
    closes and opens on one line, so a line-granular count nets to zero and the
    walk sails straight past the brace that actually delimits the block --
    which is how a sibling `else` branch gets mistaken for the guarded one.

    `struct` must already have comments *and* literals blanked -- see the note
    in [`main`] on why counting braces over comment text is a silent defect.
    """
    depth = 0
    start = i
    for k in range(i - 1, max(-1, i - 400), -1):
        for ch in reversed(struct[k]):
            depth += -1 if ch == "{" else (1 if ch == "}" else 0)
            if depth < 0:
                break
        start = k
        if depth < 0:
            break
    return start


def block_end(struct, i):
    """The line holding the brace that closes the block containing line `i`."""
    depth = 0
    end = i
    for k in range(i + 1, min(len(struct), i + 400)):
        for ch in struct[k]:
            depth += 1 if ch == "{" else (-1 if ch == "}" else 0)
            if depth < 0:
                break
        end = k
        if depth < 0:
            break
    return end


def scan(text, allowed):
    """The whole rule, over one file's text: `(hits, stale)`. Separate from
    `main` so the self-test runs exactly this over fixture text."""
    # Three views of the same file, all with identical line numbering because
    # `strip_noise` blanks in place rather than deleting.
    #
    #   lines   verbatim; used only to report a finding back to the reader.
    #   code    comments blanked, literals kept; everything this checker
    #           *matches* on lives in a literal (`shell_println!("...")`), and
    #           nothing it matches on should be found in prose.
    #   struct  comments and literals blanked; the only thing braces may be
    #           counted over.
    #
    # `struct` is the one that was wrong. It used to be a one-line
    # `strip_strings` applied to raw lines, so braces inside *comments* counted
    # as structure -- and kshell.rs holds 26 comments carrying an unbalanced
    # brace, which is precisely what a comment is allowed to carry:
    #
    #     i = i.saturating_add(1);  // skip `{`
    #     /// Brace nesting depth.  Starts at 1 (the opening `{`).
    #
    # Any of those inside the 400-line scan window shifts the computed block
    # boundary, so the checker then reads a range it was never asked about --
    # missing real findings and manufacturing false ones, with no diagnostic
    # either way. Two of the five hand-rolled strippers in this directory had
    # written that hazard down independently; this one, which needed it most,
    # had not. It is now the shared, self-tested scanner for all of them.
    lines = text.split("\n")
    code = _rl.strip_noise(text, keep_literals=True).split("\n")
    struct = _rl.strip_noise(text).split("\n")

    starts = [(i, m.group(1)) for i, ln in enumerate(lines) if (m := FN.match(ln))]

    def fn_of(i):
        name = "?"
        for s, n in starts:
            if s <= i:
                name = n
            else:
                break
        return name

    hits = []
    fired: set[tuple[str, str]] = set()
    for i, ln in enumerate(code):
        # No `startswith("//")` guard is needed any more: a `set_exit(1)`
        # written in a comment -- whole-line *or* trailing, which that guard
        # never caught -- is already blanked out of `code`.
        if not FAIL.search(ln):
            continue

        start = block_start(struct, i)
        guard = code[start]
        if not GUARD.search(guard):
            continue
        # A `None =>` / `"" =>` arm only means "no argument" if the match is on
        # the user's words.
        if re.match(r"\s*(?:None|\"\")\s*=>", guard):
            if not ARG_MATCH.search(code[block_start(struct, start)]):
                continue

        end = block_end(struct, i)
        answers = []
        rel = 0
        for k in range(start, end + 1):
            here = rel
            for ch in struct[k]:
                rel += 1 if ch == "{" else (-1 if ch == "}" else 0)
            m = PRINT.search(code[k])
            # A print inside a nested arm belongs to that arm, not to the
            # query, so only lines sitting directly in this block count.
            if not m or (k != start and here != 1):
                continue
            payload = m.group(1)
            j = k
            while payload.count("(") - payload.count(")") >= 0 and j + 1 <= end:
                j += 1
                payload += " " + code[j].strip()
            bits = payload.split('"')
            if len(bits) < 3:
                continue  # no argument list: literal text only
            if STATE.search('"'.join(bits[2:])):
                answers.append(payload.strip())
        if not answers:
            continue

        fn = fn_of(i)
        used = {(f, frag) for (f, frag) in allowed if fn == f and any(frag in a for a in answers)}
        if used:
            fired |= used
            continue
        hits.append((i + 1, fn, answers[0][:96]))

    # An exemption that exempts nothing is a defect in its own right, and this
    # checker shipped with two of them -- inert from the day it was written,
    # because they described a shape the detector never matched. Nothing
    # reported that, because nothing looked. Its mirror check-usage-status.py
    # has carried the equivalent guard on its ledger from the start; this is
    # that guard. Reported only when the run is otherwise clean, so a real
    # finding is never buried under bookkeeping.
    return hits, sorted(set(allowed) - fired)


def main(argv):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    unknown = selftestflag.unknown_options(argv[1:])
    if unknown:
        print(f"check-query-status: unrecognised option {unknown[0]!r}", file=sys.stderr)
        return 2
    if selftestflag.wants_selftest(argv[1:]):
        return self_test()
    path = pathlib.Path(argv[1]) if len(argv) > 1 else PATH
    text = path.read_text(encoding="utf-8", errors="surrogateescape")
    hits, stale = scan(text, ALLOWED)
    if stale and not hits:
        print("", file=sys.stderr)
        print(
            "ALLOWED entries that exempted nothing (detector no longer reaches "
            "them, or never did) -- remove them from check-query-status.py:",
            file=sys.stderr,
        )
        for f, frag in stale:
            print(f"  {f}: {frag!r}", file=sys.stderr)
        return 1

    if not hits:
        print(
            f"[query-status] {path.name}: no query answers correctly and then reports "
            f"failure ({len(ALLOWED)} allowed)"
        )
        return 0

    print("", file=sys.stderr)
    print(
        f"{len(hits)} block(s) reachable only by asking, which answer and then "
        f"report FAILURE:", file=sys.stderr
    )
    for ln, fn, answer in hits:
        print(f"  {path}:{ln}  {fn}", file=sys.stderr)
        print(f"      {answer}", file=sys.stderr)
    return 1




# --- Self-test ---------------------------------------------------------------
#
# One fixture function per clause of the rule. `_REPORTED` is the set that
# must come back as hits; every other function in the fixture must not.

_FIXTURE = r'''
fn cmd_query_fails(parts: &[&str]) {
    if parts.len() < 2 {
        shell_println!("Level: {}", log::level());
        shell_println!("Usage: q <level>  to change");
        set_exit(1);
        return;
    }
}

fn cmd_query_ok(parts: &[&str]) {
    if parts.len() < 2 {
        shell_println!("Level: {}", log::level());
        return;
    }
}

fn cmd_complaint(parts: &[&str]) {
    if parts.len() < 2 {
        shell_println!("Usage: c <x>");
        set_exit(1);
        return;
    }
}

fn cmd_negated(x: &str) {
    if !x.is_empty() {
        shell_println!("Bad value: {}", cfg.value());
        set_exit(1);
    }
}

fn cmd_none_arm_args(parts: &[&str]) {
    match parts.get(1) {
        None => {
            shell_println!("Mode: {}", mode::current());
            set_exit(1);
        }
        Some(m) => set(m),
    }
}

fn cmd_none_arm_lookup(name: &str) {
    match lookup(name) {
        None => {
            shell_println!("not found: {}", name.len());
            set_exit(1);
        }
        Some(v) => take(v),
    }
}

fn cmd_empty_string_arm(sub: &str) {
    match sub {
        "" => {
            shell_println!("Status: {}", svc::status());
            set_exit(1);
        }
        _ => {}
    }
}

fn cmd_nested_arm(parts: &[&str]) {
    if parts.is_empty() {
        match mode {
            X => shell_println!("x: {}", st.x()),
            _ => {}
        }
        set_exit(1);
    }
}

fn cmd_comment_fail(parts: &[&str]) {
    if parts.is_empty() {
        shell_println!("Current: {}", cfg.current());
        // set_exit(1) would be wrong here: this is the answer.
    }
}

fn cmd_else_branch(parts: &[&str]) {
    if parts.is_empty() {
        shell_println!("Current: {}", cfg.current());
    } else {
        shell_println!("Unknown: {}", parts.len());
        set_exit(1);
    }
}

fn cmd_wrapped_query(parts: &[&str]) {
    if parts.len() < 2 {
        shell_println!(
            "Echo level: {} (and above)",
            elog::echo_level().as_str()
        );
        set_exit(1);
    }
}

fn cmd_exit_zero(parts: &[&str]) {
    if parts.is_empty() {
        shell_println!("Current: {}", cfg.current());
        set_exit(0);
    }
}

fn cmd_exempt(parts: &[&str]) {
    if parts.is_empty() {
        shell_println!("Exempt: {}", st.value());
        set_exit(1);
    }
}
'''

_REPORTED = {
    "cmd_query_fails",
    "cmd_none_arm_args",
    "cmd_empty_string_arm",
    "cmd_wrapped_query",
}

_FIXTURE_ALLOWED = {
    ("cmd_exempt", "Exempt:"): "the fixture's one used exemption",
    ("cmd_gone", "Nothing:"): "matches nothing, so it must be reported stale",
}


def self_test():
    """Run `scan` over the fixture; 0 if every verdict is right."""
    failures = 0

    def check(ok, what):
        nonlocal failures
        print(f"  {'ok  ' if ok else 'FAIL'}  {what}")
        if not ok:
            failures += 1

    names = re.findall(r"^fn (\w+)", _FIXTURE, re.MULTILINE)
    hits, stale = scan(_FIXTURE, _FIXTURE_ALLOWED)
    got = {fn for _, fn, _ in hits}
    for fn in names:
        verdict = "reported" if fn in got else "passed"
        check((fn in got) == (fn in _REPORTED), f"{fn}: {verdict}")
    check(
        stale == [("cmd_gone", "Nothing:")],
        "a used ALLOWED entry exempts its site; an unused one is reported stale",
    )
    if failures:
        print(f"[query-status] self-test: {failures} check(s) FAILED", file=sys.stderr)
        return 1
    print(f"[query-status] self-test passed ({len(names) + 1} checks)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
