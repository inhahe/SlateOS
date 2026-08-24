#!/usr/bin/env python3
"""Differential check: does our long-option resolution agree with GNU's?

Every converted coreutils bin carries a ``LONG_OPTIONS`` table copied from GNU,
because that table — not the set of options we implement — is what decides
whether an abbreviation like ``--p`` is ambiguous.  Copying the *names* is not
enough.  GNU's ``struct option`` also carries a ``val``, and ``getopt_long``
judges ambiguity by that: two spellings sharing a ``val`` are one option, so a
prefix matching both of them resolves instead of failing.

``rmdir`` is the case that motivated this script.  Its table holds ``--path``
and ``--parents``, which are the same option, so GNU accepts ``rmdir --p`` —
while a faithful-looking name-only table refuses it.  ``cp`` has the same pair
*and* a genuinely different ``--preserve``, so ``cp --p`` is ambiguous but lists
only two of its three prefix matches.  Neither behaviour is guessable; both are
one measurement away.

What this does
--------------

For each bin it parses ``LONG_OPTIONS`` (and ``ALIASES`` if present) out of the
Rust source, and runs two independent comparisons against the real GNU utility.

**1. The table itself.**  ``<util> --=x`` prints GNU's whole option table: the
empty prefix matches every entry, so the ambiguity message lists all of them, in
declaration order.  That is a direct readout of the thing we are copying, so the
two name lists are compared as *sequences*.  Order is not cosmetic — glibc
reports ``pfound``, the first entry that matched, so two tables holding the same
names in different orders name different options in their diagnostics.

**2. Every abbreviation.**  For each distinct proper prefix of each name, our
verdict is compared with GNU's:

``we say ambiguous, GNU resolves``
    A missing alias, like ``rmdir --p``.  We refuse a command that works.

``we resolve, GNU says ambiguous``
    We silently accept an abbreviation GNU rejects, and act on whichever option
    we did match — the failure mode the tables exist to prevent, and the worse
    of the two.

The second check cannot find everything on its own, which is why the first
exists: the prefixes it probes are generated from *our* names, so an option GNU
has and we lack is never typed and never measured.  Only comparing the tables
finds that.  Both drift bugs caught so far were extra entries written from
memory of a newer upstream than the one we can measure — ``mv --exchange`` and
``cp --keep-directory-symlink`` (which is in fact a ``tar`` option) — and the
missing-entry direction is the more dangerous one, so it is worth a check that
can actually see it.

A genuine difference is possible without anyone being wrong: ``--context`` is
compiled out of a build without SELinux.  If that ever shows up here, exempt the
one name explicitly rather than loosening the comparison.

Running it
----------

    python scripts/getopt-ambiguity-check.py            # check every bin
    python scripts/getopt-ambiguity-check.py cp rmdir   # just these

It needs a GNU userland to compare against.  On this Windows host that means
WSL, which it finds itself; on a Linux host it runs the utilities directly.  If
neither is available it exits 0 with a note, because a check that cannot run is
not a failure — it just has nothing to say.

Exit codes: 0 agreed (or could not run), 1 disagreements found, 2 bad usage.
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

SRC_DIR = Path(__file__).resolve().parent.parent / "userspace" / "coreutils" / "src"
BIN_DIR = SRC_DIR / "bin"

# Bins whose name is ours rather than GNU's, or which GNU has no equivalent of.
# Comparing these against whatever the host happens to have under that name
# would measure the host, not us.
NOT_GNU = {
    "bc",  # GNU bc exists but is not coreutils and has a different option set
    "fetch",
    "logger",
    "time_cmd",
    "minishell",
}

# Only the *head* of the declaration is matched by regex; the body is then
# taken by counting brackets in `slice_body` below.
#
# The earlier version ended these patterns at a literal `\n];`, which quietly
# made the gate depend on how rustfmt had chosen to lay the table out. A table
# short enough to fit on one line — `yes` has exactly two entries, so rustfmt
# collapses it to `&[("help", …), ("version", …)];` — has no `];` at the start
# of a line and was therefore not found at all. The failure is silent and reads
# as reassurance: the bin falls through to "no LONG_OPTIONS table … not yet
# converted?", which is a note the sweep prints routinely for bins that really
# have none, and the summary still says "0 disagreement(s)".
TABLE_HEAD_RE = re.compile(r"const\s+LONG_OPTIONS\s*:\s*&\[\([^\]]*?\)\]\s*=\s*")
ALIAS_HEAD_RE = re.compile(r"const\s+ALIASES\s*:\s*&\[\(&str,\s*&str\)\]\s*=\s*")
ENTRY_RE = re.compile(r'\(\s*"([^"]*)"\s*,')
PAIR_RE = re.compile(r'\(\s*"([^"]*)"\s*,\s*"([^"]*)"\s*\)')

# A bin whose whole `main` is `coreutils::digest::main(&MD5)` keeps its option
# table in that module, not in itself. Upstream has the same arrangement —
# `src/digest.c` is one file compiled eight times — and it is the reason this
# has to be followed rather than skipped: without it, `md5sum` and `sha256sum`
# were both reported as "no LONG_OPTIONS table … not yet converted?", which is
# a note the reader is trained to ignore, and the sweep silently checked
# nothing for two shipped programs that do have one.
DELEGATE_RE = re.compile(r"coreutils::([a-z_][a-z0-9_]*)::main\s*\(")


@dataclass
class Table:
    util: str
    names: list[str]
    aliases: dict[str, str] = field(default_factory=dict)

    def identity(self, name: str) -> str:
        return self.aliases.get(name, name)

    def verdict(self, typed: str) -> str:
        """``"exact"``, ``"resolves"``, ``"ambiguous"`` or ``"unknown"``.

        Mirrors ``Program::resolve_long_aliased``: an exact match wins outright,
        then every later prefix match is compared against the *first* one — never
        against each other, which is what glibc does and what decides the shape
        of the reported list.
        """
        if typed in self.names:
            return "exact"
        hits = [n for n in self.names if n.startswith(typed)]
        if not hits:
            return "unknown"
        first = self.identity(hits[0])
        if any(self.identity(n) != first for n in hits[1:]):
            return "ambiguous"
        return "resolves"


def slice_body(text: str, start: int) -> str | None:
    """The contents of the `&[ … ]` beginning at or after ``start``.

    Counting brackets rather than matching a closing delimiter is what makes
    this independent of line breaks, so a one-line table and a fifty-line one
    parse alike. String literals are skipped so a `]` or `"` inside a name
    cannot end the slice early.
    """
    i = text.find("&[", start)
    if i == -1 or text[start:i].strip():
        return None  # something other than the slice literal follows the `=`
    i += 1  # sit on the `[`
    depth = 0
    j = i
    n = len(text)
    while j < n:
        c = text[j]
        if c == '"':
            j += 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
        elif c in "[(":
            depth += 1
        elif c in "])":
            depth -= 1
            if depth == 0:
                return text[i + 1 : j]
        j += 1
    return None


def strip_comments(body: str) -> str:
    """Drop `//` comments so a name quoted in prose is not read as an entry."""
    return "\n".join(re.sub(r"//.*$", "", line) for line in body.splitlines())


def parse_table(path: Path) -> Table | None:
    text = path.read_text(encoding="utf-8", errors="replace")
    m = TABLE_HEAD_RE.search(text)
    body = slice_body(text, m.end()) if m else None
    if body is None:
        d = DELEGATE_RE.search(text)
        if d and path.is_relative_to(BIN_DIR):
            shared = SRC_DIR / f"{d.group(1)}.rs"
            if shared.is_file():
                # Named for the *bin*, not the module: two programs share this
                # table, and each is compared against its own GNU binary. That
                # is not redundant — it is the only thing that would catch a
                # table right for one algorithm and wrong for the other.
                t = parse_table(shared)
                if t:
                    t.util = path.stem
                return t
        return None
    names = ENTRY_RE.findall(strip_comments(body))
    if not names:
        return None
    table = Table(util=path.stem, names=names)
    a = ALIAS_HEAD_RE.search(text)
    abody = slice_body(text, a.end()) if a else None
    if abody is not None:
        table.aliases = dict(PAIR_RE.findall(strip_comments(abody)))
    return table


def find_runner() -> list[str] | None:
    """How to run a GNU utility, as an argv prefix, or ``None``."""
    if sys.platform.startswith("linux"):
        return []
    wsl = shutil.which("wsl")
    if wsl:
        try:
            subprocess.run(
                [wsl, "-e", "true"], capture_output=True, timeout=30, check=True
            )
        except (subprocess.SubprocessError, OSError):
            return None
        return [wsl, "-e"]
    return None


POSSIBILITY_RE = re.compile(r"'--([^']*)'")


def gnu_table(runner: list[str], util: str) -> list[str] | None:
    """GNU's own long-option names, in declaration order, or ``None``.

    Read straight out of ``<util> --=x``: the empty prefix matches every entry,
    so glibc lists the whole table.  ``None`` means the readout did not happen —
    the utility is missing, or holds too few long options for anything to be
    ambiguous — which is not the same as "the table is empty" and must not be
    reported as every name having been deleted.
    """
    proc = subprocess.run(
        [*runner, "bash", "-c",
         'export LC_ALL=C.UTF-8; cd "$(mktemp -d)" || exit 1; '
         'exec timeout 5 "$1" --=x 2>&1 </dev/null',
         "probe", util],
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    for line in proc.stdout.splitlines():
        if "is ambiguous" not in line:
            continue
        _, _, tail = line.partition("possibilities:")
        names = POSSIBILITY_RE.findall(tail)
        if names:
            return names
    return None


def compare_tables(table: Table, theirs: list[str]) -> list[str]:
    """Ordered comparison of our names against GNU's."""
    ours = table.names
    if ours == theirs:
        return []
    problems = []
    missing = [n for n in theirs if n not in ours]
    extra = [n for n in ours if n not in theirs]
    for n in missing:
        problems.append(
            f"{table.util}: GNU has --{n}, LONG_OPTIONS does not; "
            f"every abbreviation of it is one we may misresolve"
        )
    for n in extra:
        problems.append(
            f"{table.util}: LONG_OPTIONS has --{n}, GNU does not; "
            f"likely copied from a different release than the reference"
        )
    if not missing and not extra:
        problems.append(
            f"{table.util}: same names as GNU but in a different order, so "
            f"an ambiguous abbreviation names a different option than GNU's\n"
            f"    ours: {ours}\n"
            f"    GNU:  {theirs}"
        )
    return problems


