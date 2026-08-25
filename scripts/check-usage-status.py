#!/usr/bin/env python3
"""Guard the rule that printing a usage message is *reporting a failure*.

The rule
--------
**If a kshell command prints `Usage: ...` because it could not do what it was
asked, it must also set a non-zero exit status before it returns.**

Why a checker and not just a code review
----------------------------------------
Because this defect has now been fixed twice, and the second fix existed only
because the first one could not see its own blind spot.

`A-KSHELL-A-MISTYPED-COMMAND-REPORTED-SUCCESS` fixed **710** sites in August
2026.  It found them by searching for the shape they had: a `Usage:` print
followed by a bare `return;`.  Every site matching that shape was fixed, the
sweep reported itself complete, and **87 more were left untouched** -- because
they are the whole body of a `match` arm and leave by falling off the end of
it, with no `return` to match on:

    _ => shell_println!("Usage: dynlock autounlock <on|off>"),

That is the general failure of a syntax-keyed sweep: it defines its own blind
spot and cannot report it.  So this checker is keyed on the *semantic*
property instead -- "a `Usage:` print that can be reached and then left without
a non-zero `set_exit`" -- which is the thing actually being guaranteed, and
does not care what shape the next one is written in.

The consequence of a miss is not cosmetic.  The diagnostic is on the screen
either way; it is the status that lies.  A script reads the status:
`cmd && next` runs `next` after a typo, `set -e` does not stop, an `ERR` trap
does not fire.  A wrong answer reported as a right one is worse than no answer,
because the caller has no signal to distrust it.

What is checked
---------------
For each `Usage:`-printing line, walk forward to wherever control leaves the
enclosing block (a `return`, or the brace that closes it) and look for a
`set_exit` with a non-zero argument.  If none is found, the site is reported.

Sites that legitimately print `Usage:` without failing are listed in `ALLOWED`,
keyed by enclosing function plus a distinguishing fragment of the message --
not by line number, which drifts on every edit and would make the allowlist
rot into a rubber stamp.  Adding an entry is meant to require saying why.

Exit status: 0 clean, 1 unaccounted sites found.
"""

import pathlib
import re
import sys

PATH = pathlib.Path(__file__).resolve().parent.parent / "kernel" / "src" / "kshell.rs"

