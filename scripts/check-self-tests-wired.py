#!/usr/bin/env python3
"""Fail if a kernel `self_test` exists that nothing ever calls.

A self-test that is written but not *invoked* is not a test. It compiles, it
reads as coverage, it shows up in a commit message as "tested" -- and it has
never executed. This is not hypothetical: `evdev::self_test` existed for one
commit without a caller, and the very first boot that ran it failed hard on a
real ordering bug (`B-A-EVDEV-SYN-DROPPED-ARRIVES-ONE-RECORD-LATE`) that had
been sitting in the tree unnoticed. Every uncalled self-test is a lottery
ticket nobody scratched.

Run from the boot test's prerequisite gate, before the build, because the check
costs milliseconds and the build costs ten minutes.

Usage:
    python scripts/check-self-tests-wired.py [--root kernel/src]
                                             [--list-manual] [--quiet]

Exit codes:
    0  every self-test is reachable from a root
    1  at least one is reachable from nothing (they are listed)
    2  the check could not run (bad root, unreadable tree) -- never confused
       with "nothing is wrong", because a check that cannot fire must not be
       indistinguishable from a check that passes.

## Two roots, three answers

There are two entirely different ways a self-test gets run in this kernel, and
lumping them together produces a failure message that is factually wrong for
most of what it lists:

  boot    reachable from `main.rs` -- *can* run on a boot test. Read the next
          section before treating this as "tested": reachable is not reached.
  manual  reachable only from `kshell.rs` -- an interactive `test` subcommand
          of the kernel shell (`taskbar test`, `http test`, ...). A human can
          run it; the boot test never does. Not dead code, but not coverage
          either: a regression here is caught only if someone happens to type
          the command.
  dead    reachable from neither. Nothing anywhere can invoke it.

Only `dead` fails the build. `manual` is reported as a count (and in full under
`--list-manual`) because it is a real but different gap, and burying a
one-line-fix `dead` entry in a list of hundreds of `manual` ones is how a guard
stops being read.

## The fourth answer: `boot` does not mean "ran"

`boot` says a call *exists* in `main.rs`, not that control ever reaches it. A
call inside `if fat_ok { ... }` is reachable from `main.rs` and was counted here
as coverage for a year -- while never executing once in CI, because the boot
test mounts a memfs root and `fat_ok` is false. Seven suites sat behind that one
condition; searching a full 43,450-line serial log for "[ext4] Phase 0" returned
nothing.

So every `main.rs` call site inside a conditional is now reported separately
(`--list-gated`), with the conditions it sits under. This is deliberately *not*
a failure and deliberately *not* a verdict: a gate can be perfectly benign --
`acpi::self_test` is called from both arms of an `if`/`else` and therefore
always runs -- and this check cannot reason about that. It tells you where to
look; the serial log tells you what happened. Of the 12 sites it flags today,
5 do run and 7 do not.

Cross-checking is the point. "Is it wired up" is a question about source, and
source cannot answer "did it run" -- only the log can.

## What "reachable" means here

Not a real call-graph -- that would need to parse Rust. The rule is:

1. A self-test named in a root file is reachable.
2. A self-test named anywhere in a file that already contains a reachable
   self-test is reachable, because in this kernel the way one self-test invokes
   another is from inside a sibling's body in the same file.
3. Iterate 2 to a fixpoint.

Plus one special case that is not a guess: `pub use <path>::<fn>;` genuinely
renames a symbol, so `syscall::self_test()` in `main.rs` really is a call to
`syscall::dispatch::self_test`. Those are followed exactly rather than
approximated, because an unfollowed re-export is a *false positive* -- the one
kind of result this check cannot afford.

The rest is deliberately a little generous -- a symbol named in a *comment* in
a reachable file counts -- and deliberately not generous about the case that
actually bites: a whole module whose self-test nothing anywhere mentions. False
negatives here are cheap; a false positive would train people to pass `--quiet`
and ignore it, which is how the guard dies.

## The allowlist

An entry means "uncalled on purpose", and needs a reason that would satisfy
someone who did not write it. Keep it short; a growing allowlist is the signal
that this check has stopped being useful.
"""

import argparse
import io
import os
import re
import sys

