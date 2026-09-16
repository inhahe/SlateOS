#!/usr/bin/env python3
"""Refuse a manifest entry that nothing in the tree can produce.

WHY THIS EXISTS
===============

`scripts/rootfs-bin-manifest.txt` names the binaries that should be on the
image. `create-ext4-rootfs.sh` reports names it could not stage::

    [rootfs] NOTE: N name(s) in the manifest have no built binary and were
             skipped: <names>

That message conflates two entirely different situations:

1. **Not built yet.** Normal, and common: a checkout that has not built the
   `x86_64-slateos` target has none of them. Lane A's tree is usually in this
   state. Nothing is wrong and nothing should fail.
2. **No producer anywhere in the tree.** The manifest asks for a binary that
   no crate builds -- a typo, or a promise nobody kept. Building will never
   satisfy it.

Only the second is a defect, and it hides inside a NOTE that is legitimately
noisy for the first -- so an unsatisfiable entry can sit there indefinitely
looking like an unbuilt one.

**This gate currently reports nothing, and the story of why is the reason to
trust it.** Its first version reported `awk` as having no producer anywhere.
That was wrong: `awk` is `userspace/coreutils/src/bin/awk/`, cargo's DIRECTORY
form for a multi-file binary, and it passes 171 differential cases against GNU.
The scan looked only at `src/bin/*.rs`. Before believing it I read the commit
that retired the standalone `awk`, whose own evidence said "coreutils: 171
passed, 0 differed" -- a working implementation, contradicting the finding.
The gate was wrong; the tree was fine.

That is why `producible_names` enumerates four sources rather than the obvious
one, and why the self-test asserts the real tree yields a plausible producer
set: a scan that silently under-reports producers turns every manifest entry
into a false positive, which is loud -- but one that over-reports turns a real
defect into silence, which is not.

THE SHAPE, NAMED BY LANE A
==========================

This is the inverse of a publisher with no subscriber. That is a value written
correctly that nothing reads; this is **a reference to something nothing
produces** -- a manifest naming a binary, a config naming a service, a fixture
naming a layout. A `dead_code` sweep finds the first direction. Nothing finds
the second, because the reference lives outside the language: no compiler sees
a line in a text file that names a program.

So the check has to be written per reference-kind, and this is the one for the
image manifest.

WHY A GATE RATHER THAN A HARDER ERROR IN THE ROOTFS SCRIPT
==========================================================

The rootfs script runs at image-build time, which is rare and slow. A gate runs
on every push, so an entry with no producer is caught the day it is written
rather than the next time somebody builds an image. It also keeps the script's
existing NOTE correct for case 1, which is the common and harmless one.
"""

import io
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "scripts" / "rootfs-bin-manifest.txt"
USERSPACE = ROOT / "userspace"

# `ranlib = ar` -- a second name for a binary another entry produces.
ALIAS_RE = re.compile(r"^(\S+)\s*=\s*(\S+)$")
# `name = "x"` inside a [[bin]] table.
BIN_NAME_RE = re.compile(r'^\s*name\s*=\s*"([^"]+)"', re.M)

# Far below the real figure (75), so it fires on a scan that broke rather than
# on a manifest that shrank. The empty-corpus lesson: a check that iterates
# nothing reports clean, and clean is indistinguishable from correct.
FLOOR_ENTRIES = 20


def manifest_entries(text):
    """(name, producer) for every real line. `producer == name` when no alias."""
    out = []
    for raw in text.split("\n"):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        m = ALIAS_RE.match(line)
        out.append((m.group(1), m.group(2)) if m else (line, line))
    return out


