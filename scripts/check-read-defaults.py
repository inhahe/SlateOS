#!/usr/bin/env python3
"""Find `read_to_string(...).unwrap_or_default()` -- a read whose failure is
indistinguishable from an empty file.

## What it looks for, and why only this shape

    let text = fs::read_to_string(path).unwrap_or_default();

collapses three situations into one empty string:

    the file is there        its contents
    the file is absent       nothing configured yet
    the file cannot be read  WE DO NOT KNOW

The third is the defect. It is lane A's rule from `mkfs`/`fsck`'s
`is_mounted` -- for a value that guards a decision, "I do not know" and "there
is nothing there" must not be the same -- and on 2026-09-10 it was found four
times in this lane, written independently each time:

  * `userspace/sudo`'s visudo opened an EMPTY EDITOR over `/etc/sudoers`. The
    user adds a rule to what looks like a blank file, saves, and every existing
    rule is replaced by the one line they typed.
  * `userspace/xdg` rewrote `~/.config/mimeapps.list` holding only the
    association just set.
  * `userspace/hostnamectl` rewrote `/etc/machine-info` holding only the field
    just set.
  * `userspace/ntpd` fell back to `pool.ntp.org`, `time.google.com` and
    `time.cloudflare.com`, so an administrator who had restricted time sync to
    internal servers silently took the clock from outside their network.

`optionalfile::read_or_empty` is the answer and says which failure means what.

**`unwrap_or_else(|_| something)` is NOT flagged.** A caller writing an
explicit default has thought about the failure; `nologin`'s built-in message
and `perf`'s `[pid:N]` placeholder are both deliberate and correct. It is
`unwrap_or_default()` specifically that reads as "I did not consider this".

**A `fs::read` (bytes) is not flagged either.** The whole-file UTF-8 failure --
where ONE byte anywhere in the file empties all of it -- is what makes the text
version reachable without unusual permissions, and `userspace/pwdb` reads bytes
deliberately for exactly that reason.

## Usage

    python scripts/check-read-defaults.py            # report
    python scripts/check-read-defaults.py --check    # 1 if a new one appeared
    python scripts/check-read-defaults.py --update-baseline
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gittree  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
# Repo-relative and `/`-separated: the only spelling `gittree.Tree` accepts.
BASELINE_REL = "scripts/read-defaults-baseline.txt"
BASELINE = Path(__file__).resolve().parent / "read-defaults-baseline.txt"

# A complete char literal: `'x'`, `'\n'`, `'\''`, `'\u{1F600}'`. Deliberately
# NOT a lifetime -- `'a` has no closing quote and must pass through untouched.
_CHAR_LITERAL = re.compile(r"'(?:\\u\{[0-9a-fA-F]{1,6}\}|\\.|[^\\'\n])'")

# A raw-string opener: `r"`, `r#"`, `br##"`. Backslash is NOT an escape inside
# one, so `r"a\"` ends at that quote -- treating it as an escape swallows the
# terminator and pairs every later quote one off. `b"` is deliberately absent:
# a byte string is escaped like an ordinary one.
_RAW_OPEN = re.compile(r'b?r(#*)"')

PATTERN = re.compile(
    r"(?:fs::)?read_to_string\s*\((?:[^()]|\([^()]*\))*\)\s*(?:\n\s*)?\.unwrap_or_default\s*\(\s*\)",
    re.S,
)


def strip_noise(src: str) -> str:
    """Blank comments and string literals, preserving length and newlines.

    Without this the scan matches its own documentation: the four fixes above
    each carry a doc comment quoting the line they replaced, and an earlier
    version of this query counted all of them as live code.

    # The char-literal case, which the first version got wrong

    `rest.find('"')` is a char literal holding a double quote, and it appears
    in more than thirty crates here. Without the `'` branch below, that `"`
    opened a string that ran to the next `"` anywhere later in the file --
    after which every quote was paired one off, so real code was blanked as
    string and string contents were left as code.

    It failed silently and in the direction that looks fine: the scan still
    produced a plausible list. It was caught because a survey of `dbus` matched
    the word "simulated" INSIDE a println! whose text should have been blanked,
    and the blanked line showed the string contents surviving while the
    delimiters had gone.

    Lifetimes are not char literals -- `'a` and `'static` must pass through --
    so the branch matches only a complete `'x'`, `'\\n'` or `'\\u{1F600}'`.
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                out[i] = " "
                i += 1
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            while i < n and not src.startswith("*/", i):
                if src[i] != "\n":
                    out[i] = " "
                i += 1
            for k in range(i, min(i + 2, n)):
                out[k] = " "
            i += 2
        elif (c in "rb") and (_m := _RAW_OPEN.match(src, i)):
            close = '"' + "#" * len(_m.group(1))
            end = src.find(close, _m.end())
            end = n if end < 0 else end + len(close)
            for k in range(i, end):
                if src[k] != "\n":
                    out[k] = " "
            i = end
        elif c == "'":
            # A CHAR LITERAL, not a lifetime. `'a` / `'static` fall through to
            # the catch-all below and are left alone; only a complete literal
            # is blanked, so `find('\"')` cannot open a string.
            m = _CHAR_LITERAL.match(src, i)
            if m:
                for k in range(i, m.end()):
                    out[k] = " "
                i = m.end()
            else:
                i += 1
        elif c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            for k in range(i, min(j, n)):
                if src[k] != "\n":
                    out[k] = " "
            i = j
        else:
            i += 1
    return "".join(out)


