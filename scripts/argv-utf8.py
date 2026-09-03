#!/usr/bin/env python3
"""Find utilities that read argv or the environment as `String`, and so panic.

On this OS a path may hold every byte except `/` and NUL -- that is written
down in `design.txt`, not an accident of the implementation. A utility that
reads its command line as `String` therefore does not merely mishandle such a
name, it *dies* before reaching its own first statement:

    let args: Vec<String> = env::args().collect();

`std::env::args()`'s iterator is documented to panic on an argument that is not
valid Unicode, and its body is a literal `unwrap`. So `rm` on a file whose name
holds byte 0x80 does not remove it, does not report an error, and does not run
any code this repository wrote: it aborts with a Rust panic message. The same
goes for `cp`, `mv`, `ls`, `find`, `grep` and most of the rest -- 49 of 84
shipped utilities, measured 2026-08-22 by this script. See `known-issues.md` ->
`B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.

The fix in each case is `env::args_os()` carried through as `OsString`/`&[u8]`
to wherever the argument is used, which for a filename means all the way to the
syscall. `coreutils::getopt` is byte-based, and the correlation is the argument
for doing it that way: of the 35 bins already clean, 24 go through `getopt`; of
the 49 dirty ones, **none** do. A bin that parses options through the shared
module never had a reason to reach for `String` in the first place.

# Why a gate and not just a burn-down

Fixing 49 `main`s fixes 49 instances. It does not stop the fiftieth, and the
defect is invisible on the development host: Windows delivers argv as UTF-16,
so a test cannot easily produce an argument that triggers it, and `cargo test`
never runs the binaries at all -- only their internal functions. `main` is the
least-tested line in every one of these files. A convention will not survive
that; a ratchet will.

# The ratchet

`--check` fails on a finding that is *not* in `scripts/argv-utf8-baseline.txt`
-- and on a baseline line naming a finding that is no longer there. The
baseline records the backlog that existed when this landed and is only ever
meant to shrink; "meant to" is not a mechanism, and its sibling ratchet
`host-errmsg-baseline.txt` was found carrying 17 already-fixed lines, each of
which was a standing permission for that bin to regress with the gate still
green. Shrinking is one command, `--write-baseline`, and it cannot lose a real
finding, because a bin that still has the defect is still found.

A genuine false positive belongs in `IGNORE` below, which records *why*, rather
than in the baseline, which records only *that*.

# Scope, stated rather than assumed

The gate covers `userspace/coreutils/`: the 84 shipped, on-`PATH` utilities
where the panic destroys work a user cannot see coming -- one oddly-named file
in a directory is enough to make `rm -r` abort part-way, and a cleanup script
that dies half-done is worse than one that refuses to start.

It does **not** gate the ~2750 single-file crates under `userspace/*/` that
print canned output. They are stubs; a panic there is a stub being a stub, and
a 2750-line baseline is a baseline nobody reads. But they are *counted and
reported* rather than passed over silently, for the same reason the whole tool
exists: a checker that quietly narrows its own scope reports a clean tree, and a
clean report is the one outcome that must never be produced by accident.

That count is printed by a bare run and by `--write-baseline`, and not by
`--check` -- see the comment in `main`. It is there to be read by a person
deciding whether the scope is right, and nobody is reading in a push hook.

# Which tree it judges

    python scripts/argv-utf8.py --check              # judge the working tree
    python scripts/argv-utf8.py --check --head <rev> # judge that revision

Without `--head` this reads the working tree, which is what a run by hand
means. The push hook passes `--head <sha>` for each commit being pushed,
because the two are not the same question: a commit that reads argv as
`String` is published whether or not the author has since edited the file, and
a fix that exists only on disk does not travel. Everything the checker reads
goes through the `Tree` seam in `scripts/gittree.py` -- the file list, each
file's source, and the baseline -- so that "judge this revision" means all
three, not just the ones that were easy to convert. A baseline read off the
disk would be the worst of the three: an uncommitted line added to it silences
a finding in a commit that does not contain the silencing line.

`scripts/test-checkers-honour-head.py` is what keeps the flag honest.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "argv-utf8-baseline.txt"

# Repo-relative spellings, because that is the only kind of path a `Tree`
# knows: a git revision has no directory to resolve an absolute path against.
# This is where the disk stops being the subject and becomes one of two
# possible answers to the same question.
#
# `BASELINE` above survives alongside `BASELINE_REL` and is not a duplicate:
# `--write-baseline` writes to the disk, always, because that is the file the
# author will commit. Only the *reading* moved onto the tree.
BASELINE_REL = "scripts/argv-utf8-baseline.txt"
# The gated tree: the real, shipped utilities.
GATED_REL = "userspace/coreutils"
# Reported-but-not-gated, so the excluded scope is a number rather than a
# silence. See the module docstring.
SURVEYED_REL = "userspace"

# `strip_comments_and_strings` is imported rather than copied. It is forty lines
# of Rust lexing that already earned its keep once -- `raced-globals.py` spent a
# pass reporting a global whose name appeared only in a comment about it -- and
# a second copy is a copy that drifts. The hyphen in the module's name is why
# this needs a loader rather than `import`.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import gittree  # noqa: E402
import srcload  # noqa: E402

# Loaded from source rather than through `importlib`: a `SourceFileLoader`
# consults `__pycache__`, whose staleness check is `(mtime, size)` at
# one-second resolution, so two same-size writes to the sibling inside one
# second leave the second one invisible and this script silently runs the
# previous version of it. See `scripts/srcload.py`.
_rg = srcload.load(
    str(Path(__file__).resolve().parent / "raced-globals.py"), "_raced_globals"
)
strip_comments_and_strings = _rg.strip_comments_and_strings


# --------------------------------------------------------------------- rules --
#
# Each rule is a spelling of the *same* defect: a byte string from the OS forced
# through Rust's `String`, which cannot hold one. They are separate entries
# rather than one because a file can carry several and fixing one is not fixing
# the file -- a baseline keyed on the file alone would go green halfway.

# `(name, pattern, fix)`.
RULES: list[tuple[str, re.Pattern[str], str]] = [
    (
        "argv-as-string",
        # `env::args()` and `std::env::args()`, but not `args_os()`: after
        # `args` the next character there is `_`, which neither `\s` nor `\(`
        # matches.
        re.compile(r"\benv::args\s*\(\s*\)"),
        "reads argv as String; std's iterator unwraps. Use env::args_os().",
    ),
    (
        "env-as-string",
        # Same defect on the environment. `env::var()` is *not* here: it
        # returns `Err(NotUnicode)` rather than panicking, so it is a
        # behaviour bug at worst and not this one.
        re.compile(r"\benv::vars\s*\(\s*\)"),
        "reads the environment as String; std's iterator unwraps. "
        "Use env::vars_os().",
    ),
    (
        "into_string-unwrap",
        # The same panic written out by hand, which is what a conversion that
        # changed `args()` to `args_os()` and stopped there leaves behind.
        re.compile(r"\.into_string\s*\(\s*\)\s*\.\s*(?:unwrap|expect)\s*\("),
        "converts an OsString to String and unwraps. Carry the OsString.",
    ),
    (
        "to_str-unwrap",
        # `Path::to_str`/`OsStr::to_str` return `Option`, so this is a panic
        # the caller wrote rather than one std wrote -- same crash, same
        # trigger, same fix.
        re.compile(r"\.to_str\s*\(\s*\)\s*\.\s*(?:unwrap|expect)\s*\("),
        "converts a path to str and unwraps. Use quote::os_bytes.",
    ),
]

# A finding is keyed `<relative path>:<rule>`, split on the last colon -- so a
# rule name holding one would silently take a piece of the path with it, and
# the baseline would then be keyed on a path that does not exist. That is a
# mis-split, not a crash: it produces a plausible-looking report and a baseline
# that can never match, which is the failure mode this whole file is built to
# avoid. The first draft named these rules `env::args` and did exactly that.
assert all(":" not in name for name, _p, _f in RULES), (
    "a rule name must not contain ':' -- it is the key separator"
)

# Genuine false positives. Keyed `<relative path>:<rule>`, valued with the
# reason -- which is the difference between this and the baseline: this file
# records *why* a finding is not a defect, the baseline records only that one
# exists.
IGNORE: dict[str, str] = {}


def _relpath(p: Path) -> str:
    try:
        return p.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return p.as_posix()


def rust_files(tree: gittree.Tree, prefix: str) -> list[str]:
    """Every `.rs` file below `prefix`, skipping build output.

    Build output is skipped by the seam, which prunes `target/`, `.git` and the
    `target-*` family while walking rather than filtering the results: that
    directory holds tens of gigabytes of generated sources, and the difference
    is a gate that runs in a second versus one that gets uninstalled.

    This used to be a hand-rolled walk with its own copy of that rule. The copy
    is gone rather than kept, because two spellings of one rule is one rule that
    drifts -- and this one had already drifted the other way: the seam was
    missing `target-*` until it was added for this conversion.
    """
    return [rel for rel in tree.files_under(prefix)
            if rel.rsplit("/", 1)[-1].endswith(".rs")]


def analyse(tree: gittree.Tree, rel: str) -> list[tuple[str, int, str]]:
    """Findings in one file, read from whichever tree is being judged."""
    raw = tree.read_text(rel)
    if raw is None:
        return []
    return analyse_text(raw)


def analyse_text(raw: str) -> list[tuple[str, int, str]]:
    """Return `(rule, line number, the line)` for every finding in one file.

    Split from [`analyse`] so the self-test can hand it a literal. It used to
    write a temporary `x.rs` and read it back, which meant every rule case was
    also a test of the filesystem, and none of them could run against a tree
    that is not the disk.

    Only the *first* hit per rule is returned. The finding is "this file reads
    argv as String", which is one fact however many times it is spelled, and a
    per-occurrence report would make a file look worse for being long.

    Every file is lexed. A substring prefilter was tried here and removed: the
    obvious literal for the first rule is `env::args`, which is a substring of
    `env::args_os` -- so it admits every *fixed* file too, and measured, it let
    2857 of 2902 files through. A prefilter that cannot tell the defect from its
    own fix buys nothing and can only be wrong in the silent direction.
    """
    src = strip_comments_and_strings(raw)
    real = raw.splitlines()
    stripped = src.splitlines()

    out: list[tuple[str, int, str]] = []
    for rule, pattern, _fix in RULES:
        for i, line in enumerate(stripped):
            if pattern.search(line):
                out.append((rule, i + 1, real[i].strip() if i < len(real) else ""))
                break
    return out


def findings(tree: gittree.Tree, prefix: str) -> dict[str, tuple[int, str]]:
    """`{"<path>:<rule>": (line, text)}` for everything below `prefix`."""
    out: dict[str, tuple[int, str]] = {}
    for rel in sorted(rust_files(tree, prefix)):
        for rule, line, text in analyse(tree, rel):
            key = f"{rel}:{rule}"
            if key in IGNORE:
                continue
            out[key] = (line, text)
    return out


def load_baseline(tree: gittree.Tree) -> set[str]:
    """The baselined backlog, read from the tree being judged.

    From the tree, not the disk, for the same reason as everything else here:
    a baseline edited but not committed would otherwise silence a finding in a
    commit that does not contain the silencing line -- the ratchet answering
    for a tree nobody is pushing.

    `errors="strict"`, because a baseline that is not valid UTF-8 is not a
    baseline to be read leniently: the replacement characters would land inside
    finding keys and quietly stop matching, which reads as "the backlog is
    fixed" rather than as an error.
    """
    text = tree.read_text(BASELINE_REL, errors="strict")
    if text is None:
        return set()
    out = set()
    for line in text.splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.add(line)
    return out


def stale_entries(known: set[str], gated: set[str]) -> list[str]:
    """Baseline lines naming a finding that is no longer there.

    A function rather than an expression in `main` only so that `--selftest`
    can reach it. That is not ceremony: this guard fails toward *silence* in
    the same way the detector does -- a version of it that never fires looks
    exactly like a baseline that happens to be exact, which is what let the
    sibling ratchet accumulate 17 dead lines unnoticed.
    """
    return sorted(known - gated)


def _inputs_missing(tree: gittree.Tree, needs_baseline: bool) -> str | None:
    """Why this tree cannot be judged at all, or `None` if it can.

    Every rule in `--selftest` proves the detector classifies a *given* file
    correctly. None of them would notice if `GATED_REL` named a directory that
    had been renamed away, or if `rust_files` pruned too eagerly: the listing
    would come back empty, `--check` would find nothing new, and the gate would
    pass forever while looking at nothing. That is the precise shape of failure
    this tool exists to prevent -- a clean report produced by accident -- so it
    is worth one explicit question.

    The baseline is the second input, and until 2026-09-03 it had no such
    question. It fails the other way, loudly: unreadable through the seam it
    comes back as an *empty backlog* rather than as an error, so `--check`
    calls every baselined bin NEW and refuses the push with a paragraph each,
    all of them naming bins the author never touched -- while on a clean tree
    the same read calls every baseline line stale, which reads as "the backlog
    was fixed" by a commit that fixed nothing. Both are false accusations over
    a file that moved, which `scripts/run-checker.sh` argues is the worst thing
    a gate can do. `needs_baseline` is false only for `--write-baseline`, which
    creates the file and so must be allowed to run without it; gate 6's
    equivalent also excuses `--list`, a mode this checker does not have.

    Asked of the tree under judgement rather than of the disk, because that is
    where the risk is: a commit that moves either path disarms the gate *for
    that commit*, and a disk-side check would answer for a working tree that
    still has both. Non-emptiness rather than a count: a threshold would be a
    claim about this repository, and this checker is run against fixtures too.

    Named and shaped to match `scripts/host-errmsg.py`'s `_inputs_missing`
    deliberately -- these are one rule enforced at two gates, and two spellings
    of one rule is one rule that drifts. This function was `_no_corpus` until
    the baseline half was added; see `known-issues.md` ->
    `TD-B-GATE-4-CANNOT-TELL-AN-EMPTY-BACKLOG-FROM-A-MISSING-ONE`.
    """
    if not rust_files(tree, GATED_REL):
        return (f"no .rs files under {GATED_REL} -- the gate has nothing to "
                f"judge, which is not the same as a clean tree. Has the "
                f"directory moved?")
    if needs_baseline and tree.read_text(BASELINE_REL, errors="strict") is None:
        return (f"cannot read {BASELINE_REL} -- without the backlog every "
                f"baselined bin reads as a new finding, so this would refuse "
                f"the push over a file that moved rather than over any code.")
    return None


def selftest() -> int:
    """Check the rules that decide what this tool reports.

    A detector that fails toward silence looks exactly like a clean tree, and
    this one guards a defect that cannot be reproduced on the development host
    at all -- so `--check` passing means nothing unless this passed first. Rules
    are counted from `rule()` calls rather than from a literal, so adding a case
    cannot leave the summary claiming a total that no longer matches what ran.
    """
    def classify(src: str) -> set[str]:
        return {rule for rule, _line, _text in analyse_text(src)}

    failures: list[str] = []
    rules: list[str] = []
    current = ""

    def rule(name: str) -> None:
        nonlocal current
        current = name
        rules.append(name)

    def expect(label: str, got: object, want: object) -> None:
        if got != want:
            failures.append(f"{current}: {label}: want {want!r}, got {got!r}")

    # 1. The base case, in the exact shape the 50 bins are written in.
    rule("base")
    expect(
        "base/reported",
        classify("fn main() { let a: Vec<String> = env::args().collect(); }"),
        {"argv-as-string"},
    )
    expect(
        "base/qualified",
        classify("fn main() { let a: Vec<String> = std::env::args().skip(1).collect(); }"),
        {"argv-as-string"},
    )

    # 2. `args_os` is the fix, and must not match. This is the rule whose
    #    failure would be invisible: a regex that also matched `args_os` would
    #    report every *converted* bin, and the natural response to a tool that
    #    flags its own fix is to stop believing the tool.
    rule("args_os-is-not-args")
    expect(
        "args_os/clean",
        classify("fn main() { let a: Vec<OsString> = std::env::args_os().collect(); }"),
        set(),
    )
    expect(
        "vars_os/clean",
        classify("fn main() { for (k, v) in std::env::vars_os() {} }"),
        set(),
    )

    # 3. Prose about the defect is not the defect. `raced-globals.py` spent a
    #    whole pass reporting a global whose name appeared only in a comment
    #    about it, and a file being *fixed* is the single most likely place for
    #    `env::args()` to appear in a comment explaining what it replaced.
    rule("comments")
    expect(
        "comment/ignored",
        classify(
            """
