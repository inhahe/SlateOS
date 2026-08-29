#!/usr/bin/env python3
"""Check that a shell diagnostic names the command it actually came from.

Every operand helper in `kshell.rs` is handed the command's own name so that
the message it prints can say where the complaint came from:

    let Some(pid) = required_num::<u32>(&parts, 1, "epollstat", sub, "pid")

    epollstat pid: 'x' is not a readable pid

That name is a bare string literal sitting next to the code that uses it, and
nothing in the compiler relates it to the function it is written in.  So a
copied block carries the *donor's* name into the recipient, and the result is a
message that is fluent, specific, well-formed and about a different command.

This is not hypothetical.  It is how this gate came to exist.  A burn-down pass
converted eleven functions in one sitting, several via a whole-file
search-and-replace of an identical `let pid = parts.get(1)...unwrap_or(0);`
block; the block was not unique to the function being edited, so seven arms
across `cmd_filelock`, `cmd_netsock`, `cmd_pipestat`, `cmd_schedclass`,
`cmd_taskstats`, `cmd_hdrdisplay` and `cmd_dpiscaling` were converted correctly
in substance while announcing themselves as `epollstat` and `displayarrange`.
Everything downstream stayed green: it compiled, it was `cargo fmt`-clean, and
`check-option-refusal.py` counted the sites as *fixed*, because by its own
measure they were.  The only visible trace was in the text of a message that no
test reads.

That is the shape worth gating.  A wrong operand is caught by the type checker;
a wrong *name for the thing complaining* is caught by nothing, and it is worse
than an unhelpful message, because it sends whoever reads it to the wrong
command's source.  It is the same defect the operand helpers exist to prevent --
a confident, specific, invented answer -- one level up, in the machinery of the
fix itself.

The rule
--------

`kshell.rs` dispatches on the typed command word:

    "webcam" | "cam" => cmd_webcam(args),

so the set of names a function may legitimately call itself is *exactly* the set
of literals in its own dispatch arm.  Aliases are the reason this cannot simply
compare against the `cmd_` suffix: `cmd_webcam` correctly says "cam" and
`cmd_vdesktop` correctly says "vd", because those are the short names the usage
lines use.  Both are in the dispatch arm, so both pass.

Checked call sites are the ones that take a command name as a literal:
`required_num`, `optional_num`, `readable_num`, `readable_hex` and
`end_help_arm`.

It starts at zero -- 523 name-bearing calls across 749 dispatch entries, none
mismatched, once the seven above were corrected -- which is the bar DD 635 sets
for a new gate.  A function with no dispatch arm at all (a helper called from
another `cmd_` function rather than from the table) is reported too, since a
name-bearing call there has no set of legitimate names to be checked against and
was almost certainly copied in.

Exit status: 0 clean, 1 findings, 2 could not read the file.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "kernel" / "src" / "kshell.rs"

# `"a" | "b" | "c" => cmd_x(args),` -- the shell's dispatch table.
DISPATCH = re.compile(
    r'^\s*((?:"[^"]+"\s*\|\s*)*"[^"]+")\s*=>\s*(cmd_[a-z0-9_]+)\('
)
FN = re.compile(r"^fn (cmd_[a-z0-9_]+)\(")

# The operand helpers: the command name is the first string literal, after the
# slice/word argument and (for the indexed forms) the index.
HELPER = re.compile(
    r"\b(?:required_num|optional_num|readable_num|readable_hex)"
    r'::<[^>]*>\(\s*[^,]+,\s*(?:\d+,\s*)?"([^"]+)"'
)
HELP_ARM = re.compile(r'\bend_help_arm\(\s*"([^"]+)"')


def dispatch_table(lines: list[str]) -> dict[str, set[str]]:
    """Map cmd_ function -> the set of words that dispatch to it."""
    table: dict[str, set[str]] = {}
    for line in lines:
        m = DISPATCH.match(line)
        if m:
            table.setdefault(m.group(2), set()).update(
                re.findall(r'"([^"]+)"', m.group(1))
            )
    return table


def named_calls(lines: list[str]) -> list[tuple[int, str | None, str]]:
    """Return [(line_number, enclosing_fn, name_literal)] in file order."""
    out: list[tuple[int, str | None, str]] = []
    fn: str | None = None
    for lineno, line in enumerate(lines, start=1):
        m = FN.match(line)
        if m:
            fn = m.group(1)
        for rx in (HELPER, HELP_ARM):
            hit = rx.search(line)
            if hit:
                out.append((lineno, fn, hit.group(1)))
    return out


def check(text: str) -> list[str]:
    lines = text.splitlines()
    table = dispatch_table(lines)
    calls = named_calls(lines)

    if not calls:
        return [
            "no name-bearing helper calls found at all -- either the operand "
            "helpers were renamed or this gate is looking at the wrong file"
        ]

    findings: list[str] = []
    for lineno, fn, name in calls:
        if fn is None:
            findings.append(
                f"line {lineno}: a call names {name!r} but sits outside any "
                f"`fn cmd_*`, so there is no dispatch arm to check it against"
            )
            continue
        allowed = table.get(fn)
        if not allowed:
            findings.append(
                f"line {lineno}: {fn} names {name!r} but has no dispatch arm, "
                f"so nothing establishes what it may legitimately call itself"
            )
        elif name not in allowed:
            findings.append(
                f"line {lineno}: {fn} prints {name!r} in a diagnostic, but the "
                f"shell dispatches to it as "
                f"{' or '.join(repr(a) for a in sorted(allowed))} -- the "
                f"message sends the reader to another command's source"
            )
    return findings


# Fixtures, for the reason every gate here has them: a clean tree and a checker
# that has stopped working report success in identical words.
_FIXTURE_OK = """
        "webcam" | "cam" => cmd_webcam(args),
        "vdesktop" | "vd" => cmd_vdesktop(args),
