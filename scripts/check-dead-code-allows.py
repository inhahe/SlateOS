#!/usr/bin/env python3
"""Refuse a NEW crate-level ``#![allow(..., dead_code, ...)]`` in lane B's tree.

WHY THIS EXISTS. ``userspace/gdb`` carried 105 lines of a function nothing
called -- a second, wrong expression tokeniser that resolved ``$rax`` to the
register's *index*, sitting beside the working one. rustc never mentioned it,
because line 55 of that file is::

    #![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, dead_code)]

and the careful eight-line comment above it justifies the first two lints for
a DWARF and ELF parser, and says nothing about the third. That is the shape
this gate is about: a defensible suppression with an indefensible one appended
to it, where the comment covers the neighbours and the reader's eye passes on.

Twenty-two crates shared that shape when this was written, hiding 180
findings measured on ``x86_64-unknown-linux-gnu``. Both numbers are history
and are deliberately not restated as current: the live count is
``len(BASELINE)`` and the run prints it. A docstring that carries a total is
a total that goes stale, which is the same failure as the comments this gate
exists to replace -- gdb's justified two lints and not the third, wpa's
described personalities deleted three days earlier, and logind's cited a
todo.txt note that has never existed. See known-issues.md ->
B-PROGRAMS-THAT-INVENT-THEIR-OUTPUT.

WHAT IT DOES NOT FORBID. A per-item ``#[allow(dead_code)]`` is fine and is the
intended destination -- a complete ELF or DBus constant table with some entries
unread is legitimate, and saying so at the item, with a reason, is exactly
right. What is forbidden is the *crate-level* form, which cannot distinguish a
deliberate table from a real finding.

THE BASELINE RATCHETS BOTH WAYS. A crate not in BASELINE that gains one fails.
A crate in BASELINE that *loses* one also fails, asking for the baseline to
shrink. Without the second half the list goes stale and starts certifying
crates that were cleaned long before -- the failure mode this project keeps
finding in its own documents, where a statement true when written is read as
present tense.

Run ``--selftest`` to check the detector against its own fixtures.
"""

import os
import sys

import selftestflag

ROOTS = ("userspace", "services", "init", "posix")

# The crates that already had one when this gate was written. Shrink this as
# crates are cleaned; the gate fails if an entry no longer needs to be here.
BASELINE = {
    "userspace/acpi",
    "userspace/ar",
    "userspace/blkid",
    "userspace/dhcpcd",
    "userspace/findmnt",
    "userspace/finger",
    "userspace/getty",
    "userspace/irqbalance",
    "userspace/ldconfig",
    "userspace/login",
    "userspace/ntpd",
    "userspace/oils",
    "userspace/resolvectl",
    "userspace/ss",
    "userspace/tcpdump",
    "userspace/upower",
}

# Conditions under which a crate-level dead_code allow is inherently narrow,
# because the condition excludes the configuration that actually ships.
#
#   test        -- the harness replaces `main`, so the whole CLI entry path is
#                  unreachable in that build and live in the real one.
#   not(unix)   -- a crate whose `#[cfg(unix)]` half is compiled out on a
#                  Windows host reports its entire unix side dead. `logind`
#                  reports 31 such items and is clean on linux-gnu.
#
# Anything else -- `all()` above all, which is unconditional wearing a
# condition -- is treated as a blanket allow. That distinction is the whole
# point: the first version of this gate matched only `#![allow(`, so every
# form below slipped past it untouched, including the tautological one.
NARROW_CONDS = ("test", "not(unix)")

NL = chr(10)


def strip_line_comments(text):
    """Drop ``//`` comments so a mention of dead_code in prose is not a hit.

    Block comments are left alone: an attribute is not written inside one in
    this tree, and a half-right comment stripper is worse than none.
    """
    out = []
    for line in text.split(NL):
        i = line.find("//")
        out.append(line if i < 0 else line[:i])
    return NL.join(out)


def crate_level_dead_code(text):
    """How a crate-level attribute allows ``dead_code``: ``None``, ``"plain"``
    or ``"cfg"``.

    Paren-matched rather than line-matched, because the attribute is often
    split across lines and a line regex would miss exactly the ones that are
    hardest to notice by eye.

    ``cfg_attr`` is matched too, and finding that it was not is the reason
    this returns a kind rather than a bool. `#![cfg_attr(not(unix),
    allow(dead_code))]` is legitimate in a crate whose unix half is compiled
    out on the build host -- `logind` is exactly that -- but nothing stopped
    `#![cfg_attr(all(), allow(dead_code))]`, which silences the crate
    unconditionally and slipped straight past the first version of this gate.
    A conditional allow is permitted and must be declared in ``CFG_SCOPED``
    with its reason, so the narrow form stays available and stays visible.
    """
    text = strip_line_comments(text)
    if _names_dead_code(text, "#![allow("):
        return "plain"
    cond = _cfg_attr_condition(text)
    if cond is None:
        return None
    return "cfg" if cond.replace(" ", "") in NARROW_CONDS else "plain"


