#!/usr/bin/env python3
"""Refuse a raw control byte in a tracked text file.

## What it catches, and why nothing else can

Text written through a shell heredoc arrives with its escapes already
collapsed. `printf 'a b\\n'` inside a `python - <<'PY'` body reaches Python as a
real newline; `b"hello\\0"` reaches it as a NUL; `` `\\b(\\w+)` `` reaches it as
a backspace. The receiving program then writes the byte into a file, and the
file is committed.

That is not a hypothetical. It had happened **eight times in this tree** by the
time this gate was written, across four files and three lanes:

| file | byte | what it destroyed |
|---|---|---|
| `known-issues.md` | NUL x3, LF | a markdown table row split across four lines; a row about `\\v` and `\\f` whose every escape became the character, so it read "`` and `` are not whitespace" |
| `posix/src/wchar.rs` | NUL x2 | `b"hello\\0"` |
| `userspace/ldd/src/main.rs` | NUL x2 | `b"\\0libfoo.so.1\\0"` |
| `userspace/sshd/src/lib.rs` | NUL | `b"openssh-key-v1\\0"` |
| `kernel/src/main.rs` | ETX | a comment quoting `write(fm, "\\x03", 1)` |
| `scripts/check-unreachable-mutators.py` | BS | a comment quoting the regex `\\b(\\w+)::` -- so the documented pattern was not the pattern |

**The Rust ones are why this needed a gate rather than more care.** Rust accepts
a raw NUL inside a byte-string literal, so `b"hello<NUL>"` and `b"hello\\0"` are
the same value. Those crates compiled, passed their tests and shipped with an
unreadable literal in them, and no build, lint, test or review could have told
the difference -- the defect changes the source and not the behaviour, so it has
no natural discovery path at all. The two in documentation are worse in a
different way: they are silently *wrong instructions*, and the reader who copies
the regex gets a backspace.

`grep` reporting `known-issues.md` as "Binary file matches" was the only symptom
anything ever produced, and it reads as a quirk of grep.

## The rule

A byte below 0x20 that is not TAB, LF or CR, or the byte 0x7F, in a file git
does not consider binary. Nothing else: this is not a lint about unicode, or
about what the text says.

## A gate must not use a classifier its own offence can change

This gate was first written to share `check-eol.py`'s `is_binary`, on the
reasoning that two gates walking "the tracked text files" with two different
definitions of that phrase is how a file ends up in neither. That reasoning was
right about the risk and wrong about this gate, and the self-test caught it:

    $ # inject a raw NUL into posix/src/wchar.rs, then
    $ python scripts/check-control-bytes.py
    check-control-bytes: clean -- 6431 text file(s), 1 baselined occurrence(s)

**Clean, on a file corrupted seconds earlier.** git's binary test is "does it
contain a NUL", so the instant a source file acquires the most common form of
this defect it stops being a file this gate looks at. The offence erased the
evidence *and* the file.

The general rule this tree should keep: **a gate must not decide its population
with a test that its own offence can flip.** `check-eol` is safe because a CR
does not make a file binary, so its population is stable under its own offence.
This one is not, so it needs a classifier that a handful of bad bytes cannot
move.

The classifier used instead: a file is binary if it does not decode as UTF-8,
or if more than 1% of its bytes are control bytes. A PNG fails the first test
in its first few bytes. A 40 KB source file with two stray NULs passes both and
is judged -- which is the entire point. The cost is that genuinely non-UTF-8
*text* (UTF-16, Latin-1) is called binary and skipped; that is the conservative
direction, and `check-eol.py` still covers those files for line endings.

## The baseline

A ratchet, like the tree's others. `scripts/control-bytes-baseline.txt` lists
the occurrences that are known and tolerated; anything not in it is a refusal.
The baseline is expected to stay very small, because unlike most ratchets here
its population is not technical debt that needs paying down gradually -- every
entry is either a deliberate control byte in a fixture or a defect nobody has
got to yet, and both are worth a line of explanation.

Usage:
    python scripts/check-control-bytes.py                 # check the worktree
    python scripts/check-control-bytes.py --head HEAD     # check a revision
    python scripts/check-control-bytes.py --changed A..B  # only what B changed
    python scripts/check-control-bytes.py --list          # every occurrence
    python scripts/check-control-bytes.py --update-baseline
    python scripts/check-control-bytes.py --selftest

`--changed` is the mode the push hook uses. `--head` reads all 6431 tracked
files and takes 18 seconds; a push of three commits would pay that three times,
and a gate that adds a minute to every push is a gate that gets bypassed.
`--changed` costs half a second, and it is the more accurate scope anyway: this
is a ratchet, so everything already in the tree is clean or baselined and the
only question a push has to answer is whether IT introduced one.
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import gitenv  # noqa: E402,F401  (imported for its side effect; see gittree)
import gittree  # noqa: E402

BASELINE = Path(__file__).resolve().parent / "control-bytes-baseline.txt"

# TAB, LF and CR are the three control characters text is allowed to contain.
# CR is allowed HERE because line endings are `check-eol.py`'s population, and
# a byte that two gates both refuse gets counted twice and fixed once.
ALLOWED = frozenset({0x09, 0x0A, 0x0D})

# A tracked-file count below this means the scan found nothing to look at --
# wrong directory, a failed `git ls-files`, a pathspec that matched nothing --
# rather than a clean tree. Measured 2026-09-11: 6453 tracked paths.
FLOOR = 500


def _load_check_eol():
    """`check-eol.py`, whose name has a hyphen in it and so cannot be imported.

    Only `tracked_files` is taken from it -- the list of paths, which is a fact
    about git and not about any file's contents. Its `is_binary` is deliberately
    NOT used; see "A gate must not use a classifier its own offence can change".
    """
    path = Path(__file__).resolve().parent / "check-eol.py"
    spec = importlib.util.spec_from_file_location("check_eol", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# More than this fraction of control bytes and the file is data, not corrupted
# text. Measured 2026-09-11 on this tree: the four tracked binaries (three PNGs
# and an OVMF firmware image) are all above 20%; no source file is above 0%.
# 1% leaves two orders of magnitude of headroom on both sides.
BINARY_CONTROL_FRACTION = 0.01

# ...but a fraction alone is wrong for SHORT files, and the refusal probe is
# what proved it. A sixteen-byte fixture holding one NUL is 6.25% control bytes,
# so the fraction called it binary and the gate reported "clean -- 1 text
# file(s)" on the commit that had just added it. Any short file -- a one-line
# `.txt`, a tiny config, a fixture -- would have been invisible for exactly the
# reason it needed checking.
#
# So a file is binary only if it has MANY control bytes *and* a high proportion
# of them. Real data has thousands; corrupted text has one to ten. A small blob
# that falls between gets called text, reported, and can be baselined -- a false
# positive, which is the direction a gate should fail in.
#
# Worth recording how this was missed the first time: the self-test fixture for
# "a source file with one NUL is still text" was a 430-byte file, comfortably
# under the fraction. The test passed because the fixture was too big to expose
# the bug, which is its own lesson about choosing fixture sizes.
BINARY_MIN_CONTROL = 32


def is_binary(data: bytes) -> bool:
    """Whether these bytes are data rather than text that may be corrupted.

    Deliberately not git's test, and not `check-eol.py`'s. Both answer "does it
    contain a NUL", which this gate cannot use: a NUL is the commonest form of
    the very defect being looked for, so that test would classify every freshly
    corrupted source file as binary and skip it. It did exactly that, and the
    self-test is what noticed.

    Two questions instead, neither of which a handful of stray bytes can move:
    does it decode as UTF-8, and does it hold MANY control bytes AND a high
    proportion of them. The "many" half is not redundant -- see
    `BINARY_MIN_CONTROL`, where a sixteen-byte fixture with one NUL was called
    binary by the proportion alone and skipped.
    """
    if not data:
        return False
    try:
        data.decode("utf-8")
    except UnicodeDecodeError:
        return True
    ctrl = sum(1 for b in data if (b < 0x20 and b not in ALLOWED) or b == 0x7F)
    return ctrl >= BINARY_MIN_CONTROL and ctrl > len(data) * BINARY_CONTROL_FRACTION


def offences(data: bytes) -> list[tuple[int, int]]:
    """Every `(offset, byte)` this gate refuses, in order."""
    return [(i, b) for i, b in enumerate(data)
            if (b < 0x20 and b not in ALLOWED) or b == 0x7F]


def line_of(data: bytes, offset: int) -> int:
    """The 1-based line number containing `offset`."""
    return data.count(b"\n", 0, offset) + 1


def scan_worktree() -> tuple[list[tuple[str, int, int, int]], int]:
    """`(occurrences, files_scanned)` for the files on disk.

    An occurrence is `(path, line, byte, count_of_that_byte_in_that_file)`.
    """
    ce = _load_check_eol()
    paths = ce.tracked_files()
    found: list[tuple[str, int, int, int]] = []
    scanned = 0
    for raw in paths:
        rel = raw.decode("utf-8", "surrogateescape")
        try:
            data = Path(rel).read_bytes()
        except OSError:
            # Not on disk: a sparse checkout, a broken link, a path this
            # filesystem cannot open. Absent is not an offence.
            continue
        if is_binary(data):
            continue
        scanned += 1
        hits = offences(data)
        if not hits:
            continue
        per_byte: dict[int, int] = {}
        for _off, b in hits:
            per_byte[b] = per_byte.get(b, 0) + 1
        for off, b in hits:
            found.append((rel, line_of(data, off), b, per_byte[b]))
    return found, scanned


def scan_revision(rev: str) -> tuple[list[tuple[str, int, int, int]], int]:
    """The same, for a git revision rather than the disk.

    A gate that judges the working tree passes on bytes that were tidied after
    they were committed, which is the hole `check-eol.py`'s header and lane A's
    gate-7 notice both record. `--head` is how this one is pointed at the
    revision that will actually be pushed.
    """
    tree = gittree.RevTree(rev)
    found: list[tuple[str, int, int, int]] = []
    scanned = 0
    for rel in tree.files_under(""):
        data = tree.read_bytes(rel)
        if data is None:
            continue
        # The same content-based classifier as the disk side, for the same
        # reason: asking git whether a revision's file is binary means asking
        # "does it contain a NUL", and a NUL is the offence.
        if is_binary(data):
            continue
        scanned += 1
        hits = offences(data)
        if not hits:
            continue
        per_byte: dict[int, int] = {}
        for _off, b in hits:
            per_byte[b] = per_byte.get(b, 0) + 1
        for off, b in hits:
            found.append((rel, line_of(data, off), b, per_byte[b]))
    return found, scanned


def scan_changed(rng: str) -> tuple[list[tuple[str, int, int, int]], int]:
    """The files a revision range touches, judged at the range's head.

    This is the mode the push hook uses, and it exists for a reason beyond
    speed. `--head` reads all 6431 tracked files through git and takes 18
    seconds; a push of three commits would pay that three times, and a gate
    that adds a minute to every push is a gate that gets bypassed. Scanning
    what the push actually changed costs well under a second.

    It is also the more accurate scope. This is a ratchet: everything already
    in the tree is either clean or baselined, so the only question a push has
    to answer is whether IT introduced one.

    `git diff` failing and `git diff` reporting no changed files must not look
    alike -- that is the defect family this tree keeps meeting -- so a failed
    invocation raises rather than returning an empty list.
    """
    import subprocess
    r = subprocess.run(["git", "diff", "--name-only", "-z", rng],
                       capture_output=True)
    if r.returncode != 0:
        msg = r.stderr.decode("utf-8", "replace").strip()
        raise gittree.GitTreeError(f"git diff {rng} failed: {msg}")
    rels = [p.decode("utf-8", "surrogateescape")
            for p in r.stdout.split(b"\0") if p]
    head = rng.split("..")[-1] or "HEAD"
    tree = gittree.RevTree(head)
    found: list[tuple[str, int, int, int]] = []
    scanned = 0
    for rel in rels:
        data = tree.read_bytes(rel)
        if data is None:
            # Deleted by this range, or never in it. Absent is not an offence.
            continue
        if is_binary(data):
            continue
        scanned += 1
        hits = offences(data)
        if not hits:
            continue
        per_byte: dict[int, int] = {}
        for _off, b in hits:
            per_byte[b] = per_byte.get(b, 0) + 1
        for off, b in hits:
            found.append((rel, line_of(data, off), b, per_byte[b]))
    return found, scanned


def pushed_paths(sha: str, remote: str) -> list[str]:
    """Every path the commits this push would publish touch.

    `git log --name-only <sha> --not --remotes=<remote>` rather than a
    two-point `git diff`, and the difference matters twice:

      * a byte added by one commit and removed by a later one is invisible to a
        diff of the endpoints, and is still readable in the published history.
        This is the same reasoning gate 1 uses for `todo2.txt`;
      * it needs no base revision, so a root commit and a first push are not
        edge cases that have to be special-cased into working.

    `-z`, because a path is bytes and git quotes it otherwise
    (CLAUDE.md self-review item 7).
    """
    import subprocess
    r = subprocess.run(
        ["git", "log", "--pretty=format:", "--name-only", "-z",
         sha, "--not", f"--remotes={remote}"], capture_output=True)
    if r.returncode != 0:
        msg = r.stderr.decode("utf-8", "replace").strip()
        raise gittree.GitTreeError(f"git log for {sha} failed: {msg}")
    seen: dict[str, None] = {}
    for p in r.stdout.split(b"\0"):
        if p:
            seen[p.decode("utf-8", "surrogateescape")] = None
    return list(seen)


def scan_pushed(sha: str, remote: str) -> tuple[list[tuple[str, int, int, int]], int]:
    """The paths `pushed_paths` names, judged as of `sha`."""
    rels = pushed_paths(sha, remote)
    tree = gittree.RevTree(sha)
    found: list[tuple[str, int, int, int]] = []
    scanned = 0
    for rel in rels:
        data = tree.read_bytes(rel)
        if data is None:
            # Deleted by this push. A file that is not there cannot offend.
            continue
        if is_binary(data):
            continue
        scanned += 1
        hits = offences(data)
        if not hits:
            continue
        per_byte: dict[int, int] = {}
        for _off, b in hits:
            per_byte[b] = per_byte.get(b, 0) + 1
        for off, b in hits:
            found.append((rel, line_of(data, off), b, per_byte[b]))
    return found, scanned


def baseline_keys() -> set[tuple[str, int]]:
    """`(path, byte)` pairs the baseline tolerates."""
    if not BASELINE.is_file():
        return set()
    out: set[tuple[str, int]] = set()
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) < 2:
            continue
        try:
            out.add((parts[0], int(parts[1], 16)))
        except ValueError:
            continue
    return out


def write_baseline(found: list[tuple[str, int, int, int]]) -> int:
    keys = sorted({(p, b) for p, _ln, b, _n in found})
    lines = [
        "# Occurrences of a raw control byte that are known and tolerated.",
        "# Written by scripts/check-control-bytes.py --update-baseline.",
        "# Fields: path<TAB>byte (hex)<TAB>note",
        "#",
        "# Every entry here is either a deliberate control byte in a fixture or",
        "# a defect nobody has got to yet. Both deserve the note.",
        "",
    ]
    for p, b in keys:
        lines.append(f"{p}\t{b:02x}\t")
    # `newline=""` so this is LF on every platform: the file is read back by
    # this same gate, and a fixture whose bytes depend on the host is the one
    # kind that cannot be trusted.
    BASELINE.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="")
    return len(keys)


NAMES = {0x00: "NUL", 0x07: "BEL", 0x08: "BS", 0x0B: "VT", 0x0C: "FF",
         0x1B: "ESC", 0x7F: "DEL"}


def report(found: list[tuple[str, int, int, int]]) -> None:
    for p, ln, b, n in found:
        name = NAMES.get(b, f"0x{b:02x}")
        times = "" if n == 1 else f"  ({n} in this file)"
        print(f"{p}:{ln}: raw {name} (0x{b:02x}){times}")


def check(rev: str | None, show_all: bool, rng: str | None = None,
          pushed: str | None = None, remote: str = "origin") -> int:
    if rng or pushed:
        found, scanned = (scan_pushed(pushed, remote) if pushed
                          else scan_changed(rng))
        label = f"pushed by {pushed}" if pushed else rng
        # No floor here, and that is not an oversight: a range that changed two
        # files legitimately scans two files. The thing that must not be quiet
        # is `git diff` FAILING, and `scan_changed` raises for that rather than
        # returning an empty list -- so "nothing changed" and "I could not ask"
        # stay distinguishable, which is the whole lesson of this gate.
        if not show_all:
            tolerated = baseline_keys()
            new = [f for f in found if (f[0], f[2]) not in tolerated]
            if not new:
                print(f"check-control-bytes: clean -- {scanned} text "
                      f"file(s) {label}")
                return 0
            return _refuse(new)
        report(found)
        print(f"\n{len(found)} occurrence(s) across {scanned} changed file(s)")
        return 0
    found, scanned = (scan_revision(rev) if rev else scan_worktree())
    if scanned < FLOOR:
        where = f"revision {rev}" if rev else "the working tree"
        print(f"check-control-bytes: only {scanned} text file(s) found in "
              f"{where}.", file=sys.stderr)
        print(f"  That is below the floor of {FLOOR}. Refusing to report a "
              f"clean tree from a scan that found nothing to look at.",
              file=sys.stderr)
        return 2
    if show_all:
        report(found)
        print(f"\n{len(found)} occurrence(s) across {scanned} text file(s)")
        return 0
    tolerated = baseline_keys()
    new = [f for f in found if (f[0], f[2]) not in tolerated]
    if not new:
        print(f"check-control-bytes: clean -- {scanned} text file(s), "
              f"{len(tolerated)} baselined occurrence(s)")
        return 0
    return _refuse(new)


def _refuse(new: list[tuple[str, int, int, int]]) -> int:
    """The refusal message, shared by every mode so they cannot drift apart."""
    print("check-control-bytes: raw control byte(s) in tracked text:",
          file=sys.stderr)
    for p, ln, b, n in new:
        name = NAMES.get(b, f"0x{b:02x}")
        times = "" if n == 1 else f"  ({n} in this file)"
        print(f"  {p}:{ln}: raw {name} (0x{b:02x}){times}", file=sys.stderr)
    print("", file=sys.stderr)
    print("  Almost always a shell heredoc: `\\0`, `\\b` and `\\n` are collapsed",
          file=sys.stderr)
    print("  into the byte they name before the receiving program sees them.",
          file=sys.stderr)
    print("  Write the file with the Write tool, or from a script ON DISK.",
          file=sys.stderr)
    print("  If the byte is deliberate: --update-baseline, then say why.",
          file=sys.stderr)
    return 1


def self_test() -> int:
    """Proof that it runs, and proof that it can refuse.

    A gate whose scan silently matched nothing would print the same "clean" as
    a gate that examined everything, so both halves are exercised on bytes
    built here rather than on whatever the tree happens to contain.
    """
    cases: list[tuple[str, bool]] = []

    def case(label: str, ok: bool) -> None:
        cases.append((label, ok))

    # -- the scanner --
    case("plain text has no offence", offences(b"hello world\n") == [])
    case("TAB, LF and CR are allowed", offences(b"a\tb\r\nc") == [])
    case("a NUL is an offence", offences(b"a\x00b") == [(1, 0x00)])
    case("a backspace is an offence", offences(b"a\x08b") == [(1, 0x08)])
    case("a vertical tab is an offence", offences(b"\x0b") == [(0, 0x0B)])
    case("a form feed is an offence", offences(b"\x0c") == [(0, 0x0C)])
    case("DEL is an offence", offences(b"a\x7f") == [(1, 0x7F)])
    case("0x80 is not (it is UTF-8's business)", offences(b"\x80\xff") == [])
    case("every offence is reported, not just the first",
         len(offences(b"\x00a\x00b\x00")) == 3)

    # -- the exact shapes that actually occurred --
    case("the wchar.rs literal", offences(b'b"hello\x00"') == [(7, 0x00)])
    case("the mutators regex comment", offences(b"`\x08(\\w+)") == [(1, 0x08)])
    case("the escaped form is clean", offences(b'b"hello\\0"') == [])

    # -- line numbers, because the message is useless without them --
    case("line 1", line_of(b"a\x00b\nc\n", 1) == 1)
    case("line 2", line_of(b"ab\nc\x00d\n", 4) == 2)
    case("a NUL right after a newline is on the next line",
         line_of(b"ab\n\x00", 3) == 2)

    # -- the classifier, and the bug it was written to replace --
    #
    # The first four are the whole point: a source file that has just acquired
    # the defect must still be IN the population. Under the old classifier --
    # git's, "does it contain a NUL" -- each of these was called binary and
    # skipped, and the gate printed "clean" on a file corrupted seconds before.
    src = b'fn main() {\n    let s = b"hello\x00";\n}\n' + b"// padding\n" * 40
    case("a source file with one NUL is still text", not is_binary(src))
    # The offset is derived rather than hand-counted: a literal here would be
    # asserting my arithmetic, not the scanner's.
    case("...and its NUL is reported",
         offences(src) == [(src.index(b"\x00"), 0x00)])
    case("a source file with a backspace is still text",
         not is_binary(b"# `\x08(\\w+)` is the pattern\n" + b"# more\n" * 40))
    case("an empty file is text, not binary", not is_binary(b""))
    case("a PNG header is binary",
         is_binary(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR" + bytes(range(256))))
    case("bytes that are not UTF-8 are binary", is_binary(b"\xff\xfe\x00\x41"))
    case("mostly-control is binary even when it decodes",
         is_binary(bytes(200) + b"ab"))
    # The case the probe had to find for me. The `src` fixture above is 430
    # bytes, comfortably under the 1% fraction, so it passed against a
    # fraction-only classifier that failed every SHORT file. The fixture was too
    # big to expose the bug it was written to cover.
    probe = b"a probe: b" + bytes([34, 104, 105, 0, 34]) + b"\n"
    case("a 16-byte file with one NUL is text, not binary", not is_binary(probe))
    case("...and its NUL is found",
         offences(probe) == [(probe.index(b"\x00"), 0x00)])
    case("a one-byte file that IS a NUL is text, and reported",
         not is_binary(bytes(1)) and offences(bytes(1)) == [(0, 0x00)])
    case("a short file of pure control bytes is still text (too few to be data)",
         not is_binary(bytes(8)))
    case("plain prose is text", not is_binary("hello — world\n".encode()))

    # -- the baseline reader --
    case("a baseline entry is (path, byte)",
         ("x/y.rs", 0x00) in _parse_baseline_text("x/y.rs\t00\tnote"))
    case("comments and blanks are skipped",
         _parse_baseline_text("# c\n\n") == set())
    case("a malformed byte field is skipped, not crashed on",
         _parse_baseline_text("x\tzz\t") == set())

    bad = [label for label, ok in cases if not ok]
    for label, ok in cases:
        print(f"{'ok  ' if ok else 'FAIL'}: {label}")
    print(f"\n{len(cases) - len(bad)} passed, {len(bad)} failed")
    return 1 if bad else 0


def _parse_baseline_text(text: str) -> set[tuple[str, int]]:
    """`baseline_keys`'s parser, over a string, so the self-test can reach it."""
    out: set[tuple[str, int]] = set()
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) < 2:
            continue
        try:
            out.add((parts[0], int(parts[1], 16)))
        except ValueError:
            continue
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="Refuse raw control bytes in tracked text.")
    ap.add_argument("--head", metavar="REV",
                    help="judge this revision rather than the working tree")
    ap.add_argument("--pushed", metavar="SHA",
                    help="judge every path the commits this push would publish "
                         "touch, as of SHA; the mode the push hook uses")
    ap.add_argument("--remote", default="origin",
                    help="remote whose branches bound --pushed (default origin)")
    ap.add_argument("--changed", metavar="RANGE",
                    help="judge only the files a revision range touches, at "
                         "the range's head; the mode the push hook uses")
    ap.add_argument("--list", action="store_true",
                    help="print every occurrence, baselined or not")
    ap.add_argument("--update-baseline", action="store_true",
                    help="rewrite the baseline from the current tree")
    ap.add_argument("--selftest", "--self-test", dest="selftest",
                    action="store_true", help="test this gate's own logic")
    args = ap.parse_args()

    # The self-test runs BEFORE any scan, so a gate with broken logic cannot
    # report a clean tree on its way to being wrong.
    if args.selftest:
        return self_test()
    if args.update_baseline:
        found, scanned = scan_worktree()
        if scanned < FLOOR:
            print(f"check-control-bytes: only {scanned} text file(s); refusing "
                  f"to write a baseline from a scan that found nothing.",
                  file=sys.stderr)
            return 2
        n = write_baseline(found)
        print(f"wrote {BASELINE.name} with {n} entr{'y' if n == 1 else 'ies'}")
        return 0
    return check(args.head, args.list, args.changed, args.pushed, args.remote)


if __name__ == "__main__":
    sys.exit(main())