fn cmd_webcam(args: &str) {
            let Some(id) = required_num::<u32>(&parts, 1, "cam", sub, "camera id") else {
            end_help_arm("webcam", sub);
fn cmd_vdesktop(args: &str) {
            let Some(n) = optional_num::<u32>(&parts, 1, "vd", sub, "count", 4) else {
"""

# The real defect: a block copied from cmd_epollstat into cmd_filelock, keeping
# the donor's name.  Everything about it compiles and formats correctly.
_FIXTURE_LEAKED = """
        "epollstat" | "epoll" => cmd_epollstat(args),
        "filelock" | "flkstat" => cmd_filelock(args),
fn cmd_epollstat(args: &str) {
            let Some(pid) = required_num::<u32>(&parts, 1, "epollstat", sub, "pid") else {
fn cmd_filelock(args: &str) {
            let Some(pid) = required_num::<u32>(&parts, 1, "epollstat", sub, "pid") else {
"""

# A name that is nobody's: a typo, or a command renamed in the table and not in
# its messages.
_FIXTURE_UNKNOWN = """
        "webcam" | "cam" => cmd_webcam(args),
fn cmd_webcam(args: &str) {
            let Some(id) = required_num::<u32>(&parts, 1, "wecbam", sub, "camera id") else {
"""

# A function the table never reaches, so no arm bounds what it may print.
_FIXTURE_ORPHAN = """
        "webcam" | "cam" => cmd_webcam(args),
fn cmd_webcam(args: &str) {
            let Some(id) = required_num::<u32>(&parts, 1, "cam", sub, "camera id") else {
fn cmd_ghost(args: &str) {
            let Some(n) = required_num::<u32>(&parts, 1, "ghost", sub, "n") else {
"""


def self_test() -> int:
    cases = [
        ("a correct tree, aliases included", _FIXTURE_OK, False),
        ("a name copied in from another command", _FIXTURE_LEAKED, True),
        ("a name that dispatches to nothing", _FIXTURE_UNKNOWN, True),
        ("a function with no dispatch arm", _FIXTURE_ORPHAN, True),
    ]
    bad = 0
    for name, text, should_report in cases:
        got = bool(check(text))
        if got != should_report:
            verb = "reported nothing for" if should_report else "reported"
            print(f"SELF-TEST FAIL: {verb} {name}", file=sys.stderr)
            bad += 1
    if bad:
        print(
            f"\n{bad} fixture case(s) disagree with the checker. Its verdict on "
            f"kshell.rs means nothing until they agree.",
            file=sys.stderr,
        )
        return 1
    print(
        "self-test OK: the message-name gate reports all 3 broken fixtures and "
        "not the clean one"
    )
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()

    try:
        text = TARGET.read_text(encoding="utf-8")
    except OSError as exc:
        print(f"cannot read {TARGET}: {exc}", file=sys.stderr)
        return 2

    findings = check(text)
    if not findings:
        lines = text.splitlines()
        print(
            f"shell message names OK: {len(named_calls(lines))} diagnostic(s) "
            f"name a command, and every one is a word the shell actually "
            f"dispatches to that function ({len(dispatch_table(lines))} "
            f"functions in the table)"
        )
        return 0

    for f in findings:
        print(f"kshell.rs: {f}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