def survey(tree: gittree.Tree) -> list[str]:
    """`<crate>: <call>` for every live occurrence.

    Keyed on the call text rather than a line number, because a line number
    churns on every edit above it and would make the baseline unreadable.

    # Two things this gets right that the first draft did not

    **The snippet comes from the ORIGINAL source, not the stripped copy.**
    `strip_noise` blanks string literals, so matching text is not the text to
    show: `read_to_string(format!("{sys_path}/removable"))` came out as
    `read_to_string(format!( ))`, and the three `eject` calls -- for
    `removable`, `device/model` and `device/vendor` -- became the same
    unreadable line. That is why `strip_noise` preserves length: the match
    offsets index the original just as well.

    **Identical calls are numbered.** Two of `newgrp`'s reads normalise to the
    same text even with their arguments restored, and a `set` comparison
    silently merges them -- so fixing one of two would look like fixing both.
    A `#2` suffix keeps them distinct.
    """
    found: list[str] = []
    for rel in sorted(tree.files_under("userspace")):
        if not rel.endswith(".rs"):
            continue
        src = tree.read_text(rel)
        if src is None:
            continue
        crate = rel.split("/")[1]
        original = src.split("#[cfg(test)]")[0]
        body = strip_noise(original)
        seen: dict[str, int] = {}
        for m in PATTERN.finditer(body):
            call = " ".join(original[m.start() : m.end()].split())
            key = f"{crate}: {call}"
            seen[key] = seen.get(key, 0) + 1
            if seen[key] > 1:
                key = f"{key}  #{seen[key]}"
            found.append(key)
    return sorted(found)


def read_baseline(tree: gittree.Tree) -> set[str] | None:
    """The pinned set, out of the REVISION rather than the disk.

    `None` means the revision carries no baseline at all, which is not the
    same as an empty one: absent says this revision has no policy -- it
    predates the ratchet, or it is a fixture -- and judging a commit by a file
    it does not carry is judging it by a rule it never had.

    Both halves of this were wrong in `multicall-aliases.py` earlier today and
    broke every lane's boot test: it read the disk, and it turned absent into
    empty so that every pinned entry looked new.
    """
    text = tree.read_text(BASELINE_REL)
    if text is None:
        return None
    names = set()
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            names.add(line)
    return names


HEADER = """\
# `read_to_string(...).unwrap_or_default()` sites pinned by
# `scripts/check-read-defaults.py --check`.
#
# Each line is a read whose failure is indistinguishable from an empty file.
# THIS FILE SHOULD ONLY EVER SHRINK. The fix is `optionalfile::read_or_empty`,
# which separates "absent" from "could not read"; adding a line here to turn a
# red --check green is the defect itself.
#
# The entries below are the ones that survived inspection on 2026-09-10 -- all
# of them report or display, none rewrites the file it read. The four that DID
# rewrite (sudo/visudo, xdg, hostnamectl, ntpd) are fixed and are not here.
# Being pinned means "known and not destructive", not "correct".
#
#     python scripts/check-read-defaults.py --update-baseline
#
"""