# `module::function` -> why it is deliberately never invoked.
ALLOWLIST = {
    "hardlockup::self_test_fire": (
        "Deliberately not auto-invoked, and says so in its own doc comment: it "
        "forces the watchdog to fire, which costs a ~15 s stall on every "
        "watchdog boot and latches the one-shot NMI dump, spoiling a real "
        "catch. Wire a temporary call in for a one-off validation."
    ),
}

# The file every boot runs. A self-test reachable from here is real coverage.
BOOT_ROOT = "main.rs"

# Files that can only be reached by a human typing a command. Reachable from
# here and nowhere else means "runnable, but never actually run".
MANUAL_ROOTS = ("kshell.rs",)

DEF_RE = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?fn[ \t]+(self_test\w*)[ \t]*[(<]",
    re.M,
)

# `pub use a::b::self_test;` / `pub use a::b::{self_test, self_test_x};`
USE_RE = re.compile(
    r"\bpub(?:\([^)]*\))?[ \t]+use[ \t]+([\w:]+?)::(?:\{([^}]*)\}|(self_test\w*))[ \t]*;"
)

QUAL = re.compile(r"(\w+)::(self_test\w*)")
DECL = re.compile(r"\bfn\s+(self_test\w*)\s*[(<]")

# Two different questions, so two different patterns.
#
# `BARE_CALL` asks "is this line a call?", which is what the gated-call report
# needs: a *call* inside `if fat_ok { ... }` may not execute.
#
# `BARE_MENTION` asks "does this name reach a value?", which is what
# reachability needs, and it is deliberately looser -- Rust reaches a function
# by naming it, not only by calling it. `selftest::with_pristine(&STATE,
# State::new(), self_test_inner)` passes the suite as a function *value*, so the
# name is followed by `)`, and under the call-shaped pattern all 53 suites
# converted to that helper read as reachable from nothing. The failure mode of
# getting this wrong is the worst one available to a guard: 53 false alarms at
# once, which is how a guard gets `--quiet`-ed permanently.
BARE_CALL = re.compile(r"(?<![\w:])(self_test\w*)\s*[(<]")
BARE_MENTION = re.compile(r"(?<![\w:])(self_test\w*)(?![\w:])")


def module_path(rel):
    """`fs/ext4/mod.rs` -> `fs::ext4`; `fs/devfs.rs` -> `fs::devfs`."""
    parts = rel[:-3].split("/")
    if parts and parts[-1] == "mod":
        parts = parts[:-1]
    return "::".join(parts)


def load_tree(root):
    files = {}
    for dirpath, _dirs, names in os.walk(root):
        for nm in sorted(names):
            if not nm.endswith(".rs"):
                continue
            path = os.path.join(dirpath, nm)
            rel = os.path.relpath(path, root).replace("\\", "/")
            with io.open(path, "r", encoding="utf-8", errors="replace") as fh:
                files[rel] = fh.read()
    return files


def index_definitions(files):
    """symbol -> (rel path, module path, bare fn name)."""
    defs = {}
    for rel, text in files.items():
        mod = module_path(rel)
        for m in DEF_RE.finditer(text):
            fn = m.group(1)
            defs["%s::%s" % (mod, fn) if mod else fn] = (rel, mod, fn)
    return defs


def index_call_spellings(defs):
    """Index definitions by how call sites actually spell them.

    Every call site in this tree writes at most the last module component
    (`acl::self_test`, never the full `fs::acl::self_test`), so that is the key.
    One dict lookup beats re-scanning the tree per symbol: the naive form was
    O(definitions x files) and took minutes on 1000+ files.
    """
    by_qualified = {}   # ("acl", "self_test") -> [sym, ...]
    by_bare = {}        # "self_test_discard"  -> [sym, ...]
    for sym, (_rel, mod, fn) in defs.items():
        last = mod.rsplit("::", 1)[-1] if mod else ""
        by_qualified.setdefault((last, fn), []).append(sym)
        by_bare.setdefault(fn, []).append(sym)
    return by_qualified, by_bare