def _cfg_attr_condition(text):
    """The condition of a crate-level ``cfg_attr`` that allows dead_code."""
    i = 0
    while True:
        i = text.find("#![cfg_attr(", i)
        if i < 0:
            return None
        j = text.index("(", i)
        depth = 0
        k = j
        while k < len(text):
            if text[k] == "(":
                depth += 1
            elif text[k] == ")":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        body = text[j + 1:k]
        if _names_dead_code("#![allow(" + body + ")]", "#![allow("):
            # The condition is everything before the comma that separates it
            # from the attribute, at paren depth zero.
            d = 0
            for n, ch in enumerate(body):
                if ch == "(":
                    d += 1
                elif ch == ")":
                    d -= 1
                elif ch == "," and d == 0:
                    return body[:n].strip()
            return body.strip()
        i = k + 1


def _names_dead_code(text, opener):
    i = 0
    while True:
        i = text.find(opener, i)
        if i < 0:
            return False
        j = text.index("(", i)
        depth = 0
        k = j
        while k < len(text):
            if text[k] == "(":
                depth += 1
            elif text[k] == ")":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        # Token-exact. `clippy::dead_code_like_name` contains the string and
        # is a different lint; a substring test called it a hit.
        body = text[j + 1:k]
        for sep in ",()":
            body = body.replace(sep, " ")
        if "dead_code" in body.split():
            return True
        i = k + 1


def crate_files(base="."):
    """Every ``.rs`` file under the lane's dirs, paired with its crate.

    Not just crate roots. A module file may carry its own inner
    ``#![allow(dead_code)]``, which blinds that module exactly as thoroughly
    as the crate-level form blinds the crate -- and being one directory down
    is what makes it *less* likely to be noticed, not more.

    A crate is the nearest ancestor directory holding a ``Cargo.toml``.
    """
    found = []
    for root in ROOTS:
        top = os.path.join(base, root)
        if not os.path.isdir(top):
            continue
        for dirpath, dirnames, filenames in os.walk(top):
            dirnames[:] = [d for d in dirnames if d != "target"]
            for name in filenames:
                if not name.endswith(".rs"):
                    continue
                path = os.path.join(dirpath, name)
                crate = dirpath
                stop = os.path.abspath(base)
                while not os.path.isfile(os.path.join(crate, "Cargo.toml")):
                    parent = os.path.dirname(crate)
                    # Never ascend past `base`: without this a file in a
                    # directory with no Cargo.toml walked out of the tree
                    # and reported its crate as "..".
                    if parent == crate or os.path.abspath(crate) == stop:
                        crate = dirpath
                        break
                    crate = parent
                rel = os.path.relpath(crate, base).replace(os.sep, "/")
                found.append((rel, path))
    return sorted(set(found))


