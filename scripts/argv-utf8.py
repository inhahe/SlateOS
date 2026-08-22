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

`--check` fails only on a finding that is *not* in
`scripts/argv-utf8-baseline.txt`. The baseline records the backlog that existed
when this landed and is only ever meant to shrink. A genuine false positive
belongs in `IGNORE` below, which records *why*, rather than in the baseline,
which records only *that*.

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
"""

from __future__ import annotations

import importlib.util
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "argv-utf8-baseline.txt"

# The gated tree: the real, shipped utilities.
GATED = ROOT / "userspace" / "coreutils"
# Reported-but-not-gated, so the excluded scope is a number rather than a
# silence. See the module docstring.
SURVEYED = ROOT / "userspace"

# `strip_comments_and_strings` is imported rather than copied. It is forty lines
# of Rust lexing that already earned its keep once -- `raced-globals.py` spent a
# pass reporting a global whose name appeared only in a comment about it -- and
# a second copy is a copy that drifts. The hyphen in the module's name is why
# this needs importlib rather than `import`.
_spec = importlib.util.spec_from_file_location(
    "_raced_globals", str(Path(__file__).resolve().parent / "raced-globals.py")
)
_rg = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_rg)
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


def rust_files(under: Path) -> list[Path]:
    """Every `.rs` file below `under`, skipping build output.

    `target/` is skipped by pruning rather than by filtering the results,
    because it holds tens of gigabytes of generated sources and walking it is
    the difference between a gate that runs in a second and one that gets
    uninstalled.
    """
    out: list[Path] = []
    stack = [under]
    while stack:
        d = stack.pop()
        try:
            entries = list(d.iterdir())
        except OSError:
            continue
        for e in entries:
            if e.is_dir():
                if e.name in {"target", ".git"} or e.name.startswith("target-"):
                    continue
                stack.append(e)
            elif e.suffix == ".rs":
                out.append(e)
    return out


def analyse(path: Path) -> list[tuple[str, int, str]]:
    """Return `(rule, line number, the line)` for every finding in one file.

    Only the *first* hit per rule is returned. The finding is "this file reads
    argv as String", which is one fact however many times it is spelled, and a
    per-occurrence report would make a file look worse for being long.

    Every file is lexed. A substring prefilter was tried here and removed: the
    obvious literal for the first rule is `env::args`, which is a substring of
    `env::args_os` -- so it admits every *fixed* file too, and measured, it let
    2857 of 2902 files through. A prefilter that cannot tell the defect from its
    own fix buys nothing and can only be wrong in the silent direction.
    """
    try:
        raw = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []
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


def findings(under: Path) -> dict[str, tuple[int, str]]:
    """`{"<path>:<rule>": (line, text)}` for everything below `under`."""
    out: dict[str, tuple[int, str]] = {}
    for path in sorted(rust_files(under)):
        rel = _relpath(path)
        for rule, line, text in analyse(path):
            key = f"{rel}:{rule}"
            if key in IGNORE:
                continue
            out[key] = (line, text)
    return out


def load_baseline() -> set[str]:
    if not BASELINE.is_file():
        return set()
    out = set()
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.add(line)
    return out


def selftest() -> int:
    """Check the rules that decide what this tool reports.

    A detector that fails toward silence looks exactly like a clean tree, and
    this one guards a defect that cannot be reproduced on the development host
    at all -- so `--check` passing means nothing unless this passed first. Rules
    are counted from `rule()` calls rather than from a literal, so adding a case
    cannot leave the summary claiming a total that no longer matches what ran.
    """
    import tempfile

    def classify(src: str) -> set[str]:
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "x.rs"
            p.write_text(src, encoding="utf-8")
            return {rule for rule, _line, _text in analyse(p)}

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

    # 7. The gated tree must be non-empty, and it is the one rule that cannot
    #    be written against synthetic input.
    #
    #    Every case above proves the rules classify a *given* file correctly.
    #    None of them would notice if `GATED` pointed at a directory that had
    #    been renamed away, or if `rust_files` pruned too eagerly: the walk
    #    would return nothing, `--check` would find nothing new, and the gate
    #    would pass forever while looking at an empty set. That is the exact
    #    shape of the failure this tool exists to prevent, and the cheapest
    #    guard against it is to insist the corpus is really there.
    rule("gated-tree-is-not-empty")
    expect("gated/dir-exists", GATED.is_dir(), True)
    expect("gated/has-files", len(rust_files(GATED)) > 50, True)

    for f in failures:
        print(f"selftest FAIL {f}")
    print(f"selftest: {len(rules) - len({f.split(':')[0] for f in failures})}"
          f"/{len(rules)} rules ok")
    return 1 if failures else 0


def main() -> int:
    args = sys.argv[1:]
    if "--selftest" in args:
        return selftest()
    check = "--check" in args
    write = "--write-baseline" in args

    gated = findings(GATED)

    # Everything under `userspace/` that the gate does not cover. Counted so
    # the excluded scope is a visible number that can be argued with, rather
    # than an absence nobody notices.
    #
    # Not under `--check`. The survey walks ~2900 files and lexing them takes
    # ~17s against ~0.5s for the gated tree alone; that cost buys four lines of
    # context for a *human* reading the report, and in a push hook there is no
    # such human -- only a wait, before output nobody acts on. A gate slow
    # enough to be resented is a gate that gets bypassed, which is this tool's
    # own failure mode by a longer route. So the number is computed where it is
    # read: `--write-baseline` and a bare run print it, `--check` does not.
    ungated: dict[str, tuple[int, str]] = {}
    if not check:
        ungated = {k: v for k, v in findings(SURVEYED).items() if k not in gated}

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
            f"{_relpath(SURVEYED)}, outside the gate ---"
        )
        print(
            "    Stub programs that print canned output, not shipped utilities.\n"
            "    Reported so the gate's scope is a number rather than a silence;\n"
            "    not counted below and not gated. See the module docstring."
        )
        print()

    known = load_baseline()
    new = sorted(k for k in gated if k not in known)

    # Under --check this runs in a push hook, where the baselined backlog is not
    # news. Print only what is actually new. Without --check a human is asking
    # to see the backlog, so print it.
    to_show = new if check else sorted(gated)
    for key in to_show:
        line, text = gated[key]
        path, rule = key.rsplit(":", 1)
        mark = "NEW " if key in set(new) else "    "
        print(f"{mark}{path}:{line}  [{rule}]  {text}")

    print(f"\n{len(gated)} finding(s); {len(new)} not in the baseline.")
    if new and check:
        fixes = {name: fix for name, _p, fix in RULES}
        print()
        for key in new:
            print(f"  {key.rsplit(':', 1)[1]}: {fixes[key.rsplit(':', 1)[1]]}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