# Sites that print something matching `Usage:` and are *right* not to fail.
# Keyed (function, fragment of the printed text). Each needs a reason.
ALLOWED = {
    # Not usage messages at all -- the word "Usage" happens to appear.
    ("cmd_usagetime", "Usage time subsystem initialised."):
        "a progress message; the word is the subsystem's name, not a synopsis",
    ("cmd_memcg", "  Usage:        {}"):
        "a field label in a memory-cgroup report",

    # Bare invocation printing its own synopsis *is* the command's output, so
    # it succeeded. The `ksyms` precedent, applied consistently.
    ("cmd_ksyms", "Usage: ksyms <address>"):
        "bare `ksyms` prints its synopsis as output; documented exclusion",
    ("cmd_scrollback", "Usage: scrollback [N | search <pattern> | screen]"):
        "same: the `\"\"` arm is bare invocation, which succeeds",

    # A usage line appended to a *successful query* as a hint, not printed as a
    # complaint. Both are guarded on "no argument given", so the branch is only
    # ever reached by `elog echo` / `fc algo` asking what the current setting
    # is -- which the line above answers. Both carried a `set_exit(1)` from the
    # August sweep and so reported failure for a correct answer; see
    # A-KSHELL-A-QUERY-THAT-ANSWERED-CORRECTLY-REPORTED-FAILURE. Listed here
    # rather than left to trip the check, because a future sweep will find them
    # again and this is where the answer needs to be waiting.
    ("cmd_elog", "Usage: elog echo <level>  to change"):
        "a hint on a query's answer -- 'to change' says so; the query succeeded",
    ("cmd_fcompress", "Usage: fc algo <lz4|gzip|zstd|bzip2|xz>"):
        "same: guarded by `parts.len() < 2`, so only the query reaches it",

    # Helpers whose callers set the status. Listed rather than silently
    # skipped, because "the caller does it" is a claim that can stop being
    # true, and an entry here is where someone will look when it does.
    ("base64_help", "Usage: base64 [OPTION]... [FILE]"):
        "invoked for `--help` (exit 0) and by error paths that set their own status",
    ("sed_usage", "Usage: sed [-i] [-n]"):
        "formatter; every caller sets the status around it",
    ("awk_usage", "Usage: awk [-F sep]"):
        "formatter; every caller sets the status around it",
    ("awk_usage", "Usage: cmd | awk [-F sep]"):
        "formatter; every caller sets the status around it",
    ("cmd_cut_input", "Usage: tr SET1 SET2"):
        "tr error formatter; callers set the status",
    ("cmd_sed_input", "Usage: awk [-F sep]"):
        "awk error formatter; callers set the status",
    ("cmd_selftest", "Usage: selftest <category>"):
        "a hint line inside the successful output of `selftest list`",

    # The query halves of the five subcommand branches that used to answer a
    # query and report a typo with one arm and one status. Each now has a
    # sibling branch carrying the diagnostic and the `set_exit(1)`; what is
    # left here is the branch reached only when *no* argument was given, where
    # the line above the usage hint is the answer. Listed individually rather
    # than by function, so that a *new* unfixed arm in any of them still trips.
    ("cmd_fhist", "Usage: fhist autoversion <on|off>"):
        "the `None` arm: a query, answered by the auto-versioning line above",
    ("cmd_wallpaper", "Usage: wallpaper offset <x 0.0-1.0> <y 0.0-1.0>"):
        "the no-argument `else`: a query, answered by the offset line above",
    ("cmd_datausage", "Usage: datausage metered <on|off|roaming>"):
        "the `None` arm: a query, answered by the metered-status line above",
    ("cmd_notifgroup", "Usage: notifgroup mode <app|category|conversation|none>"):
        "the no-argument `else`: a query, answered by the current-mode line above",
    ("cmd_faceunlock", "Usage: faceunlock security <low|standard|high|maximum>"):
        "the no-argument `else`: a query, answered by the current-level line above",

    # `tsession` reaches this arm from `""`, `info` and `status` -- all three
    # are requests for the session summary, which the lines above print. Its
    # unrecognised-subcommand case has its own `_` arm and already fails.
    ("cmd_tsession", "Usage: tsession <new|list|switch|kill|rename>"):
        "the `\"\" | \"info\" | \"status\"` arm is the query; the `_` arm fails separately",

    # These two catch-all help arms *are* resolved -- they end in
    # `end_help_arm` like the other 23 -- but the checker cannot see it. Their
    # help text lives in a nested `#[inline(never)] fn case()` (a stack-frame
    # workaround for a very long arm), so the forward walk from the usage line
    # leaves the *helper's* body long before reaching the call in the arm.
    # Same situation as `sed_usage`/`awk_usage` above: a formatter whose only
    # caller sets the status around it.
    ("cmd_netsettings", "Usage: netsettings <subcommand>"):
        "help text in a nested `fn case()`; its only caller ends in `end_help_arm`",
    ("cmd_tasksched", "Usage: tasksched|schtask <subcommand>"):
        "help text in a nested `fn case()`; its only caller ends in `end_help_arm`",
}

# Arms reached BOTH by an explicit help/query request (a success) and by an
# unrecognised subcommand (a failure), so no single status is right for them.
# Such an arm cannot be *fixed* by adding a status -- whichever one it sets,
# one of its two callers is told something false -- it has to be split first.
#
# **This is empty, and that is the point.** It held 33 entries, which were the
# residue of the sweep that fixed the 87 fall-out-of-a-match arms: that sweep
# could give an arm a status but could not decide which status an arm serving
# two callers deserved. All 33 have since been split -- 23 catch-all help arms
# now end in `end_help_arm`, which sets the status only for a subcommand that
# was not a request for help, and the rest gained a sibling branch so that the
# query and the typo answer separately. See known-issues.md under
# A-KSHELL-A-HELP-ARM-AND-A-TYPO-REPORTED-THE-SAME-THING.
#
# Kept in place rather than deleted, because the shape recurs: the next command
# with a `_ => { help }` arm will arrive, and this is where it goes if it
# cannot be split immediately. Kept separate from ALLOWED so the two are never
# confused -- ALLOWED is correct, this is a debt with a name attached.
#
# The count matters, which is why this is a mapping and not a set. A bare set
# of function names would exempt the *function*, so a genuinely new unfixed
# usage arm added to `cmd_bluetooth` tomorrow would be swallowed by an entry
# that was meant to cover a different arm entirely -- an allowlist that grows
# holes on its own. Pinning the number means the debt can only shrink: fix one
# and the count must come down with it, add one and the check trips.
KNOWN_CONFLATED = {}