def add_reexport_aliases(files, by_qualified):
    """Teach the index that `pub use dispatch::self_test;` renames a symbol.

    `syscall/mod.rs` re-exports `dispatch::self_test`, and `main.rs` calls it as
    `syscall::self_test()`. Without this the definition looks uncalled, which is
    a false positive -- and a guard that cries wolf once is a guard that gets
    `--quiet`-ed forever after.
    """
    aliases = []
    for rel, text in files.items():
        mod = module_path(rel)
        if not mod:
            continue
        via = mod.rsplit("::", 1)[-1]
        for m in USE_RE.finditer(text):
            path, braced, single = m.group(1), m.group(2), m.group(3)
            names = ([single] if single else
                     [n.strip() for n in braced.split(",")])
            # The module the symbol really lives in is the last component of
            # the `use` path, which is exactly the key `by_qualified` uses.
            src = path.rsplit("::", 1)[-1]
            for fn in names:
                if not fn.startswith("self_test"):
                    continue
                for sym in by_qualified.get((src, fn), ()):
                    aliases.append(((via, fn), sym))
    for key, sym in aliases:
        bucket = by_qualified.setdefault(key, [])
        if sym not in bucket:
            bucket.append(sym)
    return len(aliases)


def collect_mentions(files, defs, by_qualified, by_bare):
    """rel path -> set of symbols that file names.

    Scans the *code*, not the text: a name written in a doc comment is not a
    caller, and counting it as one hides a genuinely dead suite behind its own
    documentation. That matters more now than it used to -- the suites this
    guard watches carry doc comments that discuss `self_test` by name.
    """
    mentions = {}
    for rel, text in files.items():
        text = strip_text(text)
        hit = set()
        for m in QUAL.finditer(text):
            for sym in by_qualified.get((m.group(1), m.group(2)), ()):
                hit.add(sym)
        # A bare mention only counts inside the defining file, and only in
        # excess of the declarations -- every file contains its own `fn` line.
        used, declared = {}, {}
        for m in BARE_MENTION.finditer(text):
            used[m.group(1)] = used.get(m.group(1), 0) + 1
        for m in DECL.finditer(text):
            declared[m.group(1)] = declared.get(m.group(1), 0) + 1
        for fn, count in used.items():
            if count <= declared.get(fn, 0):
                continue
            for sym in by_bare.get(fn, ()):
                if defs[sym][0] == rel:
                    hit.add(sym)
        mentions[rel] = hit
    return mentions


def reachable_from(roots, mentions, defs):
    """Fixpoint: a reachable self-test's file may name others."""
    out = set()
    for root in roots:
        out |= mentions.get(root, set())
    seen_files = set(roots)
    changed = True
    while changed:
        changed = False
        for rel in sorted({defs[s][0] for s in out} - seen_files):
            seen_files.add(rel)
            for sym in mentions.get(rel, ()):
                if sym not in out:
                    out.add(sym)
                    changed = True
    return out


# --- "Could run" is not "did run" ----------------------------------------
#
# `reachable_from(main.rs)` answers whether a call *exists*, not whether it is
# *reached*. A call inside `if fat_ok { ... }` is reachable from main.rs and so
# gets counted as boot coverage -- yet on a boot path where the condition is
# false it never executes, and nothing says so. That is not hypothetical: eight
# ext4 suites sat inside `if fat_ok` and had never once run in CI, because the
# boot test boots a memfs root and mounts ext4 at /mnt instead. This guard
# cheerfully counted all eight as "run at boot" the whole time. See
# known-issues.md, "Seven of the eight boot self-tests behind `if fat_ok`".
#
# Finding these needs no Rust parser -- only main.rs's block structure, which
# is brace-counted below. The one thing brace counting cannot survive is a
# brace inside a string or a comment, and main.rs is thick with
# `serial_println!("... {:?}", e)`, so those are blanked first. If the blanking
# is ever wrong the depth will not return to zero at EOF, and this reports that
# it could not analyse the file rather than reporting a wrong answer: a
# silently mis-parsed gate list is worse than no gate list.

COND_OPEN = re.compile(r"^(?:\}[ \t]*)*(else[ \t]+if|else|if|match|while|for)\b")
RAW_STR = re.compile(r"r(#*)\"")
CHAR_LIT = re.compile(r"'(?:\\.|[^\\'])'")