def gnu_verdicts(runner: list[str], util: str, prefixes: list[str]) -> dict[str, str]:
    """Ask GNU for its verdict on each prefix, in one shell round trip.

    One invocation per prefix would be a few thousand process spawns through
    WSL; a single ``bash -c`` running the loop on the far side is the difference
    between seconds and half an hour.

    Each probe runs with no operands, so an option that *does* resolve reaches
    "missing operand" and stops.  Nothing here creates, removes or writes
    anything — but it still runs in a scratch directory, because being sure of
    that by construction is cheaper than auditing 70 utilities' idle behaviour.

    Two guards keep a probe from running away.  ``timeout`` bounds it: an
    option that resolves has already told us what we came to find out, and some
    of them then go on to *work* — ``tail --follow`` with stdin on ``/dev/null``
    waits for a write that never comes, and hung this script for ten minutes
    before the guard existed.  ``head -c`` bounds the output for the same reason
    in the other direction.  Both only ever truncate a probe we have already
    classified, because the diagnostic we match on is printed before any of it.
    """
    script = r"""
set -u
export LC_ALL=C.UTF-8
d=$(mktemp -d) || exit 1
cd "$d" || exit 1
util=$1; shift
for p in "$@"; do
    out=$(timeout 2 "$util" "--$p" 2>&1 </dev/null | head -c 2000)
    case "$out" in
        *"is ambiguous"*)        echo "$p ambiguous" ;;
        *"unrecognized option"*) echo "$p unknown" ;;
        *)                       echo "$p resolves" ;;
    esac
done
cd /; rmdir "$d" 2>/dev/null || true
"""
    try:
        proc = subprocess.run(
            [*runner, "bash", "-c", script, "probe", util, *prefixes],
            capture_output=True,
            text=True,
            timeout=600,
            check=False,
        )
    except subprocess.TimeoutExpired:
        # Partial output is unreachable through TimeoutExpired here, so treat
        # the whole utility as unmeasured rather than half-measured: a verdict
        # table missing an unknown subset would report phantom disagreements.
        return {}
    out: dict[str, str] = {}
    for line in proc.stdout.splitlines():
        parts = line.split()
        if len(parts) == 2:
            out[parts[0]] = parts[1]
    return out