def _self_test() -> int:
    """Fixtures for `strip_noise`, which has been wrong twice.

    Both times it failed in the direction that looks fine: the scan still
    produced a plausible list of sites, still passed `--check`, and still
    refused when a line was unpinned. Nothing in the output said the input had
    been misread. These pin the cases by name so a third rewrite cannot lose
    them silently.
    """
    failures = 0

    def expect(label: str, got: object, want: object) -> None:
        nonlocal failures
        ok = got == want
        failures += not ok
        print(f"  {'ok  ' if ok else 'FAIL'}  {label}")
        if not ok:
            print(f"          got  {got!r}")
            print(f"          want {want!r}")

    # THE SHIPPED BUG. `find('"')` is a char literal holding a double quote and
    # appears in more than thirty crates. Without the char branch it opened a
    # string that ran to the next quote anywhere later in the file, after which
    # code was blanked as string and string contents stood as code.
    src = 'let end = rest.find(\'"\')?;\nlet x = "hidden";\n'
    out = strip_noise(src)
    expect("a char literal holding a quote does not open a string",
           "rest.find(" in out, True)
    expect("...and the string after it is still blanked",
           "hidden" in out, False)

    # Lifetimes are not char literals and must survive: they have no closing
    # quote, so blanking on sight would eat the rest of the line.
    expect("a lifetime is left alone",
           "'a" in strip_noise("fn f<'a>(s: &'a str) {}"), True)

    # A raw string does not process escapes, so `r"a\"` ends at that quote.
    # Treating the backslash as an escape swallows the terminator.
    out = strip_noise('let re = r"a\\";\nlet y = "seen";\n')
    expect("a raw string ending in a backslash terminates there",
           "seen" in out, False)
    expect("...and the code after it survives",
           "let y =" in out, True)

    # Hashed raw strings close only on the matching hash count.
    out = strip_noise('let a = r#"x"y"#;\nlet b = "gone";\n')
    expect("a hashed raw string spans an inner quote",
           "let b =" in out, True)
    expect("...and the following string is blanked",
           "gone" in out, False)

    # An ordinary string DOES process escapes.
    out = strip_noise('let a = "x\\"y";\nlet b = 1;\n')
    expect("an escaped quote does not end an ordinary string",
           "let b = 1" in out, True)

    expect("a line comment goes",
           "note" in strip_noise("let a = 1; // note\n"), False)
    expect("a block comment goes",
           "note" in strip_noise("let a = /* note */ 1;"), False)

    # Length is preserved so match offsets index the original -- the property
    # `survey` relies on to show real argument text in the baseline.
    for probe in ["let a = \"x\";", "// c\n", "r#\"q\"#", "'\\n'"]:
        expect(f"length preserved: {probe!r}",
               len(strip_noise(probe)), len(probe))

    print(f"check-read-defaults: self-test "
          f"{'FAILED' if failures else 'passed'} ({failures} failure(s))")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="exit 1 if a site appeared that is not pinned")
    ap.add_argument("--update-baseline", action="store_true", dest="update")
    ap.add_argument("--self-test", "--selftest", dest="self_test",
                    action="store_true", help="run this script's own fixtures")
    ap.add_argument("--head", metavar="REV",
                    help="judge this revision rather than the working tree")
    args = ap.parse_args()

    if args.self_test:
        return _self_test()

    try:
        tree = gittree.open_tree(str(ROOT), args.head)
    except gittree.GitTreeError as exc:
        # Exit 2, not 1: `run-checker.sh` reads 1 as "the checker found
        # something" and would print a refusal over this. A revision that
        # cannot be opened is not a finding against anyone's code.
        print(f"check-read-defaults: cannot read {args.head!r}: {exc}",
              file=sys.stderr)
        return 2

    with tree:
        found = survey(tree)
        pinned = read_baseline(tree)

    if args.update:
        BASELINE.write_text(
            HEADER + "".join(f"{f}\n" for f in found),
            encoding="utf-8",
            # newline="" so Python does not translate to CRLF on Windows, which
            # would leave the file dirty against the repo's `eol=lf` attribute.
            newline="",
        )
        print(f"wrote {BASELINE.relative_to(ROOT)} with {len(found)} entries")
        return 0

    if not args.check:
        print(f"{len(found)} read_to_string(..).unwrap_or_default() site(s):\n")
        for f in found:
            print(f"  {f}")
        return 0

    if pinned is None:
        print(f"no baseline at {BASELINE.relative_to(ROOT)}; run --update-baseline",
              file=sys.stderr)
        return 2

    current = set(found)
    new = sorted(current - pinned)
    gone = sorted(pinned - current)

    for f in gone:
        print(f"fixed: {f} -- run --update-baseline to drop the line")
    if new:
        print(
            f"\n{len(new)} NEW read_to_string(..).unwrap_or_default():\n"
            "A failed read is indistinguishable from an empty file here. If the\n"
            "caller rewrites what it read, the file is replaced by whatever was\n"
            "parsed from nothing.\n",
            file=sys.stderr,
        )
        for f in new:
            print(f"  {f}", file=sys.stderr)
        print(
            "\nUse `optionalfile::read_or_empty`, which separates a file that is\n"
            "absent from one that could not be read. See its module docs.",
            file=sys.stderr,
        )
        sys.stdout.flush()
        return 1

    print(f"ok -- {len(current)} pinned site(s), none new"
          + (f" ({len(gone)} fixed)" if gone else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
