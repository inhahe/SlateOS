#!/usr/bin/env python3
"""Refuse a program that reports an unknown option and then keeps going.

A command that prints

    cgcreate: unknown option: --zzq-not-an-option

and carries on has done something worse than ignoring the option: it has told
the caller it did not understand the command line, and then acted on that
command line anyway. Measured on the real binary before the fix:

    $ cgcreate --zzq-not-an-option -g cpu:/zzqproceed
    cgcreate: unknown option: --zzq-not-an-option
    cgcreate: created /sys/fs/cgroup/zzqproceed
    $ echo $?
    0

The diagnostic is printed AFTER the work, which is the clearest statement of
the problem available.

WHY THIS IS A STATIC CHECK AND NOT A PROBE. The behavioural sweep for this
class runs `<prog> --zzq-not-an-option` with no other arguments and reads the
exit code. That cannot distinguish

    the program rejected the option          <- what we want to know
    the program ignored the option and then
      failed its own missing-operand check   <- also exits 1

and eleven programs sat in the second group reading as green, which is how the
class came to be recorded CLOSED at 0 while nine of them still had it. Adding
an operand to the probe is the fix for that sweep, but "a valid operand" is
program-specific -- a device for `wipefs`, a cgroup spec for `cgcreate`, a path
for `fuser` -- so there is no general behavioural probe. The source, on the
other hand, says plainly whether the arm stops.

WHAT COUNTS AS STOPPING. Any of:

  * `process::exit(...)`
  * a `return`
  * the enclosing block's tail expression being a non-zero integer literal --
    the common `_ => { eprintln!(...); 1 }` shape, where the 1 IS the exit code
  * `panic!` / `unreachable!` / `todo!`
  * recording the failure in a variable whose name says so (`had_error`,
    `status`, ...), which is what `getopt(1)` does deliberately so it can
    report every bad option rather than only the first

`break` and `continue` are deliberately NOT stops. Breaking out of an argument
loop usually leads straight into doing the work, which is the defect itself.

WHAT THIS CANNOT SEE, stated because the gap is real and the two checks are
complementary rather than redundant. This gate finds a diagnostic that does not
stop. It cannot find an arm that says NOTHING -- `_ => {}` -- because there is
no diagnostic to anchor on. Two of the thirteen programs fixed on 2026-09-13
were that shape: `lscgroup --zzq-not-an-option` produced output byte-identical
to `lscgroup` and exited 0, and this checker would never have reported it. That
half is the behavioural sweep's, and the behavioural sweep is in turn blind to
the half this one catches. Run against the commit before the fixes, this gate
reports 11 of the 13.

Usage:

    python scripts/check-option-stops.py [--roots DIR ...] [--list]
    python scripts/check-option-stops.py --selftest
"""

import argparse
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gittree  # noqa: E402  (needs the path above)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Lane B's trees. `--roots` overrides, so another lane can run this over its
# own without widening a default that would fail on crates its runner cannot
# fix.
ROOTS = ("userspace", "services", "init", "posix")

BASELINE = os.path.join(ROOT, "scripts", "option-stops-baseline.txt")

NL = chr(10)

DIAG = re.compile(
    r"(unrecogni[sz]ed option|unknown option|invalid option|illegal option"
    r"|unknown flag|unrecogni[sz]ed argument)",
    re.IGNORECASE,
)
PRINT_MACRO = re.compile(r"(e?print(ln)?!|write(ln)?!)\s*\(")
# Things that must come AFTER the diagnostic to count: they end the command.
STOP = re.compile(
    r"(process::exit\s*\(|\breturn\b|\bpanic!|\bunreachable!|\btodo!)",
    re.IGNORECASE,
)
# Recording the failure counts wherever it appears in the enclosing block.
# `getopt(1)` sets its flag on the line BEFORE the message, because it reports
# every bad option and returns non-zero once at the end. A tail-only scan calls
# that correct shape a defect, which is how this rule earned its own self-test
# case rather than being written from the armchair.
RECORDS_FAILURE = re.compile(
    r"\w*(err|fail|bad|status|exit)\w*\s*=\s*(true|[1-9])",
    re.IGNORECASE,
)
# A block whose last expression is a non-zero integer: `{ eprintln!(..); 1 }`.
TAIL_CODE = re.compile(r"[;}]\s*([1-9][0-9]*)\s*$")


def test_ranges(src):
    """Byte ranges of `#[cfg(test)]` items, which are not shipped code."""
    out = []
    for m in re.finditer(r"#\[cfg\(test\)\]", src):
        b = src.find("{", m.end())
        if b < 0:
            continue
        depth = 0
        for i in range(b, len(src)):
            if src[i] == "{":
                depth += 1
            elif src[i] == "}":
                depth -= 1
                if depth == 0:
                    out.append((m.start(), i))
                    break
    return out