def check(table: Table, runner: list[str]) -> list[str]:
    """Every way this bin's table disagrees with GNU's."""
    theirs_table = gnu_table(runner, table.util)
    if theirs_table is None:
        return [f"{table.util}: could not read GNU's table (utility missing?)"]
    if mismatch := compare_tables(table, theirs_table):
        # Stop here rather than also sweeping prefixes. Every prefix of a name
        # that only one side has disagrees, so the sweep would bury the one
        # finding that matters under a dozen restatements of it.
        return mismatch

    # Every distinct proper prefix of every name. An exact name is skipped: the
    # exact-match rule short-circuits ambiguity on both sides, so it can never
    # disagree and probing it only risks running the option for real.
    prefixes = sorted(
        {n[:i] for n in table.names for i in range(1, len(n))} - set(table.names)
    )
    if not prefixes:
        return []
    theirs = gnu_verdicts(runner, table.util, prefixes)
    if not theirs:
        return [f"{table.util}: no GNU verdicts came back (utility missing?)"]

    problems: list[str] = []
    for p in prefixes:
        ours = table.verdict(p)
        gnu = theirs.get(p)
        if gnu is None or ours == gnu:
            continue
        if ours == "ambiguous" and gnu == "resolves":
            hits = [n for n in table.names if n.startswith(p)]
            problems.append(
                f"{table.util}: --{p} we say ambiguous, GNU resolves it; "
                f"matches {hits}: a missing ALIASES entry"
            )
        elif ours == "resolves" and gnu == "ambiguous":
            problems.append(
                f"{table.util}: --{p} we resolve, GNU says ambiguous; "
                f"LONG_OPTIONS is missing an entry"
            )
        else:
            problems.append(f"{table.util}: --{p} we say {ours}, GNU says {gnu}")
    return problems