// This used to be env::args(), which panics.
/// See `env::args()` for the version that unwraps.
/* let a: Vec<String> = env::args().collect(); */
fn main() { greet(); }
"""
        ),
        set(),
    )
    expect(
        "string-literal/ignored",
        classify(r'fn main() { eprintln!("do not use env::args() here"); }'),
        set(),
    )

    # 4. `env::var` returns a Result and `to_string_lossy` substitutes; neither
    #    is this defect, and reporting them would bury the one that crashes.
    rule("near-misses")
    expect(
        "env::var/clean",
        classify('fn main() { let _ = env::var("HOME"); }'),
        set(),
    )
    expect(
        "lossy/clean",
        classify("fn main() { let s = p.to_string_lossy(); }"),
        set(),
    )

    # 5. The hand-written spellings of the same panic, which is what a half
    #    conversion leaves behind: argv is read as `OsString` and then forced
    #    through `String` one line later.
    rule("hand-written")
    expect(
        "into_string/reported",
        classify("fn main() { let s = a.into_string().unwrap(); }"),
        {"into_string-unwrap"},
    )
    expect(
        "into_string-expect/reported",
        classify('fn main() { let s = a.into_string().expect("utf8"); }'),
        {"into_string-unwrap"},
    )
    expect(
        "to_str/reported",
        classify("fn main() { let s = path.to_str().unwrap(); }"),
        {"to_str-unwrap"},
    )
    # The safe forms of both must stay clean, or the rule flags its own fix.
    expect(
        "into_string-ok/clean",
        classify("fn main() { let s = a.into_string().unwrap_or_default(); }"),
        set(),
    )
    expect(
        "to_str-ok/clean",
        classify("fn main() { if let Some(s) = path.to_str() {} }"),
        set(),
    )

    # 6. Several rules in one file are several findings. A file keyed on its
    #    path alone would go green when the first of them was fixed.
    rule("several-rules")
    expect(
        "several/reported",
        classify(
            """