def strip_code(line, in_block):
    """Blank string literals and comments, preserving length and brace counts.

    Characters are replaced by spaces rather than deleted so that anything
    matched against the result still lines up with the original text.
    """
    out, i, n = [], 0, len(line)
    while i < n:
        if in_block:
            if line.startswith("*/", i):
                in_block, i = False, i + 2
                out.append("  ")
            else:
                out.append(" ")
                i += 1
            continue
        if line.startswith("//", i):
            out.append(" " * (n - i))
            break
        if line.startswith("/*", i):
            in_block, i = True, i + 2
            out.append("  ")
            continue
        m = RAW_STR.match(line, i)
        if m:
            close = '"' + "#" * len(m.group(1))
            end = line.find(close, m.end())
            stop = n if end < 0 else end + len(close)
            out.append(" " * (stop - i))
            i = stop
            continue
        if line[i] == '"':
            j = i + 1
            while j < n:
                if line[j] == "\\":
                    j += 2
                    continue
                if line[j] == '"':
                    j += 1
                    break
                j += 1
            out.append(" " * (j - i))
            i = j
            continue
        if line[i] == "'":
            m = CHAR_LIT.match(line, i)
            if m:                      # a char literal; a bare `'a` is a lifetime
                out.append(" " * (m.end() - i))
                i = m.end()
                continue
        out.append(line[i])
        i += 1
    return "".join(out), in_block


def strip_text(text):
    """`strip_code` over a whole file, carrying block-comment state across lines."""
    out, in_block = [], False
    for line in text.split("\n"):
        stripped, in_block = strip_code(line, in_block)
        out.append(stripped)
    return "\n".join(out)


def calls_on(text, defs, by_qualified, by_bare, rel):
    """Symbols named by this one line, using the same spellings as elsewhere."""
    hit = set()
    for m in QUAL.finditer(text):
        for sym in by_qualified.get((m.group(1), m.group(2)), ()):
            hit.add(sym)
    if not DECL.search(text):          # a definition is not a call
        for m in BARE_CALL.finditer(text):
            for sym in by_bare.get(m.group(1), ()):
                if defs[sym][0] == rel:
                    hit.add(sym)
    return hit