def main() -> int:
    wanted = set(sys.argv[1:])
    runner = find_runner()
    if runner is None:
        # ASCII only: this console's code page is not UTF-8 and mangles the rest.
        print("no GNU userland available (no WSL, not Linux); nothing to check")
        return 0

    tables = []
    for path in sorted(BIN_DIR.rglob("*.rs")):
        stem = path.stem if path.stem != "main" else path.parent.name
        if stem in NOT_GNU or (wanted and stem not in wanted):
            continue
        t = parse_table(path)
        if t:
            t.util = stem
            tables.append(t)

    if wanted:
        # Three reasons a requested name may not be checked, and only the last
        # is worth a warning. The pre-push hook passes whichever bins a push
        # rewrote, so it routinely names bins in the first two groups; warning
        # about those would train the reader to ignore the warnings.
        missing = wanted - {t.util for t in tables} - NOT_GNU
        for m in sorted(missing):
            if not any(
                p.stem == m or (p.stem == "main" and p.parent.name == m)
                for p in BIN_DIR.rglob("*.rs")
            ):
                continue  # not a coreutils bin at all
            print(
                f"note: {m} has no LONG_OPTIONS table, so there is nothing to "
                f"compare (not yet converted to coreutils::getopt?)",
                file=sys.stderr,
            )

    problems: list[str] = []
    for t in tables:
        # Printed per table rather than at the end: a full sweep is minutes of
        # WSL round trips, and a finding you can see at minute two is worth
        # more than the same finding at minute ten.
        found = check(t, runner)
        for line in found:
            print(line, flush=True)
        problems.extend(found)

    print(f"\n{len(tables)} table(s) checked; {len(problems)} disagreement(s).")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