def enclosing_block(src, pos):
    """(start, end) of the innermost brace block containing `pos`."""
    depth = 0
    start = None
    for i in range(pos, -1, -1):
        c = src[i]
        if c == "}":
            depth += 1
        elif c == "{":
            if depth == 0:
                start = i
                break
            depth -= 1
    if start is None:
        return None
    depth = 0
    for i in range(start, len(src)):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                return (start, i)
    return None


def stops_after(src, pos, levels=2):
    """Does the code after `pos` stop, within `levels` enclosing blocks?

    Two levels, because the diagnostic is often wrapped one deeper than the
    refusal -- `if !opts.quiet { eprintln!(...) }` with the `return 1` in the
    arm outside it. That shape is `mktemp`'s and it is correct; reading only
    the innermost block reports it as a defect.
    """
    at = pos
    for _ in range(levels):
        blk = enclosing_block(src, at)
        if not blk:
            return False
        semi = src.find(";", pos)
        tail = src[semi:blk[1]] if 0 <= semi < blk[1] else src[pos:blk[1]]
        if STOP.search(tail):
            return True
        if TAIL_CODE.search(src[blk[0]:blk[1]].rstrip()):
            return True
        if RECORDS_FAILURE.search(src[blk[0]:blk[1]]):
            return True
        at = blk[0] - 1
        if at < 0:
            return False
    return False


def judge(rel, src):
    """Findings in one file. Split out so the self-test can call it directly."""
    out = []
    seen = 0
    skip = test_ranges(src)
    for m in DIAG.finditer(src):
        if any(a <= m.start() <= b for a, b in skip):
            continue
        ls = src.rfind(NL, 0, m.start()) + 1
        le = src.find(NL, m.start())
        line = src[ls:le if le >= 0 else len(src)]
        if not PRINT_MACRO.search(line):
            continue
        seen += 1
        if not stops_after(src, m.start()):
            out.append((rel, src[:m.start()].count(NL) + 1, line.strip()))
    return out, seen


def scan(tree, roots):
    """Judge every `.rs` file under `roots`, reading through the tree seam.

    Reading through `gittree` rather than the disk is what lets `--head` judge
    the commit being pushed: a defect introduced by a commit cannot be hidden
    by a working tree that has since been tidied, and an unrelated uncommitted
    edit cannot block a push of clean commits.
    """
    findings = []
    files = 0
    diagnostics = 0
    for root in roots:
        if not tree.is_dir(root):
            continue
        for rel in tree.files_under(root):
            if not rel.endswith(".rs"):
                continue
            src = tree.read_text(rel)
            if src is None:
                continue
            files += 1
            found, seen = judge(rel, src)
            findings.extend(found)
            diagnostics += seen
    return findings, files, diagnostics


def read_baseline():
    try:
        with open(BASELINE, encoding="utf-8") as fh:
            return {
                ln.split("#", 1)[0].strip()
                for ln in fh
                if ln.split("#", 1)[0].strip()
            }
    except OSError:
        return set()