fn main() {
    let a: Vec<String> = env::args().collect();
    let s = path.to_str().unwrap();
}
"""
        ),
        {"argv-as-string", "to_str-unwrap"},
    )

    # 7. The staleness guard. It fails toward silence exactly as the detector
    #    does -- a version of it that never fires is indistinguishable from a
    #    baseline that happens to be exact -- so it needs its own cases.
    rule("baseline-staleness")
    expect(
        "stale/exact-baseline-is-clean",
        stale_entries({"a.rs:argv-as-string"}, {"a.rs:argv-as-string"}),
        [],
    )
    expect(
        "stale/fixed-finding-is-reported",
        stale_entries({"a.rs:argv-as-string", "b.rs:to_str-unwrap"},
                      {"a.rs:argv-as-string"}),
        ["b.rs:to_str-unwrap"],
    )
    # A *new* finding is the other ratchet direction and is not staleness; it
    # must not leak into this list, or the fix advice printed would be wrong.
    expect(
        "stale/new-finding-is-not-stale",
        stale_entries({"a.rs:argv-as-string"},
                      {"a.rs:argv-as-string", "c.rs:argv-as-string"}),
        [],
    )
    expect("stale/empty-baseline-is-clean", stale_entries(set(), {"a.rs:x"}), [])
    # Several dead lines come back sorted, because they are printed in order.
    expect(
        "stale/multiple-are-sorted",
        stale_entries({"z.rs:x", "a.rs:x", "m.rs:x"}, set()),
        ["a.rs:x", "m.rs:x", "z.rs:x"],
    )

    # Nothing here touches a tree, deliberately. "The corpus is really there"
    # is the other thing that has to be true before a clean report means
    # anything, and it used to be an eighth rule asking the working tree
    # whether `userspace/coreutils` held more than fifty `.rs` files. That was
    # wrong twice over. It asked the *disk* about a run that may be judging a
    # revision -- a commit that renames the gated directory disarms the gate
    # for that commit while a disk-side self-test says all is well -- and a
    # threshold of fifty is a fact about this checkout, so the checker could not
    # be self-tested anywhere else, which is exactly what its own end-to-end
    # cases do. It now lives in `main`, asked of whichever tree is being
    # judged, where both of those stop being true. See `_inputs_missing`, which
    # since 2026-09-03 asks the same of the *baseline* -- the input this comment
    # did not think to mention, and the one whose absence is loud rather than
    # silent.
    for f in failures:
        print(f"selftest FAIL {f}")
    print(f"selftest: {len(rules) - len({f.split(':')[0] for f in failures})}"
          f"/{len(rules)} rules ok")
    return 1 if failures else 0


def main() -> int:
    if "--selftest" in sys.argv[1:]:
        return selftest()
    ap = argparse.ArgumentParser(add_help=True, description=__doc__.split("\n")[0])
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--write-baseline", action="store_true", dest="write")
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--head", metavar="REV",
                    help="judge this revision instead of the working tree")
    args = ap.parse_args()
    check, write = args.check, args.write

    try:
        tree = gittree.open_tree(str(ROOT), args.head)
    except gittree.GitTreeError as exc:
        # Exit 2, not 1. `scripts/run-checker.sh` reads 1 as "the checker found
        # something" and prints the gate's refusal over it -- text that tells
        # the author their code is wrong and offers the bypass. A revision that
        # cannot be read is not a statement about anybody's code.
        print(f"argv-utf8: cannot read {args.head!r}: {exc}", file=sys.stderr)
        return 2

    with tree:
        # Before anything is reported, ask whether there is anything to report
        # *on*. Exit 2 rather than 1 for run-checker.sh's reason: this is not a
        # finding about anybody's code, and printing gate 4's refusal over it
        # would tell the author their utility panics on a legal filename when
        # what actually happened is that the gate lost its subject.
        why = _inputs_missing(tree, needs_baseline=not write)
        if why is not None:
            where = f"in {args.head}" if args.head else "in the working tree"
            print(f"argv-utf8: {where}, {why}", file=sys.stderr)
            return 2

        gated = findings(tree, GATED_REL)

        # Everything under `userspace/` that the gate does not cover. Counted
        # so the excluded scope is a visible number that can be argued with,
        # rather than an absence nobody notices.
        #
        # Not under `--check`. The survey walks ~2900 files and lexing them
        # takes ~17s against ~0.5s for the gated tree alone; that cost buys
        # four lines of context for a *human* reading the report, and in a push
        # hook there is no such human -- only a wait, before output nobody acts
        # on. A gate slow enough to be resented is a gate that gets bypassed,
        # which is this tool's own failure mode by a longer route. So the
        # number is computed where it is read: `--write-baseline` and a bare
        # run print it, `--check` does not.
        ungated: dict[str, tuple[int, str]] = {}
        if not check:
            ungated = {k: v for k, v in findings(tree, SURVEYED_REL).items()
                       if k not in gated}

        known = load_baseline(tree)

    if write:
        body = [
            "# Utilities that read argv or the environment as `String`, and so",
            "# panic on an argument holding a byte that is not valid UTF-8 --",
            "# which on this OS is a legal filename.",
            "# Generated by scripts/argv-utf8.py --write-baseline.",
            "#",
            "# This file is a ratchet and only ever shrinks. Do NOT add a line to",
            "# turn a red --check green: a new entry is a new utility that dies",
            "# rather than running. Fix it -- env::args_os() carried through as",
            "# OsString/&[u8], which for a filename means all the way to the",
            "# syscall. coreutils::getopt is byte-based and is the reason the",
            "# already-clean bins are clean.",
            "#",
            "# A genuine false positive belongs in the IGNORE table in the script,",
            "# which records *why*, not here, which records only *that*.",
            "",
        ]
        body += sorted(gated)
        BASELINE.write_text("\n".join(body) + "\n", encoding="utf-8", newline="")
        print(f"wrote {_relpath(BASELINE)} with {len(gated)} entries")
        return 0

    if ungated:
        files = len({k.rsplit(":", 1)[0] for k in ungated})
        print(
            f"--- {len(ungated)} finding(s) in {files} file(s) under "
            f"{SURVEYED_REL}, outside the gate ---"
        )
        print(
            "    Stub programs that print canned output, not shipped utilities.\n"
            "    Reported so the gate's scope is a number rather than a silence;\n"
            "    not counted below and not gated. See the module docstring."
        )
        print()

    new = sorted(k for k in gated if k not in known)
    stale = stale_entries(known, set(gated))

    # Under --check this runs in a push hook, where the baselined backlog is not
    # news. Print only what is actually new. Without --check a human is asking
    # to see the backlog, so print it.
    to_show = new if check else sorted(gated)
    for key in to_show:
        line, text = gated[key]
        path, rule = key.rsplit(":", 1)
        mark = "NEW " if key in set(new) else "    "
        print(f"{mark}{path}:{line}  [{rule}]  {text}")

    for key in stale:
        print(f"FIXED {key}  -- in the baseline but no longer found")

    print(
        f"\n{len(gated)} finding(s); {len(new)} not in the baseline; "
        f"{len(stale)} baseline line(s) now stale."
    )
    if check and (new or stale):
        fixes = {name: fix for name, _p, fix in RULES}
        if new:
            print()
            for key in new:
                print(f"  {key.rsplit(':', 1)[1]}: {fixes[key.rsplit(':', 1)[1]]}")
        if stale:
            print(
                "\n  The baseline lines above name findings that are already "
                "fixed. Shrink it:\n"
                f"      python {_relpath(Path(__file__))} --write-baseline\n"
                "  It cannot lose a real finding -- a bin that still has the "
                "defect is still found."
            )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
