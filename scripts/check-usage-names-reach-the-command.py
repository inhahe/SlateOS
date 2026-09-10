#!/usr/bin/env python3
"""Refuse a kshell usage line that names a word running a different command.

Why this exists
===============

`cmd_bluetooth` printed nine usage lines reading

    Usage: bt pair <address> <name> [type]

and `bt` is a real command. It runs `cmd_backtrace`. So the Bluetooth command told
the operator to type a word that prints a stack trace, and the operator who did as
they were told got a plausible-looking response from an unrelated subsystem. That
is worse than naming a command which does not exist, because a nonexistent name
produces an error and a wrong name produces output.

Eight more commands did the same. `cmd_dns`, reachable only as `nslookup`, printed
`Usage: dns ...` while `dns` ran `cmd_dnssettings`. `cmd_type` printed
`Usage: type ...` while `type` was an alias of `cat` -- so the command that reports
what a name means was reachable only as `which` or `typeof`, and `type ls`
concatenated a file.

The rule, and why it is a gate rather than a note
=================================================

**Every word a command's own help prints after `Usage: ` must be a word that
dispatches to that command.**

That is decidable from the tree with no judgment: one side is the printed string,
the other is the `"word" => cmd_foo(args)` arm, and both are in
`kernel/src/kshell.rs`. Nothing here has an opinion about which name *ought* to be
canonical -- only that the help and the dispatch table agree about it.

It is also invisible to every other gate in this tree. The help text is correct
English, the command it names exists, the dispatch table is well-formed, and the
code compiles. Nothing is wrong except the relationship between two files' worth of
strings, which is exactly the class a human reader skims past: `Usage: bt` next to
`fn cmd_bluetooth` reads like an abbreviation, not like a different program.

What is NOT reported, and why
=============================

* **A word that reaches no command at all.** `xargs` prints `Usage: xargs cmd ...`
  where `cmd` is a placeholder for the operator's own command, and `unset`,
  `unalias` and `local` are shell builtins dispatched outside the command table.
  A rule covering these would need to tell a placeholder from a typo, which is
  judgment, and this checker has none. They are a real but separate question.

* **A usage line inside a nested function.** 175 of them sit in `impl` blocks and
  inner `fn`s, and a method named `report` printing `Usage: awk` belongs to `awk`,
  not to whichever top-level command happens to be defined above it. An earlier
  draft attributed by "nearest preceding function at column 0" and reported
  `cmd_cut_input` as naming `tr`, `cmd_sed_input` as naming `awk` and
  `cmd_rev_input` as naming `cut` -- three findings that were entirely an artifact
  of not seeing indented definitions. Measured across four drafts this population
  read 40, then 44, then 12, then 9.

* **A helper printing its parent's name.** `base64_help` prints `Usage: base64`
  and is right to: it is not itself dispatched. Only functions that appear in the
  dispatch table are held to the rule, which removed most of the raw count.

Exit codes: 0 clean, 1 a finding, 2 the tree could not be read.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
from collections import defaultdict

REPO = pathlib.Path(__file__).resolve().parent.parent
KSHELL = REPO / "kernel" / "src" / "kshell.rs"
Q = chr(34)

# `fn name(` at ANY indent, keeping the indent: a usage line inside an indented
# definition belongs to that definition, not to the command above it.
FN = re.compile(r"^([ \t]*)(?:pub )?(?:unsafe )?fn ([a-z_][a-z0-9_]*)", re.M)

# `"word" => cmd_foo(args),` including alias chains. The alternatives accept any
# character but a quote, so a hyphenated alias like "disc-mode" cannot be missed --
# a pattern of [a-z0-9_]+ silently drops those, and a dropped alias here turns a
# correct usage line into a reported finding.
DISPATCH = re.compile(
    r"^\s*((?:" + Q + r"[^" + Q + r"]+" + Q + r"\s*\|\s*)*"
    + Q + r"[^" + Q + r"]+" + Q + r")\s*=> (cmd_[a-z0-9_]+)[\(;]",
    re.M,
)
WORD = re.compile(Q + r"([^" + Q + r"]+)" + Q)
USAGE = re.compile(r"shell_println!\(\s*" + Q + r"Usage: ([a-z0-9_-]+)[ " + Q + r"]")


def dispatch_table(text: str) -> dict[str, set[str]]:
    """Command function -> every word that reaches it."""
    out: dict[str, set[str]] = defaultdict(set)
    for m in DISPATCH.finditer(text):
        for w in WORD.findall(m.group(1)):
            out[m.group(2)].add(w)
    return dict(out)


def enclosing(text: str) -> list[tuple[int, str, int]]:
    return [(m.start(), m.group(2), len(m.group(1))) for m in FN.finditer(text)]


def findings(text: str) -> list[tuple[int, str, str, str]]:
    """(line, host command, printed word, command the word really runs)."""
    fns = enclosing(text)
    words = dispatch_table(text)
    owner = {w: fn for fn, ws in words.items() for w in ws}
    out: list[tuple[int, str, str, str]] = []
    for m in USAGE.finditer(text):
        host, indent = None, None
        for off, name, ind in fns:
            if off > m.start():
                break
            host, indent = name, ind
        if indent != 0 or host not in words:
            continue                      # nested fn, or a helper that is not dispatched
        printed = m.group(1)
        real = owner.get(printed)
        if real is None or real == host:
            continue                      # reaches nothing (placeholder/builtin), or itself
        out.append((text.count(chr(10), 0, m.start()) + 1, host, printed, real))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="usage lines must name a reachable command")
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("--list", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return self_test()

    if not KSHELL.is_file():
        print(
            f"check-usage-names: no {KSHELL.name}; nothing to check. That is not a "
            "clean verdict, it is no verdict.",
            file=sys.stderr,
        )
        return 2

    text = KSHELL.read_text(encoding="utf-8", errors="replace")
    found = findings(text)

    if args.list:
        for line, host, printed, real in found:
            print(f"kernel/src/kshell.rs:{line}: {host} prints {printed!r}, which runs {real}")
        print(f"\n{len(found)} misdirecting usage line(s)")
        return 0

    if not found:
        words = dispatch_table(text)
        total = sum(1 for _ in USAGE.finditer(text))
        print(
            f"check-usage-names: OK ({total} usage line(s) across "
            f"{len(words)} dispatched command(s); every printed name reaches its own command)"
        )
        return 0

    by_host: dict[str, list[tuple[int, str, str]]] = defaultdict(list)
    for line, host, printed, real in found:
        by_host[host].append((line, printed, real))

    print("A usage line names a word that runs a DIFFERENT command:")
    for host in sorted(by_host):
        hits = by_host[host]
        line, printed, real = hits[0]
        extra = f" (and {len(hits) - 1} more line(s))" if len(hits) > 1 else ""
        print(f"  kernel/src/kshell.rs:{line}: {host} prints {printed!r} -> runs {real}{extra}")
    print()
    print("The operator who types what they were told gets a different program, and")
    print("it answers -- which is worse than a name that does not exist, because a")
    print("nonexistent name produces an error and a wrong one produces output.")
    print()
    print("Fix the HELP to print a word that reaches this command, or fix the DISPATCH")
    print("so the printed word arrives here. `type` was the second kind: it printed")
    print("`Usage: type` while `type` was an alias of `cat`.")
    return 1


def self_test() -> int:
    failures = 0
    ran = 0

    def check(label: str, got: object, want: object) -> None:
        nonlocal failures, ran
        ran += 1
        if got == want:
            print(f"  ok   {label}")
        else:
            print(f"  FAIL {label} -- got {got!r}, want {want!r}")
            failures += 1

    NL = chr(10)

    def tree(dispatch: list[str], body: str) -> str:
        return NL.join(
            ["fn dispatch(cmd: &str, args: &str) {", "    match cmd {"]
            + ["        " + d for d in dispatch]
            + ["    }", "}", "", body]
        )

    # The defect: bluetooth's help names a word that runs the backtrace command.
    t = tree(
        ['"bluetooth" => cmd_bluetooth(args),', '"bt" | "backtrace" => cmd_backtrace(),'],
        NL.join([
            "fn cmd_bluetooth(args: &str) {",
            '    shell_println!("Usage: bt pair <address>");',
            "}",
        ]),
    )
    check(
        "a usage line naming another command is reported",
        [(h, p, r) for _l, h, p, r in findings(t)],
        [("cmd_bluetooth", "bt", "cmd_backtrace")],
    )

    # Corrected, it is silent.
    check(
        "naming its own word is clean",
        findings(t.replace("Usage: bt pair", "Usage: bluetooth pair")),
        [],
    )

    # An alias is its own word too, so printing any of them is fine.
    t = tree(
        ['"netsyslog" | "rsyslog" => cmd_netsyslog(args),'],
        NL.join(["fn cmd_netsyslog(args: &str) {",
                 '    shell_println!("Usage: rsyslog add <host>");', "}"]),
    )
    check("printing one of its own aliases is clean", findings(t), [])

    # A HYPHENATED alias must be seen, or a correct line is reported as a finding.
    t = tree(
        ['"disc-mode" | "dm" => cmd_discmode(args),'],
        NL.join(["fn cmd_discmode(args: &str) {",
                 '    shell_println!("Usage: disc-mode on");', "}"]),
    )
    check("a hyphenated alias is recognised", findings(t), [])

    # A word reaching nothing is a placeholder or a builtin, not this gate's business.
    t = tree(
        ['"xargs" => cmd_xargs(args),'],
        NL.join(["fn cmd_xargs(args: &str) {",
                 '    shell_println!("Usage: cmd arg...");', "}"]),
    )
    check("a word reaching no command is not reported", findings(t), [])

    # A helper that is not dispatched may print its parent's name.
    t = tree(
        ['"base64" => cmd_base64(args),'],
        NL.join(["fn base64_help() {",
                 '    shell_println!("Usage: base64 -d");', "}"]),
    )
    check("an undispatched helper may name its parent", findings(t), [])

    # The regression that produced three phantom findings: a usage line inside an
    # INDENTED definition belongs to that definition, not to the command above it.
    t = tree(
        ['"cut" => cmd_cut_input(args),', '"tr" => cmd_tr(args),'],
        NL.join([
            "fn cmd_cut_input(args: &str) {",
            "    let _ = args;",
            "}",
            "",
            "impl Report for Thing {",
            "    fn report(&self) {",
            '        shell_println!("Usage: tr a b");',
            "    }",
            "}",
        ]),
    )
    check("a usage line in a nested fn is not charged to a command", findings(t), [])

    # And the line number is the usage line's, so the finding is actionable.
    t = tree(
        ['"bluetooth" => cmd_bluetooth(args),', '"bt" => cmd_backtrace(),'],
        NL.join(["fn cmd_bluetooth(args: &str) {",
                 '    shell_println!("Usage: bt pair <a>");', "}"]),
    )
    got = findings(t)
    check("the reported line holds the usage call",
          t.splitlines()[got[0][0] - 1].strip().startswith("shell_println!"), True)

    if failures:
        print(f"\nselftest: {failures} case(s) FAILED", file=sys.stderr)
        return 1
    print(f"\ncheck-usage-names --self-test: OK ({ran} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