USAGE = re.compile(r'(?:console_println!|shell_println!)\s*\(\s*"\s*[Uu]sage\b')
FN = re.compile(r"(?:pub )?(?:async )?fn ([a-z_0-9]+)")


def strip_strings(s):
    """Drop string literals so braces inside them do not count as structure."""
    return "".join(s.split('"')[::2])


def main(argv):
    # An explicit path is how this checker gets tested: run it against an older
    # revision of kshell.rs (`git show <rev>:kernel/src/kshell.rs`) and it must
    # report the sites that revision had. A checker nobody has watched fail is
    # a checker nobody knows works.
    path = pathlib.Path(argv[1]) if len(argv) > 1 else PATH
    lines = path.read_text(encoding="utf-8", errors="surrogateescape").split("\n")

    starts = [(i, m.group(1)) for i, ln in enumerate(lines) if (m := FN.match(ln))]

    def fn_of(i):
        name = "?"
        for s, n in starts:
            if s <= i:
                name = n
            else:
                break
        return name

    unaccounted = []
    seen_conflated = {}
    for i, ln in enumerate(lines):
        if not USAGE.search(ln):
            continue

        # Walk to wherever control leaves this block, looking for a failure status.
        depth = 0
        raised = False
        for k in range(i, min(i + 300, len(lines))):
            s = lines[k]
            if "set_exit(" in s and "set_exit(0)" not in s:
                raised = True
                break
            # `end_help_arm(cmd, sub)` is the resolution of a catch-all help
            # arm, and it sets the status *conditionally* -- non-zero for an
            # unrecognised subcommand, zero for `help`, which is the whole
            # point. It counts as accounting for the site: the arm no longer
            # answers both callers the same way. Matched by name rather than
            # by inlining its body here, because a checker that re-derives
            # what a helper does will drift away from the helper.
            if "end_help_arm(" in s:
                raised = True
                break
            if k > i and re.search(r"\breturn\b", strip_strings(s)):
                break
            depth += strip_strings(s).count("{") - strip_strings(s).count("}")
            if depth < 0:
                break
        if raised:
            continue

        fn = fn_of(i)
        text = ln.strip()
        if any(fn == f and frag in text for (f, frag) in ALLOWED):
            continue
        if seen_conflated.get(fn, 0) < KNOWN_CONFLATED.get(fn, 0):
            seen_conflated[fn] = seen_conflated.get(fn, 0) + 1
            continue
        unaccounted.append((i + 1, fn, text[:88]))

    # A debt entry that no longer matches anything is itself a defect: it means
    # the site was fixed (so the entry is stale and should go) or renamed away
    # (so the entry is now exempting something it was never meant to).
    stale = [
        (fn, n - seen_conflated.get(fn, 0))
        for fn, n in KNOWN_CONFLATED.items()
        if seen_conflated.get(fn, 0) < n
    ]

    conflated = sum(seen_conflated.values())
    if not unaccounted and not stale:
        print(
            f"[usage-status] kshell.rs: every usage diagnostic sets a failure status "
            f"({len(ALLOWED)} allowed, {conflated} known help/error conflations)"
        )
        return 0

    print("", file=sys.stderr)
    if unaccounted:
        print(
            f"{len(unaccounted)} usage message(s) print a diagnostic and then report "
            f"SUCCESS:", file=sys.stderr
        )
        for ln, fn, text in unaccounted:
            print(f"  {path}:{ln}  {fn}", file=sys.stderr)
            print(f"      {text}", file=sys.stderr)
    if stale:
        print("", file=sys.stderr)
        print(
            "KNOWN_CONFLATED entries that matched fewer sites than they claim "
            "(fixed? renamed?) -- lower or remove the count:", file=sys.stderr
        )
        for fn, missing in stale:
            print(f"  {fn}: {missing} fewer than expected", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
