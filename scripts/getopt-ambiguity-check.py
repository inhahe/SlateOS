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

**1. The table itself.**  ``<util> --=x`` prints GNU's option table: the empty
prefix matches every entry, so the ambiguity message lists them, in declaration
order.  That is a direct readout of the thing we are copying, so the two name
lists are compared as *sequences*.  Order is not cosmetic — glibc reports
``pfound``, the first entry that matched, so two tables holding the same names
in different orders name different options in their diagnostics.

The readout is *not* quite the table, and the gap is the same ``pfound`` rule.
glibc lists a match only when it differs from the first one, so an alias of the
**first** entry is dropped: ``tty``'s four-entry table reads out as three, with
``--quiet`` missing because ``--silent`` precedes it.  An alias of any *later*
entry survives, which is why ``rmdir`` lists both ``--path`` and ``--parents``.
So our side is put through the identical elision (``Table.readout``) before the
comparison — which makes this a comparison of the two programs' *output*, since
``resolve_long_aliased`` performs that elision at runtime too.  Comparing the
raw names instead reports a phantom "LONG_OPTIONS has --quiet, GNU does not",
and did, for a correct ``tty`` table.

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

**When the utility is not a glibc one.**  Check 1 assumes ``getopt_long``, and
one utility we transcribe does not use it: GNU ed carries its own
``arg_parser.c``, which prints ``option '--=x' is ambiguous`` and stops, with no
candidate list to read back.  The readout came back empty and the gate said
"utility missing?" about a utility that is installed — a diagnostic that sends
the reader looking for a package rather than at the real difference.

Those bins are listed in ``OWN_PARSER`` and compared through a second path: the
names in ``<util> --help``, as an unordered *set*.  Unordered because the thing
declaration order decides in glibc is which candidate a diagnostic names first,
and these utilities print no candidates — so help-text order would be evidence
about the manual, not about the parser.  Check 2 is unaffected and does the real
work: these parsers were written to imitate glibc closely enough that they emit
the same two phrases it classifies on.

A ``--help`` scrape is a one-sided measurement, and the code is asymmetric to
match.  Cross-checked against the ``--=x`` readout on six glibc utilities, help
never named an option the parser lacked but routinely omitted ones it had —
``cp --path``, ``date --uct``, ``tar --HANG``.  So a name GNU documents and we
lack is reported outright, while a name we carry and its help omits is reported
only after GNU has been asked directly and answered "unrecognized option".
Without that second step every undocumented alias we correctly carry would read
as a defect to be deleted.

Running it
----------

    python scripts/getopt-ambiguity-check.py            # check every bin
    python scripts/getopt-ambiguity-check.py cp rmdir   # just these
    python scripts/getopt-ambiguity-check.py --selftest # check the checker

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
    "minishell",
}

# Bins whose GNU counterpart parses long options with its *own* parser rather
# than glibc's `getopt_long`, and the reason. These are not skipped -- the
# comparison is still wanted, and the prefix sweep below still works, because
# these parsers were written to imitate glibc and emit the same two phrases
# ("is ambiguous", "unrecognized option") that `gnu_verdicts` classifies on.
# What does *not* work is the `--=x` table readout: it depends on glibc listing
# the candidates, and these parsers print the complaint without the list. The
# readout comes back empty, which `gnu_table` reports as `None` and `check`
# renders as "utility missing?" -- a diagnostic that sends the reader to look
# for a package that is in fact installed.
#
# For these the names are read out of `--help` instead (`gnu_help_table`), and
# compared as a *set*: `--help` is hand-written prose, so its order is evidence
# about the manual rather than about the parser, and ordering is meaningless
# here anyway -- the thing declaration order decides in glibc is which
# candidate a diagnostic names first, and these utilities print no candidates.
# The per-prefix sweep is what actually pins the behaviour down, and it is
# unaffected.
OWN_PARSER: dict[str, str] = {
    # GNU ed carries `arg_parser.c` (the "carg_parser" Antonio Diaz Diaz uses
    # across his projects) so that it builds where glibc is not. Measured on
    # ed 1.20.1: `ed --=x` prints "ed: option '--=x' is ambiguous" and stops,
    # where a glibc utility would go on to "possibilities: ...".
    "ed": "GNU ed uses carg_parser, not glibc getopt_long, so it prints no "
          "candidate list for --=x to read back",
}

