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
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "read-defaults-baseline.txt"

PATTERN = re.compile(
    r"(?:fs::)?read_to_string\s*\((?:[^()]|\([^()]*\))*\)\s*(?:\n\s*)?\.unwrap_or_default\s*\(\s*\)",
    re.S,
)


def strip_noise(src: str) -> str:
    """Blank comments and string literals, preserving length and newlines.

    Without this the scan matches its own documentation: the four fixes above
    each carry a doc comment quoting the line they replaced, and an earlier
    version of this query counted all of them as live code.
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


def survey() -> list[str]:
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
    for path in sorted((ROOT / "userspace").glob("*/src/**/*.rs")):
        try:
            src = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        crate = path.relative_to(ROOT).parts[1]
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


def read_baseline() -> set[str] | None:
    if not BASELINE.is_file():
        return None
    names = set()
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
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


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="exit 1 if a site appeared that is not pinned")
    ap.add_argument("--update-baseline", action="store_true", dest="update")
    args = ap.parse_args()

    found = survey()

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

    pinned = read_baseline()
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