def producible_names(userspace):
    """Every binary name any crate under `userspace/` can produce.

    Three sources, because a crate's binary is not always its directory name:
    the directory itself, each `src/bin/<n>.rs`, and every `name =` under a
    `[[bin]]` table in `Cargo.toml`. Missing the third would report a crate
    that renames its binary as having no producer, which is a false positive
    on exactly the crates that did something deliberate.
    """
    names = set()
    if not userspace.is_dir():
        return names
    for crate in sorted(userspace.iterdir()):
        if not crate.is_dir():
            continue
        names.add(crate.name)
        bindir = crate / "src" / "bin"
        if bindir.is_dir():
            for f in bindir.iterdir():
                if f.suffix == ".rs":
                    names.add(f.stem)
                # CARGO'S DIRECTORY FORM: src/bin/<name>/main.rs builds a
                # binary called <name>. Missing this is not hypothetical --
                # the first version of this gate did, and reported `awk` as
                # having no producer anywhere in the tree. `awk` is
                # src/bin/awk/{main,lex,parse,interp,...}.rs, eight files,
                # 171 differential cases passing against GNU. I came within
                # one check of reporting a working implementation as deleted.
                elif f.is_dir() and (f / "main.rs").is_file():
                    names.add(f.name)
        manifest = crate / "Cargo.toml"
        if manifest.is_file():
            try:
                text = manifest.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            if "[[bin]]" in text:
                names.update(BIN_NAME_RE.findall(text))
    return names


def unproducible(entries, names):
    """Manifest entries whose producer no crate can build."""
    return [(n, p) for n, p in entries if p not in names]


SELFTEST = [
    (
        "an entry with a crate behind it is fine",
        "cp\nmv\n",
        {"cp", "mv"},
        0,
    ),
    (
        "an entry with nothing behind it is reported",
        "cp\nawk\n",
        {"cp"},
        1,
    ),
    (
        "an alias is judged by its PRODUCER, not its own name",
        "ranlib = ar\n",
        {"ar"},
        0,
    ),
    (
        "...and an alias of a producer that does not exist IS reported",
        "ranlib = nosuchtool\n",
        {"ar"},
        1,
    ),
    (
        "comments and blank lines are not entries",
        "# a comment\n\n   \ncp\n",
        {"cp"},
        0,
    ),
]


def selftest():
    bad = 0
    for name, text, names, want in SELFTEST:
        got = len(unproducible(manifest_entries(text), names))
        ok = got == want
        bad += 0 if ok else 1
        print("%-4s %s" % ("ok" if ok else "FAIL", name))
        if not ok:
            print("       wanted %d, got %d" % (want, got))

    # The producer scan must actually find things in the real tree. Without
    # this, a broken `producible_names` returns an empty set and EVERY entry
    # reports as unproducible -- which is loud, or returns everything and NONE
    # does, which is silent. The second is the dangerous one.
    real = producible_names(USERSPACE)
    ok = len(real) > 100
    bad += 0 if ok else 1
    print("%-4s the real tree yields a plausible producer set (%d names)"
          % ("ok" if ok else "FAIL", len(real)))

    print()
    print("check-manifest-producers selftest: %d case(s), %d failed"
          % (len(SELFTEST) + 1, bad))
    return 1 if bad else 0


def main(argv=None):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    argv = sys.argv[1:] if argv is None else argv
    if "--self-test" in argv or "--selftest" in argv:
        return selftest()

    if not MANIFEST.is_file():
        print("check-manifest-producers: no rootfs-bin-manifest.txt here",
              file=sys.stderr)
        return 2

    entries = manifest_entries(MANIFEST.read_text(encoding="utf-8", errors="replace"))
    if len(entries) < FLOOR_ENTRIES:
        print(
            "check-manifest-producers: REFUSING a verdict -- read %d manifest "
            "entries, expected at least %d. Something stopped it reading rather "
            "than the manifest shrinking." % (len(entries), FLOOR_ENTRIES),
            file=sys.stderr,
        )
        return 2

    names = producible_names(USERSPACE)
    if not names:
        print(
            "check-manifest-producers: REFUSING a verdict -- no crate under "
            "userspace/ yielded a binary name, so every entry would report as "
            "unproducible. That is a broken scan, not a broken manifest.",
            file=sys.stderr,
        )
        return 2

    missing = unproducible(entries, names)
    for name, producer in missing:
        if name == producer:
            print("rootfs-bin-manifest.txt: `%s` names a binary no crate produces."
                  % name)
        else:
            print("rootfs-bin-manifest.txt: `%s = %s` -- no crate produces `%s`."
                  % (name, producer, producer))
        print("    Not 'has not been built yet': nothing in userspace/ can build")
        print("    it. Either a crate is missing, or the entry is a typo, or the")
        print("    name should come out of the manifest.")

    print(
        "check-manifest-producers: %d manifest entr(ies) over %d producible "
        "binary name(s); %d with no producer."
        % (len(entries), len(names), len(missing))
    )
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