# Bins that have no `LONG_OPTIONS` table on purpose, and the reason. Without
# this the sweep prints "not yet converted to coreutils::getopt?" for them,
# which reads as a pending task and invites someone to do work that has already
# been considered and declined. The question mark is the problem: it is a guess,
# and for these the answer is recorded.
#
# Distinct from NOT_GNU, which is "we cannot compare this". These *could* be
# compared; the point is that the table they would be compared against is the
# wrong table.
#
# `tar` used to be here, on the reasoning that argp is not getopt_long and a
# table would describe a parser we were not emulating. That reasoning was
# wrong in its premise: argp *calls* getopt_long, so tar's long-option
# behaviour -- abbreviation, ambiguity, the candidate list and its order -- is
# glibc's, and is exactly what this gate compares. tar now carries the full
# 172-entry table and is checked like everything else. (The one thing that
# really is outside getopt is tar's dash-less "old option" form,
# `tar cvf x.tar`, which this gate does not claim to cover either way.)
NOT_GETOPT: dict[str, str] = {}

# Bins whose source file is named something other than the program, because the
# obvious name is taken. Checking these under `path.stem` would probe a command
# that does not exist and read as "utility missing?"; skipping them instead
# would lose a real comparison, since GNU does ship the program -- just under
# the other name.
GNU_NAME = {
    # `src/bin/time.rs` would collide with Rust's own `time` crate, and the
    # program it transcribes is GNU Time 1.9's /usr/bin/time. Note that this is
    # *not* the shell's `time` keyword: the probe runs the name through "$1",
    # which bash expands after keyword recognition, so PATH is consulted.
    "time_cmd": "time",
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

    @property
    def gnu(self) -> str:
        """The name to *run* on the GNU side.

        `util` names our source file and is what every message says, because
        that is the name a reader has to go and edit. Only the subprocess needs
        the other one.
        """
        return GNU_NAME.get(self.util, self.util)

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

    def readout(self) -> list[str]:
        """What ``<util> --=x`` would print for *this* table.

        Not the same list as ``names``, and the difference is the whole reason
        this method exists.  glibc marks a match as a candidate only when it
        differs from ``pfound`` — the first match — so an alias of the *first*
        entry is silently dropped from the possibilities, while an alias of any
        later one is kept.  ``rmdir`` shows the keeping half: ``--path`` and
        ``--parents`` are one option, but ``--ignore-fail-on-non-empty`` comes
        first, so both are listed.  ``tty`` shows the dropping half: ``--silent``
        *is* first, so its alias ``--quiet`` never appears and a four-entry table
        reads out as three.

        Comparing ``names`` against the readout therefore reports a phantom
        "LONG_OPTIONS has --quiet, GNU does not" for every correct table shaped
        like ``tty``'s.  It did, on the commit that added ``tty``'s.
        """
        first, *rest = self.names
        first_id = self.identity(first)
        differing = [n for n in rest if self.identity(n) != first_id]
        if not differing:
            # Nothing differs from `pfound`, so the prefix resolves and no
            # ambiguity message is printed at all. `gnu_table` returns None in
            # that case, and this must be comparable with it.
            return []
        return [first, *differing]


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


# An option line in a GNU `--help` block: two or more spaces, optionally a
# short form and its comma, then the long form. Anchoring on the leading indent
# is what keeps prose out — a sentence in the description that happens to name
# `--foo` starts at column 0 or inside a paragraph, not in the option column.
HELP_OPT_RE = re.compile(r"^\s{2,}(?:-\w,\s+)?(--\S.*)$")
HELP_NAME_RE = re.compile(r"--([A-Za-z0-9][A-Za-z0-9-]*)")


def gnu_help_table(runner: list[str], util: str) -> set[str] | None:
    """GNU's long-option names scraped out of ``--help``, or ``None``.

    The fallback for `OWN_PARSER` utilities, whose ambiguity diagnostic carries
    no candidate list. A *set*, not a list, and deliberately so: see the comment
    on `OWN_PARSER` for why order read off hand-written help text would be
    evidence about the manual rather than about the parser.

    Only the option column of each line is considered. Everything from the
    first run of two-or-more spaces onward is the description, which is prose
    and may legitimately mention an option this utility does not have (`ed`'s
    own help talks about `red`; other utilities cross-reference each other).
    """
    proc = subprocess.run(
        [*runner, "bash", "-c",
         'export LC_ALL=C.UTF-8; cd "$(mktemp -d)" || exit 1; '
         'exec timeout 5 "$1" --help 2>&1 </dev/null',
         "probe", util],
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    names: set[str] = set()
    for line in proc.stdout.splitlines():
        m = HELP_OPT_RE.match(line)
        if not m:
            continue
        # `--quiet, --silent      suppress ...` -> `--quiet, --silent`.
        column = re.split(r"\s{2,}", m.group(1), maxsplit=1)[0]
        names.update(HELP_NAME_RE.findall(column))
    return names or None


def compare_name_sets(
    table: Table, theirs: set[str], verdicts: dict[str, str]
) -> list[str]:
    """Unordered comparison, for utilities read out of ``--help``.

    Compares the *declared* names rather than `Table.readout()`: the elision
    `readout` performs models glibc's candidate list, and these utilities print
    no candidate list for it to model.

    The two directions are not symmetric, because a `--help` scrape is a
    one-sided measurement. Cross-checked against the `--=x` readout on six
    glibc utilities, `--help` never named an option the parser lacked, but it
    routinely omitted ones it had: `cp` parses `--path` and `--recursive`,
    `date` parses `--uct`, `--iso-8601`, `--rfc-822` and `--rfc-2822`, and
    `tar` parses `--HANG`, none of which appear in their help text. Deprecated
    spellings are kept working and dropped from the manual, which is exactly
    the shape of thing a table transcribed from source would have and a table
    transcribed from `--help` would not.

    So a name GNU's help lists and we lack is reported outright, but a name we
    list and its help does not is only reported once GNU has been asked
    directly and answered "unrecognized option" (`verdicts`). Without that
    second step every undocumented alias we correctly carry would be reported
    as a defect, and the gate would be teaching the reader to delete correct
    entries.
    """
    ours = set(table.names) | set(table.aliases)
    problems = []
    for n in sorted(theirs - ours):
        problems.append(
            f"{table.util}: GNU has --{n}, LONG_OPTIONS does not; "
            f"every abbreviation of it is one we may misresolve"
        )
    for n in sorted(ours - theirs):
        if verdicts.get(n) != "unknown":
            continue  # undocumented but parsed, or unmeasured — not a finding
        problems.append(
            f"{table.util}: LONG_OPTIONS has --{n}, GNU neither documents nor "
            f"accepts it; likely copied from a different release"
        )
    return problems


def compare_tables(table: Table, theirs: list[str]) -> list[str]:
    """Ordered comparison of our names against GNU's.

    Both sides are *readouts*, not tables: ``theirs`` is what ``--=x`` printed,
    so ours has to be put through the same elision — see ``Table.readout`` — or
    an alias of the first entry is reported as an entry GNU lacks.  Our own
    ``resolve_long_aliased`` performs the identical elision at runtime, so this
    is comparing the two programs' output and not two data structures, which is
    the stronger of the two comparisons anyway.
    """
    ours = table.readout()
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
            f"likely copied from a different release than the reference "
            f"(or --{n} aliases our first entry and needs an ALIASES row)"
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
    if table.util in OWN_PARSER:
        theirs_names = gnu_help_table(runner, table.gnu)
        if theirs_names is None:
            return [
                f"{table.util}: could not read GNU's options out of --help "
                f"(utility missing?). It is in OWN_PARSER because "
                f"{OWN_PARSER[table.util]}, so --=x is not an option either."
            ]
        # Probing an exact name can run the option for real, which is why the
        # prefix sweep below skips them. Here it is unavoidable and narrow: the
        # only names probed are ones GNU's help does not document, and the
        # question asked of each is whether it exists at all. `gnu_verdicts`
        # bounds every probe with a timeout, a scratch directory and
        # /dev/null on stdin.
        extras = sorted((set(table.names) | set(table.aliases)) - theirs_names)
        verdicts = gnu_verdicts(runner, table.gnu, extras) if extras else {}
        mismatch = compare_name_sets(table, theirs_names, verdicts)
    else:
        theirs_table = gnu_table(runner, table.gnu)
        if theirs_table is None:
            return [f"{table.util}: could not read GNU's table (utility missing?)"]
        mismatch = compare_tables(table, theirs_table)
    if mismatch:
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
    theirs = gnu_verdicts(runner, table.gnu, prefixes)
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


def selftest() -> int:
    """Check the rules that decide what the `--help` readout reports.

    This path exists because one utility's parser is not glibc's, so it cannot
    be exercised by running the gate on the tree: `ed` agrees with GNU today,
    and a broken comparison would look exactly like that agreement. The
    asymmetry in `compare_name_sets` is the specific thing at risk — the
    suppression of undocumented-but-parsed names is one `continue` away from
    either reporting every alias as a defect or reporting nothing at all, and
    neither mistake changes the summary line on a clean tree.

    Rules are counted from `rule()` calls rather than a literal, so adding a
    case cannot leave the summary claiming a total that no longer ran.
    """
    failures: list[str] = []
    rules: list[str] = []
    current = ""

    def rule(name: str) -> None:
        nonlocal current
        current = name
        rules.append(name)

    def expect(label: str, got: object, want: object) -> None:
        if got != want:
            failures.append(f"{current}: {label}: want {want!r}, got {got!r}")

    # 1. The option column is taken, the description is not. GNU ed's help is
    #    the real text: a short form and its comma, a value placeholder, two
    #    long forms on one line, and a line with no short form at all.
    rule("help-scrape")
    help_text = (
        "Usage: ed [options] [[+line] file]\n"
        "Options:\n"
        "  -h, --help                 display this help and exit\n"
        "  -p, --prompt=STRING        use STRING as an interactive prompt\n"
        "  -q, --quiet, --silent      suppress diagnostics\n"
        "      --unsafe-names         allow control characters\n"
        "The file name may be preceded by '+line'. See also --nonesuch.\n"
    )
    got: set[str] = set()
    for line in help_text.splitlines():
        m = HELP_OPT_RE.match(line)
        if m:
            column = re.split(r"\s{2,}", m.group(1), maxsplit=1)[0]
            got.update(HELP_NAME_RE.findall(column))
    expect(
        "names",
        got,
        {"help", "prompt", "quiet", "silent", "unsafe-names"},
    )

    t = Table(util="u", names=["alpha", "beta"])

    # 2. Agreement is silence.
    rule("agree")
    expect("clean", compare_name_sets(t, {"alpha", "beta"}, {}), [])

    # 3. A name GNU documents and we lack is reported on the help readout
    #    alone. This direction needs no probe: `--help` was never observed to
    #    name an option the parser lacked.
    rule("missing")
    out = compare_name_sets(t, {"alpha", "beta", "gamma"}, {})
    expect("count", len(out), 1)
    expect("names-gamma", out and "--gamma" in out[0], True)

    # 4. A name we carry that GNU's help omits is NOT a finding until GNU has
    #    been asked. Unmeasured and "resolves" both stay silent; only
    #    "unknown" — GNU answering "unrecognized option" — is reported. Get
    #    this wrong and `cp --path`, `date --uct` and `tar --HANG` become
    #    defects to be deleted.
    rule("extra")
    expect("unmeasured-silent", compare_name_sets(t, {"alpha"}, {}), [])
    expect(
        "undocumented-silent",
        compare_name_sets(t, {"alpha"}, {"beta": "resolves"}),
        [],
    )
    out = compare_name_sets(t, {"alpha"}, {"beta": "unknown"})
    expect("absent-reported", len(out), 1)
    expect("names-beta", out and "--beta" in out[0], True)

    # 5. An ALIASES row counts as one of ours, or every aliased spelling GNU
    #    documents would be reported as missing from a table that has it.
    rule("aliases")
    ta = Table(util="u", names=["alpha"], aliases={"alias": "alpha"})
    expect("alias-is-ours", compare_name_sets(ta, {"alpha", "alias"}, {}), [])

    for f in failures:
        print(f"selftest FAIL {f}")
    print(
        f"selftest: {len(rules) - len({f.split(':')[0] for f in failures})}"
        f"/{len(rules)} rules ok"
    )
    return 1 if failures else 0


def main() -> int:
    args = sys.argv[1:]
    if "--selftest" in args:
        return selftest()
    wanted = set(args)
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

    # A NOT_GETOPT entry is a claim about the tree, so it has to be able to go
    # stale. If the bin grew a table after all, the recorded reason is now
    # false and the exemption is silently suppressing a real comparison --
    # which is the same failure the `];` regex bug caused, and reads the same
    # way: reassuringly quiet. Checked over the whole tree, not just `wanted`,
    # so a push that does not touch the bin still catches it.
    stale_exemptions = [
        name
        for name in sorted(NOT_GETOPT)
        for p in BIN_DIR.rglob("*.rs")
        if (p.stem if p.stem != "main" else p.parent.name) == name
        and parse_table(p) is not None
    ]

    # An OWN_PARSER entry can go stale the same way, in the other direction: if
    # upstream switches to glibc (or the recorded observation was wrong), --=x
    # starts listing candidates and the entry is now downgrading an ordered
    # comparison to an unordered one for no reason. One WSL round trip per
    # entry, over the whole tree rather than just `wanted`, for the reason
    # above -- a push that does not touch the bin should still catch it.
    stale_own_parser = [
        name
        for name in sorted(OWN_PARSER)
        if gnu_table(runner, GNU_NAME.get(name, name)) is not None
    ]

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
            if m in NOT_GETOPT:
                print(
                    f"note: {m} has no LONG_OPTIONS table by decision, not by "
                    f"omission -- {NOT_GETOPT[m]}",
                    file=sys.stderr,
                )
                continue
            print(
                f"note: {m} has no LONG_OPTIONS table, so there is nothing to "
                f"compare (not yet converted to coreutils::getopt?)",
                file=sys.stderr,
            )

    problems: list[str] = []
    for name in stale_exemptions:
        line = (
            f"{name}: listed in NOT_GETOPT, but it now has a LONG_OPTIONS "
            f"table. Remove the exemption so the table is actually compared, "
            f"and delete the recorded reason -- it is no longer true."
        )
        print(line, flush=True)
        problems.append(line)

    for name in stale_own_parser:
        line = (
            f"{name}: listed in OWN_PARSER, but --=x now reads back a candidate "
            f"list, so it does use glibc getopt_long after all. Remove the "
            f"entry -- it is costing the ordered comparison and the recorded "
            f"reason ({OWN_PARSER[name]}) is no longer true."
        )
        print(line, flush=True)
        problems.append(line)

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