def find_gated_calls(src, defs, by_qualified, by_bare, rel):
    """Self-test calls in `src` that sit inside a conditional block.

    Returns (findings, ok). `ok` is False when brace depth did not return to
    zero, meaning the result must not be trusted.

    A call in an `if`'s *condition* -- `if let Err(e) = foo::self_test()` -- is
    not gated by that `if`: it runs in order to decide the branch. So calls on a
    line are attributed to the scopes already open *before* that line, and the
    scope the line itself opens is pushed afterwards.
    """
    stack, depth, findings, in_block = [], 0, [], False
    for lineno, raw in enumerate(src.split("\n"), 1):
        code, in_block = strip_code(raw, in_block)
        stripped = code.strip()

        # Leading `}` close scopes before anything on this line is attributed.
        lead = 0
        for ch in stripped:
            if ch == "}":
                lead += 1
            elif not ch.isspace():
                break
        depth -= lead
        while stack and depth <= stack[-1][0]:
            stack.pop()

        syms = calls_on(code, defs, by_qualified, by_bare, rel)
        if syms and stack:
            findings.append((lineno, raw.strip(), sorted(syms), list(stack)))

        m = COND_OPEN.match(stripped)
        if m and stripped.endswith("{"):
            stack.append((depth, m.group(1), stripped, lineno))
        depth += code.count("{") - code.count("}") + lead

    return findings, (depth == 0 and not stack)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--root", default=None, help="kernel source root")
    ap.add_argument("--list-manual", action="store_true",
                    help="also list the kshell-only self-tests, not just count them")
    ap.add_argument("--list-gated", action="store_true",
                    help="also list the main.rs calls that sit inside a conditional")
    ap.add_argument("--quiet", action="store_true", help="only print failures")
    args = ap.parse_args()

    here = os.path.dirname(os.path.abspath(__file__))
    root = args.root or os.path.normpath(os.path.join(here, "..", "kernel", "src"))
    if not os.path.isdir(root):
        print("ERROR: not a directory: %s" % root, file=sys.stderr)
        return 2

    files = load_tree(root)
    if BOOT_ROOT not in files:
        print("ERROR: no %s under %s -- wrong root?" % (BOOT_ROOT, root),
              file=sys.stderr)
        return 2

    defs = index_definitions(files)
    if not defs:
        print("ERROR: found no self_test definitions at all -- the pattern is "
              "probably wrong, and an empty check is worse than none.",
              file=sys.stderr)
        return 2

    by_qualified, by_bare = index_call_spellings(defs)
    n_aliases = add_reexport_aliases(files, by_qualified)
    mentions = collect_mentions(files, defs, by_qualified, by_bare)

    boot = reachable_from([BOOT_ROOT], mentions, defs)
    manual_roots = [r for r in MANUAL_ROOTS if r in files]
    manual = reachable_from(manual_roots, mentions, defs) - boot

    gated, gated_ok = find_gated_calls(
        files[BOOT_ROOT], defs, by_qualified, by_bare, BOOT_ROOT)

    dead, allowed = [], []
    for sym in sorted(defs):
        if sym in boot or sym in manual:
            continue
        (allowed if sym in ALLOWLIST else dead).append(sym)

    if not args.quiet:
        print("self-tests defined: %d" % len(defs))
        print("  run at boot (reachable from %s):        %d" % (BOOT_ROOT, len(boot)))
        print("  manual only (reachable from %s): %d"
              % ("/".join(manual_roots) or "-", len(manual)))
        print("  allowlisted as deliberately uncalled:   %d" % len(allowed))
        print("  reachable from nothing:                 %d" % len(dead))
        if n_aliases:
            print("  (%d `pub use` re-export(s) followed)" % n_aliases)

    if manual and (args.list_manual or not args.quiet):
        print("")
        print("NOTE: %d self-test(s) exist only as kernel-shell subcommands, so "
              "the boot" % len(manual))
        print("test never runs them.  They are not dead, but they are not "
              "coverage either --")
        print("a regression in one is caught only if a human types the command.")
        if args.list_manual:
            print("")
            for sym in sorted(manual):
                print("  %-46s %s" % (sym, defs[sym][0]))
        else:
            print("Re-run with --list-manual to see them.")

    if not gated_ok and not args.quiet:
        print("")
        print("NOTE: could not brace-match %s, so the conditional-call check "
              "was skipped." % BOOT_ROOT)
        print("This means a string or comment defeated the blanking pass, not "
              "that the")
        print("file is malformed -- but a mis-parsed answer would be worse "
              "than none.")
    elif gated and not args.quiet:
        print("")
        print("NOTE: %d self-test call site(s) in %s sit inside a conditional, "
              "so whether" % (len(gated), BOOT_ROOT))
        print("they run depends on the boot path.  Being reachable from %s is "
              "not the" % BOOT_ROOT)
        print("same as being reached: a suite behind a condition that is false "
              "in CI has")
        print("never run, however green the boot test looks.  Check each "
              "against the")
        print("serial log before citing it as coverage.")
        if args.list_gated:
            print("")
            for lineno, text, syms, stack in gated:
                conds = " > ".join("%s (line %d)" % (fr[2].rstrip(" {"), fr[3])
                                   for fr in stack)
                print("  %s:%d" % (BOOT_ROOT, lineno))
                print("      call:  %s" % text)
                print("      under: %s" % conds)
                print("      tests: %s" % ", ".join(syms))
        else:
            print("Re-run with --list-gated to see them.")

    if dead:
        print("")
        print("=== UNWIRED SELF-TESTS: %d ===" % len(dead), file=sys.stderr)
        print("", file=sys.stderr)
        print("These functions exist and nothing anywhere can call them -- not "
              "the boot", file=sys.stderr)
        print("path, not the kernel shell.  They have never run, so whatever "
              "they assert", file=sys.stderr)
        print("has never been checked, and any commit message that cited them "
              "as coverage", file=sys.stderr)
        print("was wrong.", file=sys.stderr)
        print("", file=sys.stderr)
        for sym in dead:
            print("  %-46s %s" % (sym, defs[sym][0]), file=sys.stderr)
        print("", file=sys.stderr)
        print("Fix by calling it from kernel_main (see the `#[inline(never)] "
              "fn case()`", file=sys.stderr)
        print("blocks in %s), or -- if it must not run at boot -- add it to "
              "ALLOWLIST" % BOOT_ROOT, file=sys.stderr)
        print("in %s with a reason."
              % os.path.relpath(os.path.abspath(__file__)), file=sys.stderr)
        return 1

    if not args.quiet and allowed:
        print("")
        for sym in allowed:
            print("  allowlisted: %s" % sym)
    return 0


if __name__ == "__main__":
    sys.exit(main())