def selftest():
    bad = 0
    checks = 0

    def check(ok, msg):
        nonlocal bad, checks
        checks += 1
        if not ok:
            print("selftest FAIL: " + msg, file=sys.stderr)
            bad += 1

    def flagged(body):
        m = DIAG.search(body)
        if m is None:
            return False
        ls = body.rfind(NL, 0, m.start()) + 1
        le = body.find(NL, m.start())
        line = body[ls:le if le >= 0 else len(body)]
        if not PRINT_MACRO.search(line):
            return False
        return not stops_after(body, m.start())

    # The defect, in the two shapes it was found in.
    check(
        flagged('fn f() { match a { _ => { eprintln!("unknown option: {x}"); } } i += 1; }'),
        "an arm that only reports must be flagged",
    )
    check(
        flagged('fn f() { for a in v { match a { b => { eprintln!("unknown option"); } } } run(); }'),
        "a reporting arm with no stop must be flagged",
    )

    # Shapes that are correct and must stay quiet.
    check(
        not flagged('fn f() -> i32 { match a { _ => { eprintln!("unknown option: {x}"); 1 } } }'),
        "a tail expression of 1 IS the exit code",
    )
    check(
        not flagged('fn f() { match a { _ => { eprintln!("unknown option: {x}"); process::exit(1); } } }'),
        "process::exit is a stop",
    )
    check(
        not flagged('fn f() -> i32 { match a { _ => { eprintln!("unknown option: {x}"); return 1; } } }'),
        "return is a stop",
    )
    check(
        not flagged('fn f() -> i32 { match a { o => { if !quiet { eprintln!("unknown option: {o}"); } return 1; } } }'),
        "a return one block out is still a stop (mktemp's shape)",
    )
    check(
        not flagged('fn f() { while i < n { had_error = true; eprintln!("invalid option -- {c}"); j += 1; } }'),
        "recording the failure in a flag is how getopt(1) reports them all",
    )

    # break and continue are NOT stops: both lead to doing the work.
    check(
        flagged('fn f() { loop { match a { _ => { eprintln!("unknown option"); break; } } } do_work(); }'),
        "break out of an argument loop is not a refusal",
    )

    # A diagnostic that is not printed is not a diagnostic.
    check(
        not flagged('fn f() { let msg = "unknown option: {x}"; use_it(msg); }'),
        "a string assigned to a variable is not a printed diagnostic",
    )

    # #[cfg(test)] code is excluded.
    src = 'fn a() {}' + NL + '#[cfg(test)]' + NL + \
          'mod t { fn b() { eprintln!("unknown option"); } }' + NL
    rngs = test_ranges(src)
    m = DIAG.search(src)
    check(
        any(a <= m.start() <= b for a, b in rngs),
        "a diagnostic inside #[cfg(test)] must be excluded",
    )

    check(
        ROOTS == ("userspace", "services", "init", "posix"),
        "the default roots changed to " + repr(ROOTS)
        + "; a wider default gates lanes that cannot fix the findings",
    )

    print("selftest: " + str(checks - bad) + "/" + str(checks) + " cases pass")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument("--roots", nargs="+", metavar="DIR", default=None,
                    help="scan these instead of the default ("
                         + " ".join(ROOTS) + ")")
    ap.add_argument("--selftest", "--self-test", dest="selftest",
                    action="store_true", help="verify the checker itself")
    ap.add_argument("--list", action="store_true",
                    help="print findings and exit 0")
    ap.add_argument("--head", default=None,
                    help="judge this commit instead of the working tree. The "
                         "push hook passes the commit being published, so a "
                         "defect introduced by a commit cannot be hidden by a "
                         "tidied worktree -- nor an uncommitted edit block a "
                         "push of unrelated clean commits.")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    roots = tuple(args.roots) if args.roots else ROOTS
    try:
        tree = gittree.open_tree(ROOT, args.head)
    except gittree.GitTreeError as exc:
        # Exit 2, not 1: `run-checker.sh` reads 1 as "the checker found
        # something" and prints the gate's refusal over it. A revision that
        # cannot be opened is not a finding against anyone's code.
        print("check-option-stops: cannot read " + repr(args.head) + ": "
              + str(exc), file=sys.stderr)
        return 2
    with tree:
        findings, files, diagnostics = scan(tree, roots)

        # Two corpus guards. A scan that read nothing, or read files but found no
        # diagnostic of this shape at all, is not a pass -- it is a checker that
        # has lost its subject, and a clean verdict by accident is the one outcome
        # a gate must never produce.
        if files == 0:
            print("check-option-stops: no .rs file under "
                  + "/, ".join(roots) + "/ -- nothing to judge.", file=sys.stderr)
            return 2
        if diagnostics == 0:
            print("check-option-stops: " + str(files) + " file(s) held no "
                  "unknown-option diagnostic at all -- the vocabulary has drifted "
                  "or this is the wrong tree.", file=sys.stderr)
            return 2

        baseline = read_baseline()
        new = [f for f in findings
               if (f[0] + ":" + str(f[1])) not in baseline and f[0] not in baseline]

        for rel, line, text in sorted(findings):
            known = (rel + ":" + str(line)) in baseline or rel in baseline
            print(("    " if known else "NEW ") + rel + ":" + str(line) + "  " + text)

        print(str(len(findings)) + " diagnostic(s) with no stop out of "
              + str(diagnostics) + ", in " + str(files) + " file(s); "
              + str(len(new)) + " not in the baseline.")

        if args.list:
            return 0
        if new:
            print("", file=sys.stderr)
            print("A program that names an option it does not understand and then "
                  "acts on the command line anyway is worse than one that ignores "
                  "the option: it has already told the caller it did not "
                  "understand.", file=sys.stderr)
            print("Add a `process::exit(1)` or `return 1` to the arm. If the arm "
                  "is a deliberate end-of-options marker -- as in `cgexec`, where "
                  "everything after the options is the command to run -- add the "
                  "line to scripts/option-stops-baseline.txt with the reason.",
                  file=sys.stderr)
            return 1
        return 0


if __name__ == "__main__":
    sys.exit(main())