def selftest():
    import tempfile

    cases = [
        ("#![allow(dead_code)]", "plain", "bare"),
        ("#![allow(clippy::all, dead_code)]", "plain", "trailing in a list"),
        ("#![allow(" + NL + "    clippy::all," + NL + "    dead_code," + NL + ")]",
         "plain", "split over lines"),
        ("#![allow(clippy::all)]", None, "unrelated lint"),
        ("#[allow(dead_code)]" + NL + "fn f() {}", None, "per-item is allowed"),
        ("// dead_code is discussed here", None, "mentioned in a comment"),
        ("#![allow(clippy::type_complexity)] // dead_code", None, "comment after"),
        ("#![allow(nonstandard_style)]" + NL + "#![allow(dead_code)]",
         "plain", "second attribute"),
        ("#![allow(clippy::dead_code_like_name)]", None, "substring is not the lint"),
        ("#![cfg_attr(not(unix), allow(dead_code))]", "cfg", "conditional form"),
        ("#![cfg_attr(all(), allow(dead_code))]", "plain",
         "a tautological cfg counts as blanket, not as scoped"),
        ("#[cfg_attr(test, allow(dead_code))]" + NL + "fn f() {}", None, "per-item cfg_attr is allowed"),
    ]
    bad = 0
    for text, want, why in cases:
        got = crate_level_dead_code(text)
        if got != want:
            print("  selftest FAIL (%s): wanted %s got %s" % (why, want, got))
            bad += 1

    # And that the walker actually finds a crate root, so a silent zero
    # cannot pass for a clean tree -- "ok out of 0" and "ok out of 900"
    # must not print the same word.
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "userspace", "zzq", "src")
        os.makedirs(src)
        with open(os.path.join(d, "userspace", "zzq", "Cargo.toml"),
                  "w", encoding="utf-8", newline="") as fh:
            fh.write('[package]' + NL + 'name = "zzq"' + NL)
        with open(os.path.join(src, "main.rs"), "w", encoding="utf-8",
                  newline="") as fh:
            fh.write("#![allow(dead_code)]")
        # A module file one directory down, to pin that the walker attributes
        # it to the crate rather than to its own directory.
        deep = os.path.join(src, "sub")
        os.makedirs(deep)
        with open(os.path.join(deep, "mod.rs"), "w", encoding="utf-8",
                  newline="") as fh:
            fh.write("pub fn f() {}")
        names = sorted({r[0] for r in crate_files(d)})
        if names != ["userspace/zzq"]:
            print("  selftest FAIL (walker): %s" % (names,))
            bad += 1

    print("check-dead-code-allows: selftest %s (%d cases)"
          % ("FAILED" if bad else "ok", len(cases) + 1))
    return 1 if bad else 0


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)

    # All three spellings, through the shared helper rather than a private
    # copy of the list: a mistyped `--self_test` that fell through to the
    # scan would answer "is this checker still correct?" with yes, without
    # having asked.
    if selftestflag.wants_selftest(argv):
        return selftest()

    # And refuse an option we do not have, rather than ignoring it and
    # printing a verdict the caller will read as covering whatever they
    # thought they asked for.
    unknown = selftestflag.unknown_options(argv)
    if unknown:
        for opt in unknown:
            print("check-dead-code-allows: unrecognized option '%s'" % opt)
        print("usage: check-dead-code-allows.py [--self-test]")
        return 2

    files = crate_files(".")
    crates = sorted({crate for crate, _ in files})
    # Group first: a crate with both a main.rs and a lib.rs was otherwise
    # reported twice, and one clean file did not make the crate clean.
    hits = {}
    for crate, path in files:
        try:
            with open(path, encoding="utf-8", errors="replace") as fh:
                text = fh.read()
        except OSError:
            continue
        kind = crate_level_dead_code(text)
        if kind:
            hits.setdefault(crate, []).append((path, kind))

    # A conditional allow is fine if declared; an undeclared one of either
    # kind is not. Checking the kind matters: `cfg_attr(all(), ...)` is
    # unconditional in effect and must not pass as a scoped exception.
    offenders = [
        (c, [p for p, _ in hits[c]])
        for c in sorted(hits)
        if c not in BASELINE and any(k == "plain" for _, k in hits[c])
    ]
    plain_hits = {c for c, v in hits.items() if any(k == "plain" for _, k in v)}
    clean_baseline = [c for c in BASELINE if c not in plain_hits and c in crates]
    # A baselined crate that no longer exists is also stale, and saying so
    # separately keeps the "go fix it" advice from being wrong.
    vanished = [c for c in BASELINE if c not in crates]

    if offenders:
        print("check-dead-code-allows: %d crate(s) added a blanket "
              "dead_code allow:" % len(offenders))
        for crate, paths in offenders:
            for path in paths:
                print("  %s  (%s)" % (crate, path))
        print("")
        print("A blanket allow cannot tell a deliberate constant table from")
        print("a function nobody calls. Put #[allow(dead_code)] on the item")
        print("instead, with a comment saying why it is kept.")
        return 1

    if clean_baseline or vanished:
        for crate in sorted(clean_baseline):
            print("check-dead-code-allows: %s no longer needs the allow -- "
                  "remove it from BASELINE." % crate)
        for crate in sorted(vanished):
            print("check-dead-code-allows: %s is in BASELINE and no longer "
                  "exists -- remove it." % crate)
        print("")
        print("The baseline ratchets both ways on purpose: a list that only")
        print("grows starts certifying crates that were cleaned long before.")
        return 1

    print("check-dead-code-allows: %d file(s) in %d crate(s) scanned, "
          "%d baselined, %d narrowly cfg-scoped, 0 new."
          % (len(files), len(crates), len(BASELINE),
             len(hits) - len(plain_hits)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
