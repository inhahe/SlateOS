#!/usr/bin/env python3
r"""Refuse a shell command substitution that calls a name nothing defines.

## The bug this exists to catch

`scripts/boot-test.sh`'s `check_libc_shape` gate began, from the day it was
wired (e3e72d4bf, 2026-09-03) until it was repaired on 2026-09-04:

    check_libc_shape() {
        local py=""
        py="$(find_python)" || return 0

`find_python` is not defined. Not in `boot-test.sh`, not in
`scripts/run-checker.sh` which it sources, not on `PATH`, not anywhere in the
tree. So the command substitution exited 127, the `|| return 0` converted that
into a pass, and the gate together with its 24-case self-test had never
executed once, on any host, in its entire existence.

Every mechanism that should have caught it declined:

| Mechanism | Why it did not fire |
|---|---|
| `set -e` | `|| return 0` is an explicit handler; that is what it is *for* |
| `bash -n` | the syntax is valid -- the failure is at run time |
| `shellcheck` | SC2154-style checks are about *variables*, not command names |
| `check-gates-are-wired.py` | it reads text; the `run_checker` call site exists |
| the boot-test log | one unprefixed stderr line, `find_python: command not found`, between two banners in a 60k-line log, carrying no ERROR and not touching the exit status |

The lesson, which is the next term past `design-decisions.md` §907 ("a gate is
what `run_checker` runs, not what it is named"): **a call site that exists is
not a call site that executes.** No amount of reading the *text* of a call can
tell you the callee resolves. That is a question about the union of this file,
its sources, the builtins and `PATH` -- and it is decidable statically, which
is why this gate can ask it.

## Why a command substitution specifically

The general question -- "does every command word in every shell script
resolve?" -- is not decidable: command words come from variables, from `eval`,
from arrays, from `$@`. Scoping to `$(word ...)` buys three things:

1. **The word is unambiguously in command position.** After `$(` the shell is
   parsing a fresh command; there is no other reading of the token.
2. **The failure mode is the dangerous one.** A bare failing command trips
   `set -e`. A failing *substitution* yields the empty string and a status that
   the surrounding `||`, `if`, or `local` swallows -- so the caller proceeds
   with an empty value and calls it success. That is precisely the shape of the
   `find_python` bug.
3. **It is what actually found the bug.** A first attempt at the broad
   command-position rule (`^\s*([a-z_]\w*)\s`) returned 42 findings, all false
   positives, and missed this one -- the real line is `$(find_python)"`, whose
   next character is `)` rather than whitespace. The narrow rule keyed on the
   substitution's opening paren found it immediately. Recorded in
   `known-issues.md` -> A-A-THE-LIBC-SHAPE-GATE-WAS-BORN-DEAD; the miss is kept
   because "the broad scan is the better one" is the intuition that failed.

Backtick substitutions are included: `` `word ...` `` is the same construct.

Arithmetic falls out for free. `$((` opens with a paren, not an identifier, so
the pattern cannot match it -- no special case is needed and none is written.

## What counts as "resolves"

A name resolves if it is any of:

- a function defined in the same file (`name()` or `function name`);
- a function defined in a file the script `source`s (followed transitively,
  best-effort -- see below);
- a shell builtin or reserved word;
- an executable on `PATH`, asked of **bash itself** via one batched
  `command -v`, not of Python's `shutil.which`. The scripts run under MSYS
  bash, whose `PATH` contains `sed`, `awk`, `readlink` and friends that the
  Windows `PATH` this interpreter sees does not. Asking the wrong shell would
  turn every unix tool in the tree into a finding.

Names that cannot be *resolved* are also not *reported* when they cannot be
read as literals at all: `$("$py" foo.py)` starts with a quote, `$($TOOL x)`
with a `$`. Those are variable-driven and outside what any static rule can
decide, so the pattern simply does not match them.

## Following `source`

`source "$PROJECT_ROOT/scripts/run-checker.sh"` cannot be resolved by path
without evaluating the variable. Rather than half-implement a shell, this takes
the *basename* of the argument and looks for tracked files with that name. It
is deliberately loose in the safe direction: a wrong match can only add
function names to the resolved set, so the error it can make is a false
negative (a missed finding), never a false positive (a spurious refusal). A
gate that cries wolf gets disabled; a gate that misses one instance still
caught the others.

## Blast radius: measured, not guessed

Across the tree, this rule finds **one** true defect -- the one above -- and it
is now fixed. That number is the entire argument for the gate's design. A rule
that fires on a tenth of the tree is a rule the next person turns off; a rule
with one finding and no false positives is one that stays on and means
something when it fires.

## Cost

~2.4 MB across the 104 graded files, read once and masked with a single-pass
character scanner: a few seconds, dominated by per-file antivirus interception
like everything else here (`open-questions.md` A-Q7). One `bash` process is
spawned for the whole run, not one per name -- the 96 names that survive the
local/sourced/builtin filter are asked in a single batch.

## `--self-test` grades the gate against real files

Per `known-issues.md` -> `TD-A-A-WIRED-GATE-CAN-GRADE-ONE-LINE-AND-LOOK-LIKE-
IT-GRADES-A-SUBSYSTEM`: a fixture built from strings the author invented proves
the author's imagination is self-consistent.

The part that cannot drift here is the regex. The parts that can are (a) the
quote/comment/heredoc masker, which if it ever over-masks makes this gate
report a clean tree in silence, (b) the function-definition parser, which if it
ever under-parses turns every local helper into a finding, and (c) the batched
`command -v` query, which if its framing breaks resolves nothing or everything.
So the self-test runs all three against real tracked files, and plants its
mutations into real bytes rather than into invented ones. Nothing is written to
disk.

Exit codes:
    0   every literal command-substitution callee resolves
    1   at least one does not (the finding)
    2   could not look: not a git worktree, no bash, or fewer files than the
        floor -- a gate that cannot look declines rather than passing

Usage:
    python scripts/check-shell-callables.py              # grade the tree
    python scripts/check-shell-callables.py --list       # show what resolved how
    python scripts/check-shell-callables.py --self-test  # grade the gate
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
from pathlib import Path

# Measured 2026-09-04: 768 tracked shell files (767 `*.sh` plus
# `scripts/hooks/pre-push`), of which 664 are the excluded oils parser corpus,
# leaving **104** graded. 60 is far below any plausible shrink and far above
# what a broken enumeration returns, which is 0. The floor exists because the
# failure this gate is most likely to suffer is finding nothing and calling it
# clean -- the same failure it exists to catch elsewhere.
DISCOVERY_FLOOR = 60

# The one excluded subtree, and the only one there should ever be.
#
# `userspace/oils/tests/corpus/` is a *shell-parser test corpus*: its files are
# inputs to a parser, not programs anyone runs to get work done. The premise of
# this gate -- "a callee that does not resolve is a mistake" -- is false there,
# because an unresolvable callee is routinely the fixture's entire point.
# `lineno-cmdsub.sh` calls `nosuchcommand_xyz` deliberately, to pin down which
# line number the diagnostic names; `a-backquote-body-inside-double-quotes...sh`
# writes ``echo p`echo $(fi)`q{,}`` to see how the brace scanner reads it.
# Ninety-six findings, every one of them a fixture behaving as specified.
#
# Keep this list at one entry. The moment a `scripts/` path appears here, this
# gate has been silenced rather than satisfied -- which is the failure mode
# every other note in this file is about. The self-test asserts the exclusion
# is exactly this subtree and that it does not reach `scripts/`.
EXCLUDED_PREFIXES = ("userspace/oils/tests/corpus/",)

# Measured 2026-09-04: 1495 literal command substitutions across the 104 graded
# files. This is the floor that actually earns its keep -- the file count can
# look healthy while the *masker* silently eats every body, which is exactly
# what a runaway heredoc did during development (a `<<<` here-string read as a
# `<<` opener swallowed the rest of boot-test.sh, dropping it from 114
# substitutions to 46 with no other symptom). 800 is comfortably under 1495 and
# far above what that bug produced.
CANDIDATE_FLOOR = 800

# bash builtins, reserved words and POSIX special builtins. `command -v` would
# answer for most of these too, but not for reserved words (`if`, `for`,
# `while`), and asking bash about `}` is not meaningful.
SHELL_WORDS = frozenset("""
: . [ alias bg bind break builtin caller cd command compgen complete compopt
continue coproc declare dirs disown echo enable eval exec exit export false fc
fg getopts hash help history jobs kill let local logout mapfile popd printf
pushd pwd read readarray readonly return select set shift shopt source suspend
test times trap true type typeset ulimit umask unalias unset wait
case do done elif else esac fi for function if in then time until while
""".split())

# `$(` or a backtick, optional whitespace, then a bare identifier. The
# identifier charset allows `.` and `-` because real commands have them
# (`docker-compose`, `foo.sh`). A leading `"` or `$` fails to match, which is
# the intent: those callees are variable-driven and undecidable here.
#
# Two refinements, both from false positives this produced on the real tree:
#
#   `$(PYTHONIOENCODING=:replace "$py" -u "$f" 2>&1)` -- a command may be
#       prefixed by any number of one-shot environment assignments. They are
#       not the callee; the callee is what follows, and here it is `"$py"`,
#       which is variable-driven and therefore correctly nothing at all. The
#       `(?:NAME=...\s+)*` group steps over them.
#
#   `$(foo=bar)` -- an assignment with no command at all. Without the trailing
#       guard the pattern would take `foo` as a callee. `(?![\w.\-=])` forbids
#       it; note the `=` must be inside the character class, because a bare
#       `(?!=)` would just backtrack the identifier one character and match.
CALLEE_RE = re.compile(
    r"(?:\$\(|`)\s*"
    r"(?:[A-Za-z_][A-Za-z0-9_]*=[^\s)]*\s+)*"
    r"([A-Za-z_][A-Za-z0-9_.\-]*)(?![A-Za-z0-9_.\-=])")

# `name ()` / `name()` / `function name`. Both forms, since both appear.
FUNCDEF_RE = re.compile(
    r"^\s*(?:function\s+([A-Za-z_][A-Za-z0-9_.\-]*)\s*(?:\(\s*\))?"
    r"|([A-Za-z_][A-Za-z0-9_.\-]*)\s*\(\s*\))\s*\{?",
    re.MULTILINE)

# `source X` / `. X` in command position.
SOURCE_RE = re.compile(r"(?:^|[;&|]|\bthen\b|\bdo\b|\belse\b)\s*"
                       r"(?:source|\.)\s+(\S+)", re.MULTILINE)

# `VAR=value` at the start of a line, unconditional and unindented-or-indented.
# `export VAR=value` and `local VAR=value` count; `VAR+=` does not.
ASSIGN_RE = re.compile(
    r"^[ \t]*(?:export[ \t]+|local[ \t]+|declare[ \t]+(?:-\w+[ \t]+)*)?"
    r"([A-Za-z_][A-Za-z0-9_]*)=([^\n;&|]*)", re.MULTILINE)

# `$VAR` or `${VAR}`. Deliberately does not handle `${VAR:-default}` or any
# other expansion form: a partial match leaves the `$` in place, and a basename
# that still contains a `$` is dropped rather than guessed at.
VARREF_RE = re.compile(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)")

# A heredoc opener: `<<` or `<<-`, then an optionally-quoted delimiter word.
#
# Two lookalikes must not match, and both bit during development:
#
#   `read -r -a A <<< "$X"`   -- a here-*string*. A `(?![<])` guard inside this
#       pattern is not enough: the caller advances one character at a time, so
#       on the second `<` of `<<<` the pattern sees a clean `<< "$X"` and
#       happily takes `"$X"` as a delimiter. It then eats every line up to a
#       line reading `$X`, which is to say the rest of the file. The guard has
#       to be on the *left* -- hence `_heredoc_at`, which refuses to start
#       inside a run of `<`.
#
#   `n=$(( 1 << 3 ))`         -- a shift. The delimiter charset would take `3`
#       and then consume lines until one read exactly `3`. Handled by skipping
#       arithmetic spans wholesale before we ever look for `<<`.
#
# Both failures are silent and one-directional: they blank out real code, so
# the gate finds less and reports a clean tree. That is the failure mode this
# whole file exists to complain about, which is why CANDIDATE_FLOOR is a
# hard floor and the self-test counts what boot-test.sh yields.
HEREDOC_RE = re.compile(r"<<(-?)[ \t]*(\"[^\"]*\"|'[^']*'|[A-Za-z0-9_./\-]+)")


def _decline(headline: str, detail: str) -> int:
    print(f"ERROR: {headline}", file=sys.stderr)
    print(detail, file=sys.stderr)
    return 2


# ---------------------------------------------------------------------------
# masking
# ---------------------------------------------------------------------------

def _unquoted_body(line: str) -> str:
    """Mask one line of an *unquoted* heredoc body.

    Inside `<<EOF` the shell still expands `$(...)` and backquotes, so the body
    cannot simply be copied through -- but quotes are inert there, so the full
    scanner would be wrong too. The only masking that applies is backslash
    escapes, and it is the one that matters: `pre-push`'s advisory heredocs are
    English prose full of \\`static FOO: Mutex<()>\\` and
    \\`unwrap_or_else(...)\\`, every one of which reads as a backquote
    substitution if the escape survives. That was 166 findings, all prose.

    Length is preserved so line numbers and offsets stay true.
    """
    out: list[str] = []
    j = 0
    ln = len(line)
    while j < ln:
        if line[j] == "\\" and j + 1 < ln:
            out.append("  ")
            j += 2
            continue
        out.append(line[j])
        j += 1
    return "".join(out)


def mask(text: str) -> str:
    """Blank out everything a command substitution cannot live in.

    Single-quoted spans, `#` comments and quoted-delimiter heredoc bodies are
    replaced with spaces, preserving every newline so line numbers survive.
    Double-quoted spans are *kept*: `"$(date)"` is a live substitution, and
    masking it would hide the majority of real call sites.

    Unquoted heredocs (`<<EOF`, no quotes) are kept for the same reason -- the
    shell really does expand `$(...)` inside them. Quoted ones (`<<'PY'`) are
    inert by definition and are almost always another language's source, which
    is the "skip heredocs whose body is another language" rule in its
    load-bearing form: it is not a guess about the body, it is the shell's own
    quoting telling us the body is not shell.
    """
    lines = text.split("\n")
    out: list[str] = []
    sq = False          # inside a single-quoted span (persists across lines)
    dq = False          # inside a double-quoted span (persists across lines)
    i = 0
    n = len(lines)
    while i < n:
        raw = lines[i]
        buf: list[str] = []
        pending: list[tuple[str, bool, bool]] = []   # (delim, quoted, strip)
        j = 0
        ln = len(raw)
        while j < ln:
            c = raw[j]
            if sq:
                if c == "'":
                    sq = False
                buf.append(" ")
                j += 1
                continue
            if dq:
                if c == "\\" and j + 1 < ln:
                    # Blank both, never copy through. An escaped character is
                    # literal text, so it is not syntax and must not be left
                    # where the pattern can read it. Copying it through was a
                    # real bug: this tree's refusal messages are full of
                    # `echo "... \`article_for\` picks by spelling ..."`, and a
                    # preserved backslash-backtick made every one of them look
                    # like a backquote substitution calling `article_for`,
                    # `picks`, `text`, `Mutex`, `thread_local`... 193 findings,
                    # every one of them prose.
                    buf.append("  ")
                    j += 2
                    continue
                if c == '"':
                    dq = False
                buf.append(c)
                j += 1
                continue
            # unquoted
            if c == "\\" and j + 1 < ln:
                buf.append("  ")   # literal, therefore not syntax -- see above
                j += 2
                continue
            if c == "'":
                sq = True
                buf.append(" ")
                j += 1
                continue
            if c == '"':
                dq = True
                buf.append(c)
                j += 1
                continue
            if c == "#" and (j == 0 or raw[j - 1] in " \t;&|(<"):
                buf.append(" " * (ln - j))
                j = ln
                continue
            if c == "$" and raw.startswith("$((", j):
                # Skip the whole arithmetic span so a `<<` shift inside it is
                # never read as a heredoc opener. Copied through verbatim:
                # CALLEE_RE cannot match `$((` anyway, since `(` is not an
                # identifier character.
                depth = 0
                k = j + 1
                while k < ln:
                    if raw[k] == "(":
                        depth += 1
                    elif raw[k] == ")":
                        depth -= 1
                        if depth == 0:
                            k += 1
                            break
                    k += 1
                buf.append(raw[j:k])
                j = k
                continue
            if (c == "<" and raw.startswith("<<", j)
                    and not raw.startswith("<<<", j)
                    and not (j > 0 and raw[j - 1] == "<")):
                m = HEREDOC_RE.match(raw, j)
                if m:
                    word = m.group(2)
                    quoted = word[0] in "\"'"
                    delim = word.strip("\"'")
                    pending.append((delim, quoted, m.group(1) == "-"))
                    buf.append(raw[j:m.end()])
                    j = m.end()
                    continue
            buf.append(c)
            j += 1
        out.append("".join(buf))
        i += 1

        # Consume this line's heredoc bodies.
        for delim, quoted, strip in pending:
            while i < n:
                body = lines[i]
                probe = body.lstrip("\t") if strip else body
                if probe.rstrip("\r") == delim:
                    out.append(body)
                    i += 1
                    break
                out.append(" " * len(body) if quoted else _unquoted_body(body))
                i += 1
        # A heredoc body cannot leave us inside a quote.
        if pending:
            sq = dq = False
    return "\n".join(out)


# ---------------------------------------------------------------------------
# corpus
# ---------------------------------------------------------------------------

def tracked_shell_files() -> list[Path]:
    """Every tracked `*.sh`, plus extensionless files with a sh/bash shebang."""
    try:
        out = subprocess.run(["git", "ls-files", "-z"],
                             capture_output=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError):
        return []
    paths = [Path(f.decode("utf-8", "surrogateescape"))
             for f in out.split(b"\0") if f]
    found: list[Path] = []
    for p in paths:
        if p.as_posix().startswith(EXCLUDED_PREFIXES):
            continue
        if p.suffix == ".sh":
            found.append(p)
            continue
        if p.suffix:
            continue
        try:
            with p.open("rb") as fh:
                first = fh.readline(200)
        except OSError:
            continue
        if first.startswith(b"#!") and (b"bash" in first or first.rstrip().endswith(b"/sh")):
            found.append(p)
    return found


def read_text(p: Path) -> str:
    try:
        return p.read_bytes().decode("utf-8", "replace")
    except OSError:
        return ""


def functions_in(text: str) -> set[str]:
    names: set[str] = set()
    for m in FUNCDEF_RE.finditer(text):
        names.add(m.group(1) or m.group(2))
    return names


def assignments_in(text: str) -> dict[str, str]:
    """Literal `VAR=value` assignments, for expanding a sourced path.

    Only top-level, unconditional, single-line assignments. This is not a shell
    evaluator and must not become one; it exists solely so that the *basename*
    of a sourced path can be recovered.
    """
    out: dict[str, str] = {}
    for m in ASSIGN_RE.finditer(text):
        name, value = m.group(1), m.group(2).strip()
        if value[:1] in "\"'" and value[-1:] == value[:1] and len(value) > 1:
            value = value[1:-1]
        out.setdefault(name, value)
    return out


def expand(value: str, assigns: dict[str, str], depth: int = 6) -> str:
    """Best-effort `$VAR` / `${VAR}` expansion against `assigns`."""
    for _ in range(depth):
        if "$" not in value:
            break
        new = VARREF_RE.sub(
            lambda m: assigns.get(m.group(1) or m.group(2), m.group(0)), value)
        if new == value:
            break
        value = new
    return value


def sourced_basenames(text: str) -> set[str]:
    """Basenames of the files this text `source`s.

    Deliberately basename-only, and deliberately loose. 134 of the tree's 312
    `source` lines name their target through a variable (`. "$BOOT_CHECKER_LIB"`
    is how boot-test.sh pulls in run-checker.sh), so refusing to expand would
    leave `run_checker` -- one of the most-called functions in the tree --
    unresolvable, and every one of its call sites a false positive. That is the
    shape of gate that gets switched off.

    Expansion can only *add* names to a file's resolved set, so its errors are
    false negatives, never false refusals.
    """
    assigns = assignments_in(text)
    out: set[str] = set()
    for m in SOURCE_RE.finditer(text):
        arg = expand(m.group(1).strip("\"'"), assigns)
        base = arg.replace("\\", "/").rsplit("/", 1)[-1]
        if base and "$" not in base:
            out.add(base)
    return out


def bash_resolves(names: set[str], bash: str) -> set[str]:
    """Ask bash which of `names` it can find. One process for the whole set.

    Uses `command -v --` so a name that looks like an option cannot be read as
    one, and prints a stable `Y `/`N ` prefix rather than relying on the exit
    status of a loop.
    """
    if not names:
        return set()
    script = ('for n in "$@"; do '
              'if command -v -- "$n" >/dev/null 2>&1; '
              'then printf "Y %s\\n" "$n"; else printf "N %s\\n" "$n"; fi; done')
    ordered = sorted(names)
    try:
        res = subprocess.run([bash, "-c", script, "_", *ordered],
                             capture_output=True, timeout=300)
    except (OSError, subprocess.SubprocessError):
        return set()
    ok: set[str] = set()
    for line in res.stdout.decode("utf-8", "replace").splitlines():
        if line.startswith("Y "):
            ok.add(line[2:].strip())
    return ok


# ---------------------------------------------------------------------------
# the check
# ---------------------------------------------------------------------------

def candidates(files: list[Path]) -> tuple[dict[Path, list[tuple[int, str]]], dict[Path, str]]:
    """Return per-file (lineno, callee) hits and the masked text of each file."""
    hits: dict[Path, list[tuple[int, str]]] = {}
    masked_by: dict[Path, str] = {}
    for p in files:
        text = read_text(p)
        if not text:
            continue
        masked = mask(text)
        masked_by[p] = masked
        found: list[tuple[int, str]] = []
        for m in CALLEE_RE.finditer(masked):
            line = masked.count("\n", 0, m.start()) + 1
            found.append((line, m.group(1)))
        if found:
            hits[p] = found
    return hits, masked_by


def resolve_sets(files: list[Path], masked_by: dict[Path, str]) -> dict[Path, set[str]]:
    """Per-file set of function names visible to that file (own + sourced)."""
    by_basename: dict[str, list[Path]] = {}
    for p in files:
        by_basename.setdefault(p.name, []).append(p)

    own: dict[Path, set[str]] = {p: functions_in(t) for p, t in masked_by.items()}
    srcs: dict[Path, set[str]] = {p: sourced_basenames(t) for p, t in masked_by.items()}

    visible: dict[Path, set[str]] = {}
    for p in masked_by:
        seen: set[str] = set(own.get(p, ()))
        # Follow sources transitively, bounded by the number of files.
        frontier = set(srcs.get(p, ()))
        walked: set[str] = set()
        while frontier:
            base = frontier.pop()
            if base in walked:
                continue
            walked.add(base)
            for q in by_basename.get(base, ()):
                seen |= own.get(q, set())
                frontier |= srcs.get(q, set()) - walked
        visible[p] = seen
    return visible


def check(files: list[Path], bash: str, show_list: bool) -> int:
    hits, masked_by = candidates(files)
    total = sum(len(v) for v in hits.values())
    if total < CANDIDATE_FLOOR:
        return _decline(
            f"cannot check shell callees: only {total} command substitution(s) "
            f"found across {len(files)} file(s), floor is {CANDIDATE_FLOOR}",
            "A number this low means the masker swallowed the file bodies, not\n"
            "that the tree stopped calling things. A gate that looks at nothing\n"
            "and reports nothing is indistinguishable from a clean tree, which\n"
            "is the exact failure this gate exists to catch, so it declines.")

    visible = resolve_sets(files, masked_by)

    # Everything not explained by a local/sourced function or a shell word gets
    # asked of bash -- once, in a batch.
    unexplained: set[str] = set()
    for p, found in hits.items():
        for _, name in found:
            if name in SHELL_WORDS or name in visible.get(p, ()):
                continue
            unexplained.add(name)
    on_path = bash_resolves(unexplained, bash)

    findings: list[tuple[Path, int, str]] = []
    for p, found in sorted(hits.items()):
        for line, name in found:
            if name in SHELL_WORDS or name in visible.get(p, ()) or name in on_path:
                continue
            findings.append((p, line, name))

    print(f"shell callees: {total} literal command substitution(s) in "
          f"{len(hits)} of {len(files)} file(s); "
          f"{len(unexplained)} name(s) asked of bash, {len(on_path)} on PATH")

    if show_list:
        for name in sorted(unexplained):
            print(f"  {'PATH ' if name in on_path else 'MISS '}{name}")

    if not findings:
        print("shell callees OK: every literal command-substitution callee resolves")
        return 0

    for p, line, name in findings:
        print(f"{p.as_posix()}:{line}: calls `{name}`, which is not defined in "
              f"this file, in anything it sources, as a shell builtin, or on PATH",
              file=sys.stderr)
    print(REFUSAL, file=sys.stderr)
    return 1


# ---------------------------------------------------------------------------
# self-test
# ---------------------------------------------------------------------------

def self_test() -> int:
    files = tracked_shell_files()
    bash = shutil.which("bash")
    if not files or bash is None:
        print("FAIL self-test cannot run: no tracked shell files, or no bash",
              file=sys.stderr)
        return 1

    boot = next((p for p in files if p.as_posix().endswith("scripts/boot-test.sh")), None)
    if boot is None:
        print("FAIL self-test cannot run: scripts/boot-test.sh not tracked",
              file=sys.stderr)
        return 1

    real = read_text(boot)
    masked = mask(real)
    hits, masked_by = candidates(files)
    visible = resolve_sets(files, masked_by)
    boot_names = [n for _, n in hits.get(boot, ())]

    # Mutations planted into *real* bytes, not invented ones.
    nonce = "zz_no_such_callee_9f3"
    planted = mask(real + f'\nx="$({nonce})"\n')
    in_sq = mask(real + f"\ny='$({nonce})'\n")
    in_cmt = mask(real + f'\n# x="$({nonce})"\n')
    in_dq = mask(real + f'\nz="outer $({nonce}) tail"\n')
    in_hd = mask(real + f"\ncat <<'PY'\n$({nonce})\nPY\n")
    arith = mask(real + '\nn=$(( 1 + 2 ))\n')

    def callees(t: str) -> list[str]:
        return [m.group(1) for m in CALLEE_RE.finditer(t)]

    probe = bash_resolves({"echo", "zz_definitely_not_on_path_7c1"}, bash)

    cases = [
        # -- the corpus is real and large
        ("the tracked shell corpus clears the discovery floor",
         len(files) >= DISCOVERY_FLOOR, True),
        ("boot-test.sh is in the corpus", boot in masked_by, True),
        ("the corpus clears the candidate floor",
         sum(len(v) for v in hits.values()) >= CANDIDATE_FLOOR, True),

        # -- the masker still sees into real files
        ("boot-test.sh yields real command substitutions", len(boot_names) > 50, True),
        ("masking preserves line count",
         masked.count("\n"), real.count("\n")),
        ("masking preserves length", len(masked), len(real)),

        # -- the definition parser still finds real functions
        ("check_libc_shape is parsed as a function of boot-test.sh",
         "check_libc_shape" in functions_in(masked), True),
        ("run_checker is visible to boot-test.sh via its source of run-checker.sh",
         "run_checker" in visible.get(boot, set()), True),
        ("run_checker is NOT defined in boot-test.sh itself",
         "run_checker" in functions_in(masked), False),

        # -- the finding, planted in real bytes
        ("a bare undefined callee is seen", nonce in callees(planted), True),
        ("the same callee inside double quotes is seen too",
         nonce in callees(in_dq), True),
        ("the same callee inside single quotes is masked away",
         nonce in callees(in_sq), False),
        ("the same callee inside a comment is masked away",
         nonce in callees(in_cmt), False),
        ("the same callee inside a quoted heredoc is masked away",
         nonce in callees(in_hd), False),
        ("arithmetic expansion yields no callee",
         len(callees(arith)) - len(callees(masked)), 0),

        # -- the batched PATH query still works in both directions
        ("bash resolves a name that exists", "echo" in probe, True),
        ("bash does not resolve a name that does not",
         "zz_definitely_not_on_path_7c1" in probe, False),

        # -- and the regression itself: the repaired gate no longer calls it
        ("find_python is no longer called anywhere",
         any("find_python" == n for v in hits.values() for _, n in v), False),

        # -- the exclusion is exactly one subtree and does not reach scripts/
        ("exactly one subtree is excluded", len(EXCLUDED_PREFIXES), 1),
        ("nothing under scripts/ is excluded",
         any(p.startswith("scripts/") for p in EXCLUDED_PREFIXES), False),
        ("the excluded subtree is really excluded",
         any(p.as_posix().startswith(EXCLUDED_PREFIXES) for p in files), False),
        ("the excluded subtree still exists (the exclusion is not a no-op that "
         "would silently widen if lane B moved it)",
         Path("userspace/oils/tests/corpus").is_dir(), True),

        # -- the two shapes that produced false positives on the real tree
        ("an env-assignment prefix is stepped over, not taken as the callee",
         [m.group(1) for m in CALLEE_RE.finditer('x="$(FOO=1 BAR=2 realcmd -u)"')],
         ["realcmd"]),
        ("an assignment with no command yields no callee",
         [m.group(1) for m in CALLEE_RE.finditer("x=$(foo=bar)")], []),
        ("an escaped backquote in a double-quoted string is not a substitution",
         [m.group(1) for m in
          CALLEE_RE.finditer(mask(r'echo "see \`article_for\` above"'))], []),
        ("an escaped backquote in an unquoted heredoc body is not one either",
         [m.group(1) for m in
          CALLEE_RE.finditer(mask('cat <<EOF\nsee \\`thread_local\\` above\nEOF\n'))], []),
        ("a here-string is not read as a heredoc",
         "QEMU_ARGS" in mask('read -r -a A <<< "$X"\nQEMU_ARGS=1\n'), True),
        ("a shift inside arithmetic is not read as a heredoc",
         "KEEPME" in mask("n=$(( 1 << 3 ))\nKEEPME=1\n"), True),
    ]

    failed = 0
    for name, got, want in cases:
        if got == want:
            print(f"ok   {name}")
        else:
            print(f"FAIL {name} -- got {got!r}, want {want!r}")
            failed += 1
    print(f"\nselftest: {len(cases) - failed}/{len(cases)} cases pass")
    return 1 if failed else 0


REFUSAL = """
ERROR: refusing to build.  A command substitution above calls a name that
nothing in reach defines.

At run time that substitution exits 127 and evaluates to the empty string.  If
it is guarded by `||`, wrapped in `if`, or assigned with `local`/`declare`, the
non-zero status is consumed and the caller proceeds with an empty value while
believing it succeeded -- which is how `check_libc_shape` shipped a gate that
never executed once in its life (known-issues.md ->
A-A-THE-LIBC-SHAPE-GATE-WAS-BORN-DEAD-AND-THE-WIRING-GATE-CALLS-IT-WIRED).

Neither `set -e`, `bash -n`, nor shellcheck will tell you about this: the
handler is explicit, the syntax is valid, and the name is a command rather than
a variable.

To repair, one of:
  * define the function (in this file, or in one this file already sources);
  * call the thing that actually exists -- check for a typo first;
  * if the callee is genuinely dynamic, put it in a variable, since
    `$("$tool" ...)` is outside this gate by design.

If a name is on PATH only on some hosts, do not silence this by adding it to
SHELL_WORDS -- guard the call with `command -v` and handle the absence, the way
the other gates in boot-test.sh do.
"""


def main() -> int:
    ap = argparse.ArgumentParser(description="check shell command-substitution callees")
    ap.add_argument("--list", action="store_true",
                    help="print every name that had to be asked of bash")
    ap.add_argument("--self-test", action="store_true",
                    help="grade the gate against real files, not the tree")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    bash = shutil.which("bash")
    if bash is None:
        return _decline(
            "cannot check shell callees: no `bash` on PATH",
            "Resolution has to be asked of the shell that actually runs these\n"
            "scripts, not of this interpreter -- MSYS bash has a PATH full of\n"
            "unix tools that Windows does not. Without it every such tool would\n"
            "read as a finding, so this declines rather than guessing.")

    files = tracked_shell_files()
    if len(files) < DISCOVERY_FLOOR:
        return _decline(
            f"cannot check shell callees: found {len(files)} shell file(s), "
            f"floor is {DISCOVERY_FLOOR}",
            "The count comes from `git ls-files`, so a number this low means\n"
            "either that this is not the repository worktree or that the\n"
            "enumeration stopped working. Both make this gate report a clean\n"
            "tree without reading one, so it declines instead of passing.")

    return check(files, bash, show_list=args.list)


if __name__ == "__main__":
    sys.exit(main())
