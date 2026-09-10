#!/usr/bin/env python3
"""Refuse a missing shell operand that silently becomes a number.

Why this exists
===============

`kernel/src/kshell.rs` contains this shape in many places:

    let id: u32 = match parts.get(1).unwrap_or(&"0").parse() {
        Ok(v) => v,
        Err(_) => { shell_println!("Invalid vault ID"); set_exit(1); return; }
    };

The `match` refuses an **unreadable** operand correctly, which is why
`check-option-refusal.py` does not count this: that checker looks for a guessed
value surviving a failed parse, and here the parse refuses. What neither it nor a
reader notices is the other half — the default `"0"` is supplied for an **absent**
operand and then parses perfectly, so a *missing* id becomes a real one.

`filevault unlock`, with nothing after it, therefore attempts to unlock vault 0
with the empty password that `parts.get(2)` also defaults to. `screensaver
preview`, bare, previews saver 1. The command does not fail and does not ask; it
acts on an object the operator never named.

This is a cousin of `design-decisions.md` §600's second prohibited shape, not an
instance of it, and the distinction matters: §600 is about a word that could not
be *read*, and this is about a word that was never *said*.

Why it needed a checker before it needed a fix
==============================================

It was measured three times by hand, with three different patterns, and gave
three answers — 14, then 31, then 37 — two of which were written into
`known-issues.md` as though they answered the same question. A population that
cannot be counted the same way twice cannot be burned down: a partial pass leaves
no marker saying how far it got, and the number in the prose drifts from the tree
the moment anyone edits either.

So the definition lives here, in code, and the count lives in
`scripts/absent-operand-ledger.txt`. That is the arrangement that makes §600's
"78 of 800 remain" trustworthy, and the reason asking *its* checker which entries
dropped — rather than grepping — is what kept batch 46's arithmetic right.

Two rules, one ledger
=====================

**Rule A -- an invented NUMBER.** A default that parses as a base-10 integer,
in any `parts.get(n).unwrap_or(&"0")` form. This was the original rule and it
is a proxy: it guesses that a number is an identifier. It stays because it
reaches arms whose command prints no synopsis at all, and because it recognises
the `match parts.get(n).unwrap_or(...).parse()` form that rule B's let-binding
matcher does not.

**Rule B -- a broken PROMISE.** The command's own printed help marks the operand
required -- angle brackets, the convention this file keeps in 2348 documented
positions across 269 commands -- and the code supplies a value anyway. This is
the better question. It needs no judgment at all: the tree states the
requirement and the tree states the violation, so the checker only puts them
side by side. It also sees string defaults, which are 78 of its 83 findings and
were wholly invisible to rule A. `apppermissions` prints `grant <app> <perm>`
and grants the storage permission to an application named "app".

They share ONE ledger. Measured at af70253de the overlap is 3 of 37 and 83,
which makes two ledgers look safe -- but that near-disjointness is an artifact
of rule B's matcher, not a property of the populations: broaden it to the
`match` form and most of A falls inside B. Two ledgers over one family is how a
burn-down double-counts, which is the thing this lane warned lane B about the
same afternoon this rule was written.

What counts
===========

A call of the form

    parts.get(<n>).unwrap_or(&"<literal>")

where `<literal>` parses as a base-10 integer. That is narrower than the harm and
deliberately so:

  * whether the number names a **live object** is judgment (share 0 is real; a
    count of 10 is harmless), and judgment does not belong in a counter;
  * the pattern is exactly decidable, so two runs agree, which is the only
    property that makes the ledger worth having.

A non-numeric default (`&""`, `&"all"`) is not counted: it cannot silently become
an identifier, and `""` is the established spelling for "the operator said
nothing" that the code already tests for.

Two things are deliberately not counted, and they are not the same kind of
exclusion.

**Comments and string bodies are masked** before matching. The first run of this
checker reported 38 sites; one was a comment in `cmd_colortemp` quoting the line
it had just replaced. Counting an epitaph means a fixed-and-documented site still
scores, the ledger can never reach zero while the explanation exists, and the
cheapest way to lower the number is to delete the comment. See mask_noncode(),
whose first draft blanked string bodies too and took the count from 37 to 0 --
the pattern being measured *is* a string literal.

**Operands their own help calls optional are blessed** by an `allow` line in the
ledger. The criterion is the command's printed synopsis, and nothing else: square
brackets mean the operand may be omitted, so supplying a value for it is the
documented behaviour rather than a guess. `sharesheet` prints `history [count]`,
so defaulting to 10 rows is correct and counting it is noise.

That criterion replaced a worse one, and the way it failed is worth keeping. The
first version blessed by MAGNITUDE -- every default naming an object is 0 or 1,
every default naming an amount is larger -- which is a true observation about the
data and the wrong test. Six sites were blessed under it. Three were defects:

    screensaver  prints `timeout <id> <s>`       ... and defaults s to 300
    sysanimations prints `speed <percent>`       ... and defaults percent to 100
    filevault    prints `autolock <id> <seconds>` ... and defaults seconds to 300

Angle brackets. The command tells the operator the operand is required, the
operator omits it, and the command silently succeeds on a value it chose. That is
the same defect as `filevault unlock` acting on vault 0, only the consequence is
a wrong duration instead of a wrong object -- and cmd_colortemp's own comment had
already settled the rule months earlier: "the synopsis says `set <id> <kelvin>`,
angle brackets, so there is no documented default to fall back to."

The blessing reasons now quote the synopsis, because the three wrong ones were
justified with a synopsis this author wrote from memory -- `[secs]`, in square
brackets that appear nowhere in the tree. An exemption argued from invented
documentation is worse than no exemption, so every `allow` line cites the line of
help text that licenses it, and that text is in the same file and greppable.

So the ledger's floor is not zero, and a reader who drives it to zero deletes
three working defaults. Three sites are allowed; each also swallows a failed
parse, which is a different defect counted in
`scripts/option-refusal-ledger.txt`, where all three functions already appear.

The ledger, and what fails
==========================

One line per enclosing function, `<fn> <count>`. The check fails when:

  * a function holds more matching sites than its entry claims, or holds them with
    no entry at all — the ratchet slipping;
  * an entry claims MORE than exist — the site was fixed and the count was not
    lowered, or the function was renamed and the entry now exempts something it
    was never written for.

Both directions, for the reason `check-option-refusal.py` gives: an entry that
over-claims is how a fixed site stays counted, and a count nobody can trust is
worse than no count because it is quoted.

Usage
=====

    python scripts/check-absent-operand-default.py            # the gate
    python scripts/check-absent-operand-default.py --list      # every site
    python scripts/check-absent-operand-default.py --pin       # write the ledger
                                                              # (allow lines kept)
    python scripts/check-absent-operand-default.py --self-test

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
LEDGER = REPO / "scripts" / "absent-operand-ledger.txt"

LEDGER_HEADER = (
    "# The counted `absent-operand-becomes-a-number` backlog for\n"
    "# scripts/check-absent-operand-default.py.\n"
    "#\n"
    "# Each line is `<enclosing function> <count>`: that many places inside that\n"
    "# function supply a numeric default for an operand the user did not give, so a\n"
    "# missing id becomes a real one. See that script's header for why the\n"
    "# definition lives in code and the count lives here.\n"
    "#\n"
    "# The number may only go DOWN. An entry claiming more sites than exist fails\n"
    "# too, because that is how a fixed site stays counted.\n"
    "# ONE exception, and it must be auditable. The number may rise when the RULE\n"
    "# changes -- a better question finds sites that were always there. When that\n"
    "# happens the re-pin belongs in the same commit as the rule change, so the\n"
    "# diff shows a new detector next to the new total. A re-pin on its own, with\n"
    "# no rule change beside it, is the ratchet being filed off.\n"
    "#\n"
    "#\n"
    "# An `allow <function> <literal>` line exempts one (function, default) pair.\n"
    "# The criterion is the command's OWN printed synopsis and nothing else: square\n"
    "# brackets mean the operand may be omitted, so a default for it is documented\n"
    "# behaviour. `sharesheet` prints `history [count]`, so defaulting to 10 rows is\n"
    "# correct. Each allow line below quotes the help text that licenses it.\n"
    "#\n"
    "# Do not bless by magnitude. The first version of this file did -- a default\n"
    "# that names an object is 0 or 1, one that names an amount is larger -- and it\n"
    "# blessed three defects: screensaver prints `timeout <id> <s>`, sysanimations\n"
    "# prints `speed <percent>`, filevault prints `autolock <id> <seconds>`. Angle\n"
    "# brackets, all three. The command tells the operator the operand is required,\n"
    "# the operator omits it, and the command silently succeeds on a value it chose.\n"
    "#\n"
    "# So the floor of this ledger is NOT zero, and a reader who drives it to zero\n"
    "# deletes three working defaults.\n"
    "#\n"
    "# These lines say nothing about the other defect on the same source line. Each\n"
    "# allowed site also writes `.parse().unwrap_or(N)`, replacing a word it could\n"
    "# not read with a guess -- design-decisions.md 600's shape, counted separately\n"
    "# in scripts/option-refusal-ledger.txt, where all three functions already\n"
    "# appear. Blessed here means blessed for the absent-operand question only.\n"
    "#\n"
)

# `parts.get(1).unwrap_or(&"0")` -- the index and the default literal.
SITE = re.compile(r"""\bparts\s*\.\s*get\(\s*(\d+)\s*\)\s*\.\s*unwrap_or\(\s*&"([^"]*)"\s*\)""")

# `fn cmd_foo(` at any indentation -- kshell's command handlers.
FN = re.compile(r"^\s*(?:pub\s+)?fn\s+([a-z_][a-z0-9_]*)\s*[(<]", re.M)


def is_numeric(literal: str) -> bool:
    """Whether a default literal would parse as a base-10 integer.

    `"0"` does and is the whole problem. `""` does not, and is the established
    spelling for "the operator said nothing" -- code that tests `is_empty()`
    afterwards is asking the right question and must not be flagged for it.
    """
    if not literal:
        return False
    body = literal[1:] if literal[0] in "+-" else literal
    return body.isdigit()


def mask_noncode(text: str) -> str:
    """Blank out comments and string bodies, preserving every byte offset.

    Why this is not optional. The first run of this checker counted 38 sites, and
    one of them was this, in `cmd_colortemp`:

        // guessed was an **absent** one -- `parts.get(1).unwrap_or(&"1")`

    That is not a defect. It is the *epitaph* of a defect -- a comment left behind
    when the site was fixed, quoting the old line so the next reader understands
    what changed. Counting it means a fixed-and-documented site still scores
    against the ledger, so the ledger can never reach zero while the explanation
    exists, and the cheapest way to make the number fall is to delete the
    comment. A counter that rewards erasing the record is worse than no counter.

    Offsets are preserved rather than the text removed, because `sites()` reports
    line numbers and attributes each hit to the function it sits in; collapsing
    the text would shift both. Every masked byte becomes a space, except newlines,
    which are kept so line counting is unaffected.

    String bodies are *traversed but not blanked*, and the distinction is the whole
    subtlety of this function. The scanner must know where strings are, because a
    naive `//`-to-end-of-line strip would treat the `//` inside a path literal like
    "a//b" as a comment start and blank the rest of the line, hiding any real site
    after it. But it must not blank them, because the pattern being measured *is* a
    string literal -- `parts.get(1).unwrap_or(&"0")` -- so masking string bodies
    masks the `0` that decides whether the site counts at all.

    The first draft blanked them, and this docstring already said a false negative
    would be worse than the phantom it was written to remove. The count went from
    38 to 0 and the gate passed. It is recorded here rather than quietly corrected
    because a checker reporting zero is indistinguishable from a clean tree, and
    that is the failure mode this entire family of gates exists to refuse: the
    --self-test below now pins a non-vacuous count so the same mistake fails loudly.

    Raw strings (`r#"..."#`) are tracked too, since their bodies may contain quotes
    and backslashes that mean nothing.
    """
    out = list(text)
    i, n = 0, len(text)
    quote = chr(34)
    bslash = chr(92)
    newline = chr(10)

    def blank(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != newline:
                out[k] = " "

    while i < n:
        c = text[i]
        # raw string: r"..." / r#"..."# / r##"..."##
        if c == "r" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            j = i + 1
            hashes = 0
            while j < n and text[j] == "#":
                hashes += 1
                j += 1
            if j < n and text[j] == quote:
                close = quote + "#" * hashes
                end = text.find(close, j + 1)
                end = n if end < 0 else end + len(close)
                i = end
                continue
        if c == quote:
            j = i + 1
            while j < n:
                if text[j] == bslash:
                    j += 2
                    continue
                if text[j] == quote:
                    break
                j += 1
            i = min(j + 1, n)
            continue
        if c == "'":
            # char literal, or a lifetime (`'a`) which has no closing quote
            j = i + 1
            if j < n and text[j] == bslash:
                j += 2
            elif j < n:
                j += 1
            if j < n and text[j] == "'":
                i = j + 1
                continue
            i += 1
            continue
        if text.startswith("//", i):
            end = text.find(newline, i)
            end = n if end < 0 else end
            blank(i, end)
            i = end
            continue
        if text.startswith("/*", i):
            depth, j = 1, i + 2          # Rust block comments nest
            while j < n and depth:
                if text.startswith("/*", j):
                    depth += 1
                    j += 2
                elif text.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(i, j)
            i = j
            continue
        i += 1
    return "".join(out)


def enclosing_functions(text: str) -> list[tuple[int, str]]:
    """(offset, name) for every function definition, in file order."""
    return [(m.start(), m.group(1)) for m in FN.finditer(text)]


def sites(text: str) -> list[tuple[str, int, str]]:
    """(function, 1-based line, default literal) for every counted site."""
    # Comments and string bodies are masked first: a site quoted in a comment that
    # documents its own removal is not a site. See mask_noncode().
    text = mask_noncode(text)
    fns = enclosing_functions(text)
    found: list[tuple[str, int, str]] = []
    for m in SITE.finditer(text):
        if not is_numeric(m.group(2)):
            continue
        name = "<top level>"
        for off, fname in fns:
            if off > m.start():
                break
            name = fname
        found.append((name, text.count("\n", 0, m.start()) + 1, m.group(2)))
    return found


OPS_RX = r"(?: *(?:<[^<>\s]+>|\[[^\[\]\s]+\]))+"
Q = chr(34)

# `fn cmd_foo(` -- one kshell command per function.
CMD_FN = re.compile(r"^(?:pub )?fn (cmd_[a-z0-9_]+)\s*\(", re.M)

# A match arm at command level: `"sub" => {` or `"sub" | "s" | "disc-mode" => {`.
# The alternatives accept ANY character but a quote. An earlier draft spelled them
# `[a-z0-9_]+`, which cannot match the hyphen in "disc-mode"; that arm header then
# failed to match, the previous arm's span ran on through it, and findings were
# attributed to the wrong subcommand. Arm boundaries decide attribution, so they
# are the one pattern here that must not be approximate.
CMD_ARM = re.compile(
    r"^        (" + Q + r"[^" + Q + r"]+" + Q +
    r"(?:\s*\|\s*" + Q + r"[^" + Q + r"]+" + Q + r")*)\s*=>",
    re.M,
)

# The two shapes kshell prints its own synopsis in:
#   shell_println!("  grant <app> <perm>  Grant permission");
#   shell_println!("Usage: apppermissions grant <app> <perm>");
SYN_SUMMARY = re.compile(
    r"shell_println!\(\s*" + Q + r"  ([a-z0-9_-]+)(" + OPS_RX + r")", re.I
)
SYN_USAGE = re.compile(
    r"shell_println!\(\s*" + Q + r"Usage: \S+ ([a-z0-9_-]+)(" + OPS_RX + r")", re.I
)

# `let host = parts.get(1).copied().unwrap_or("127.0.0.1")` -- name, index, default.
LET_DEFAULT = re.compile(
    r"let (\w+)(?:\s*:\s*[^=]+)? = parts\s*\.\s*get\(\s*(\d+)\s*\)\s*"
    r"(?:\.\s*copied\(\)\s*)?\.\s*unwrap_or\(\s*&?" + Q + r"([^" + Q + r"]*)" + Q + r"\s*\)"
)
IS_EMPTY = re.compile(r"(\w+)\s*\.\s*is_empty\(\)")


def operand_shape(text: str) -> tuple[str, ...]:
    """Per position, `<` if the help prints it required, `[` if optional.

    Only the bracket is kept. Two help lines for one subcommand routinely disagree
    on the placeholder NAME -- `<t1,t2,...>` against `<tag1,tag2,...>` -- while
    agreeing exactly on what is required, and an earlier draft compared the names
    and skipped 490 arms as "ambiguous" for a purely cosmetic difference.
    """
    return tuple(o[0] for o in re.findall(r"<[^<>\s]+>|\[[^\[\]\s]+\]", text))


def promised_sites(text: str) -> list[tuple[str, int, str, str]]:
    """Rule B: (function, line, default, detail) per broken promise.

    An operand the command's own help prints in angle brackets must not have a
    default. This needs no judgment and no ledger of opinions: the tree states the
    requirement and the tree states the violation, so the checker only has to put
    them side by side.

    Three classes are deliberately NOT reported, each for a reason that was
    established by reading the code rather than by reasoning about it:

    * **A default of the empty string.** It is the established spelling for "the
      operator said nothing", and 269 sites pair it with `if x.is_empty() { usage;
      return; }`, which is the correct idiom and the most common thing these
      commands do. Counting it produced a first measurement of 376 that was mostly
      correct code. An empty string also rarely names a real object, so even the
      90 sites that never test it fail in the callee rather than acting on the
      wrong thing.

    * **A default that cannot be reached.** Operands are positional, so if the arm
      refuses when a LATER operand is empty, reaching the work requires that later
      operand, which requires this one. `netshare mount` defaults its remote path
      to "/" and was reported, alarmingly, as turning a missing path into the
      filesystem root -- but success there needs operand 3, which guarantees
      operand 2. The default is dead. That headline was this checker's bug.

    * **An arm whose help disagrees with itself** about whether a position is
      required. Skipped rather than guessed, and a position is checked only where
      every printed synopsis for that subcommand calls it required. A gate that
      overstates gets switched off.
    """
    found: list[tuple[str, int, str, str]] = []
    fns = [(m.start(), m.group(1)) for m in CMD_FN.finditer(text)]
    for i, (start, fname) in enumerate(fns):
        end = fns[i + 1][0] if i + 1 < len(fns) else len(text)
        body = text[start:end]

        synopses: dict[str, set[tuple[str, ...]]] = {}
        for rx in (SYN_SUMMARY, SYN_USAGE):
            for h in rx.finditer(body):
                synopses.setdefault(h.group(1), set()).add(operand_shape(h.group(2)))
        if not synopses:
            continue

        arms = [(a.start(), a.group(1)) for a in CMD_ARM.finditer(body)]
        for j, (astart, header) in enumerate(arms):
            aend = arms[j + 1][0] if j + 1 < len(arms) else len(body)
            arm = body[astart:aend]
            names = re.findall(Q + r"([^" + Q + r"]+)" + Q, header)
            variants: set[tuple[str, ...]] = set()
            for n in names:
                variants |= synopses.get(n, set())
            if not variants:
                continue

            at_index: dict[int, tuple[str, str, int]] = {}
            var_index: dict[str, int] = {}
            for d in LET_DEFAULT.finditer(arm):
                var_index[d.group(1)] = int(d.group(2))
                at_index[int(d.group(2))] = (d.group(1), d.group(3), d.start())
            refused = {var_index[v] for v in IS_EMPTY.findall(arm) if v in var_index}

            for k in range(1, min(len(v) for v in variants) + 1):
                if not all(v[k - 1] == "<" for v in variants):
                    continue                      # some synopsis calls it optional
                if k not in at_index:
                    continue
                _var, default, off = at_index[k]
                if default == "":
                    continue                      # refusable sentinel
                if any(r > k for r in refused):
                    continue                      # unreachable: a later operand is refused
                line = text.count(chr(10), 0, start + astart + off) + 1
                found.append(
                    (fname, line, default, f"{names[0]} operand {k} is printed required")
                )
    return found


def read_ledger() -> tuple[dict[str, int], set[tuple[str, str]], list[str]]:
    """(counts, allowed, raw allow lines).

    `allowed` holds (function, default literal) pairs that are deliberately NOT
    counted. See `--list` and the header for why a blessing is keyed on the
    literal rather than a line number: line numbers in an 120k-line file drift
    every batch, and the literal is the thing that decides whether the default
    names an object or a quantity.
    """
    if not LEDGER.is_file():
        return {}, set(), []
    counts: dict[str, int] = {}
    allowed: set[tuple[str, str]] = set()
    raw: list[str] = []
    for line in LEDGER.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("allow "):
            raw.append(line)
            body = line.split("#", 1)[0].split()
            if len(body) == 3:
                allowed.add((body[1], body[2]))
            continue
        parts = line.split()
        if len(parts) == 2 and parts[1].isdigit():
            counts[parts[0]] = int(parts[1])
    return counts, allowed, raw


def all_sites(text: str) -> list[tuple[str, int, str, str]]:
    """Both rules, deduplicated by line, in file order.

    Rule A (`sites`) asks whether an invented value is a NUMBER. Rule B
    (`promised_sites`) asks whether the command's own help printed the operand as
    REQUIRED. B is the better question -- it needs no judgment and covers string
    defaults, which are 78 of its 83 findings -- but A still earns its place,
    because it reaches arms whose command prints no synopsis at all, and because it
    recognises the `match parts.get(n).unwrap_or(&"0").parse()` form that B's
    `let`-binding matcher does not.

    They are unioned into ONE ledger deliberately. Measured at af70253de the
    overlap is only 3 of 34 and 83, which makes two separate ledgers look safe, but
    that near-disjointness is an artifact of B's matcher rather than a property of
    the populations: broaden it to the `match` form and most of A falls inside B.
    Two ledgers over one family is how a burn-down double-counts, and a count
    nobody can trust is worse than no count because it gets quoted.
    """
    seen: dict[int, tuple[str, int, str, str]] = {}
    for name, line, dflt in sites(text):
        seen[line] = (name, line, dflt, "a numeric default for an absent operand")
    for name, line, dflt, detail in promised_sites(text):
        # Rule B's reason is the more specific of the two, so it wins the label.
        seen[line] = (name, line, dflt, detail)
    return [seen[k] for k in sorted(seen)]


def counted(text: str, allowed: set[tuple[str, str]] | None = None) -> dict[str, int]:
    """Sites per function, excluding blessed (function, literal) pairs."""
    allowed = allowed or set()
    tally: dict[str, int] = defaultdict(int)
    for name, _line, dflt, _detail in all_sites(text):
        if (name, dflt) in allowed:
            continue
        tally[name] += 1
    return dict(tally)


def main() -> int:
    ap = argparse.ArgumentParser(description="absent operands that become numbers")
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--pin", action="store_true", help="write the ledger from the tree")
    args = ap.parse_args()

    if args.selftest:
        return self_test()

    if not KSHELL.is_file():
        print(
            f"check-absent-operand-default: no {KSHELL.name}; nothing to check. "
            "That is not a clean verdict, it is no verdict.",
            file=sys.stderr,
        )
        return 2

    text = KSHELL.read_text(encoding="utf-8", errors="replace")
    found = all_sites(text)

    ledger, allowed, raw_allow = read_ledger()

    if args.list:
        for name, line, dflt, detail in found:
            mark = "  [allowed: its own help prints square brackets]" if (name, dflt) in allowed else ""
            print(
                f"kernel/src/kshell.rs:{line}: {name} defaults a missing operand "
                f"to {dflt} -- {detail}{mark}"
            )
        tally = counted(text, allowed)
        print(
            f"\n{len(found)} site(s); {sum(tally.values())} counted across "
            f"{len(tally)} function(s), {len(found) - sum(tally.values())} allowed"
        )
        return 0

    if args.pin:
        tally = counted(text, allowed)
        LEDGER.write_text(
            LEDGER_HEADER
            + "".join(line + "\n" for line in raw_allow)
            + ("\n" if raw_allow else "")
            + "".join(f"{k} {tally[k]}\n" for k in sorted(tally)),
            encoding="utf-8",
            newline="",
        )
        print(
            f"pinned {sum(tally.values())} site(s) across {len(tally)} function(s); "
            f"{len(raw_allow)} allow line(s) preserved"
        )
        return 0

    tally = counted(text, allowed)
    over: list[str] = []
    under: list[str] = []
    for name in sorted(set(tally) | set(ledger)):
        have, claim = tally.get(name, 0), ledger.get(name, 0)
        if have > claim:
            over.append(f"  {name}: {have - claim} more than the ledger allows ({claim})")
        elif have < claim:
            under.append(f"  {name}: {claim - have} fewer than it claims (fixed? renamed?)")

    # A blessing that matches nothing is the same defect as a count that claims
    # too much, and is how a fixed site stays exempt: the next real site in that
    # function with that literal inherits an exemption written for code that is gone.
    live_pairs = {(name, dflt) for name, _line, dflt, _detail in found}
    stale = sorted(pair for pair in allowed if pair not in live_pairs)

    if over:
        print("A missing operand silently becomes a number in a place the ledger does not allow:")
        for line in over:
            print(line)
        print()
        print("An operand the user did not give is not an operand whose value you may")
        print("choose: `parts.get(n)` returning None means \"you did not say\", and the")
        print("usage line is the answer to it. That is a different question from \"you")
        print("said something I could not read\", which the surrounding match already")
        print("answers by name.")
        print()
        print("Do NOT raise a count to make this pass -- the ledger only shrinks.")
    if under:
        print("Ledger entries claiming more sites than exist -- lower or remove them in")
        print(f"{LEDGER.relative_to(REPO).as_posix()}:")
        for line in under:
            print(line)
    if stale:
        print("`allow` lines matching no site in the tree -- remove them from")
        print(f"{LEDGER.relative_to(REPO).as_posix()}:")
        for fn, dflt in stale:
            print(f"  allow {fn} {dflt}")
        print()
        print("A blessing outlives the code it blessed. Left in place, the next site")
        print("in that function with that default inherits an exemption nobody wrote")
        print("for it -- which is how an object id ends up exempt because a timeout")
        print("used to be.")
    if over or under or stale:
        return 1

    print(
        f"check-absent-operand-default: OK ({sum(tally.values())} site(s) across "
        f"{len(tally)} function(s), all ledgered; {len(allowed)} documented optional)"
    )
    return 0


def self_test() -> int:
    """Cases that pin the definition, not the count.

    The count is the tree's business and changes with every batch. What must not
    drift is *what counts*, because three hand-measurements of this population
    gave three different answers and that is the whole reason this file exists.
    """
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

    # The literal decides it.
    check("a zero default counts", is_numeric("0"), True)
    check("a one default counts", is_numeric("1"), True)
    check("a multi-digit default counts", is_numeric("10"), True)
    check("a signed default counts", is_numeric("-1"), True)
    # And these must not, or the checker reports code that is asking correctly.
    check("an empty default does not count", is_numeric(""), False)
    check("a word default does not count", is_numeric("all"), False)
    check("a mixed default does not count", is_numeric("1O"), False)

    # Attribution: a site is charged to the function it sits in, not the one
    # before it. Getting this wrong would put every count on one entry and the
    # ledger would be a single number again.
    sample = (
        "fn cmd_alpha(x: u32) {\n"
        '    let a = parts.get(1).unwrap_or(&"0");\n'
        "}\n"
        "fn cmd_beta(y: u32) {\n"
        '    let b = parts.get(2).unwrap_or(&"7");\n'
        '    let c = parts.get(3).unwrap_or(&"");\n'
        "}\n"
    )
    got = counted(sample)
    check("each site is charged to its own function", got, {"cmd_alpha": 1, "cmd_beta": 1})

    found = sites(sample)
    check("the line number is the site's", [ln for _n, ln, _d in found], [2, 5])
    check("the default is reported", [d for _n, _l, d in found], ["0", "7"])

    # A site quoted in a comment that documents its own removal is not a site.
    # This is the regression test for the count that was 38 when 37 existed: the
    # 38th was cmd_colortemp's epitaph, a comment quoting the line it had replaced.
    epitaph = (
        "fn cmd_colortemp() {" + chr(10)
        + '    // What it guessed was an absent one -- parts.get(1).unwrap_or(&"1")' + chr(10)
        + "}" + chr(10)
    )
    check("a site in a line comment does not count", counted(epitaph), {})

    block = (
        "fn cmd_h() {" + chr(10)
        + '    /* old: parts.get(1).unwrap_or(&"0") */' + chr(10)
        + "}" + chr(10)
    )
    check("a site in a block comment does not count", counted(block), {})

    nested = (
        "fn cmd_n() {" + chr(10)
        + '    /* outer /* inner parts.get(1).unwrap_or(&"0") */ still out */' + chr(10)
        + "}" + chr(10)
    )
    check("a nested block comment stays masked", counted(nested), {})

    # The other direction, which is the dangerous one. Masking that is too greedy
    # makes the checker report a clean tree, and a clean tree is what a broken
    # checker and a fixed codebase look like from outside. The first draft of
    # mask_noncode blanked string bodies and took the live count from 37 to 0.
    in_string = (
        "fn cmd_s() {" + chr(10)
        + '    let p = "a//b";' + chr(10)
        + '    let a = parts.get(1).unwrap_or(&"0");' + chr(10)
        + "}" + chr(10)
    )
    check("a // inside a string does not hide a later site", counted(in_string), {"cmd_s": 1})

    raw = (
        "fn cmd_r() {" + chr(10)
        + '    let p = r#"quote " and // inside"#;' + chr(10)
        + '    let a = parts.get(1).unwrap_or(&"0");' + chr(10)
        + "}" + chr(10)
    )
    check("a raw string does not hide a later site", counted(raw), {"cmd_r": 1})

    lifetime = (
        "fn cmd_l<'a>(x: &'a str) {" + chr(10)
        + '    let a = parts.get(1).unwrap_or(&"0");' + chr(10)
        + "}" + chr(10)
    )
    check("a lifetime is not an unterminated char literal", counted(lifetime), {"cmd_l": 1})

    # Masking must not move anything: sites() reports line numbers and charges each
    # hit to its enclosing fn, and both are offsets into the text it scanned.
    shifted = (
        "// leading comment" + chr(10)
        + "fn cmd_x() {" + chr(10)
        + '    let a = parts.get(1).unwrap_or(&"0");' + chr(10)
        + "}" + chr(10)
    )
    check("masking preserves line numbers", [ln for _n, ln, _d in sites(shifted)], [3])

    # ---- rule B: the help promised the operand was required -----------------
    #
    # These pin the three deliberate exclusions and the two parser defects that
    # made earlier measurements of this population wrong while looking right.

    def fixture(help_lines: list[str], arms: list[str]) -> str:
        body = ["fn cmd_x(parts: &[&str]) {", "    match sub {"]
        body += arms
        body += ["        _ => {"]
        body += ['            shell_println!("' + h + '");' for h in help_lines]
        body += ["        }", "    }", "}"]
        return chr(10).join(body) + chr(10)

    def arm(name: str, lets: list[str]) -> list[str]:
        return ['        "' + name + '" => {'] + ["            " + l for l in lets] + ["        }"]

    DQ = chr(34)

    def default_let(var: str, idx: int, val: str) -> str:
        return ("let " + var + " = parts.get(" + str(idx) + ").copied().unwrap_or("
                + DQ + val + DQ + ");")

    # A required operand with an invented value is the defect.
    f = fixture(["  grant <app> <perm>"], arm("grant", [default_let("app", 1, "app")]))
    check("a required operand with a default is reported", counted(f), {"cmd_x": 1})

    # An optional one is not. This is the whole reason the criterion is the printed
    # synopsis: `sharesheet history [count]` defaulting to 10 rows is correct.
    f = fixture(["  history [count]"], arm("history", [default_let("n", 1, "10")]))
    check("an optional operand with a default is not reported", promised_sites(f), [])

    # The empty string is the established "the operator said nothing" spelling, and
    # 269 sites pair it with an is_empty() refusal. Counting it gave a first
    # measurement of 376 that was mostly correct code.
    f = fixture(["  grant <app> <perm>"], arm("grant", [default_let("app", 1, "")]))
    check("an empty default is not reported", promised_sites(f), [])

    # Operands are positional, so refusing a LATER one makes this default dead.
    # `netshare mount` defaults a remote path to "/" and was reported, wrongly, as
    # turning a missing path into the filesystem root.
    f = fixture(
        ["  mount <host> <remote> <point>"],
        arm("mount", [
            default_let("host", 1, ""),
            default_let("remote", 2, "/"),
            default_let("point", 3, ""),
            "if host.is_empty() || point.is_empty() { return; }",
        ]),
    )
    check("a default a later refusal makes unreachable is not reported", promised_sites(f), [])

    # ...but the same shape with nothing refused downstream IS reported, or the
    # exclusion above would silently swallow the live case too.
    f = fixture(
        ["  mount <host> <remote> <point>"],
        arm("mount", [default_let("host", 1, ""), default_let("remote", 2, "/")]),
    )
    check("the same default with no later refusal is reported", counted(f), {"cmd_x": 1})

    # THE HYPHEN. An alias pattern of [a-z0-9_]+ cannot match "disc-mode", so this
    # arm header did not match, the previous arm's span ran on through it, and the
    # finding was attributed to the previous subcommand -- reported once as
    # `cmd_bluetooth / name` for code in the discoverable arm.
    f = fixture(
        ["  name <label>", "  disc-mode <on|off>"],
        arm("name", [default_let("label", 1, "")])
        + ['        "disc-mode" | "disc" => {',
           "            " + default_let("on", 1, "on"),
           "        }"],
    )
    got = promised_sites(f)
    check("a hyphenated alias does not leak the arm boundary",
          [(fn, d) for fn, _ln, d, _dt in got], [("cmd_x", "on")])
    check("and the finding names the right subcommand",
          [dt.split()[0] for _f, _l, _d, dt in got], ["disc-mode"])

    # Two help lines for one subcommand routinely differ only in placeholder name.
    # Comparing names skipped 490 arms as ambiguous over a cosmetic difference.
    f = fixture(
        ["  set <path> <t1,t2,...>", "Usage: tag set <path> <tag1,tag2,...>"],
        arm("set", [default_let("p", 1, "."), default_let("t", 2, "")]),
    )
    check("synopses differing only in operand name agree", counted(f), {"cmd_x": 1})

    # But a real disagreement about whether a position is required is skipped, not
    # guessed. A gate that overstates gets switched off.
    f = fixture(
        ["  sync <src> <dst>", "Usage: dirsync sync <src> [dst]"],
        arm("sync", [default_let("src", 1, ""), default_let("dst", 2, "/tmp")]),
    )
    check("a genuine disagreement is skipped", promised_sites(f), [])

    # The `Usage:` form alone must work: some arms print only that.
    f = fixture(
        ["Usage: drvmon register <bus>"],
        arm("register", [default_let("bus", 1, "pci")]),
    )
    check("the Usage: form is recognised on its own", counted(f), {"cmd_x": 1})

    # An arm with no printed synopsis at all is rule A's business, not rule B's.
    f = fixture(["  other <thing>"], arm("undocumented", [default_let("x", 1, "pci")]))
    check("an arm with no synopsis is not rule B's business", promised_sites(f), [])

    # The blessing mechanism. A default of 300 in `screensaver timeout [secs]` is
    # the documented value of an optional operand, not a guess at which saver the
    # operator meant, and the floor of the ledger is therefore not zero.
    mixed = (
        "fn cmd_q() {" + chr(10)
        + '    let n = parts.get(1).unwrap_or(&"300");' + chr(10)
        + '    let i = parts.get(2).unwrap_or(&"0");' + chr(10)
        + "}" + chr(10)
    )
    check("both count with no blessing", counted(mixed), {"cmd_q": 2})
    check(
        "a blessing removes only its own literal",
        counted(mixed, {("cmd_q", "300")}),
        {"cmd_q": 1},
    )
    check(
        "a blessing for another function does not apply",
        counted(mixed, {("cmd_other", "300")}),
        {"cmd_q": 2},
    )
    check(
        "blessing every site leaves the function out entirely",
        counted(mixed, {("cmd_q", "300"), ("cmd_q", "0")}),
        {},
    )

    # Whitespace inside the call must not hide a site: rustfmt can wrap these.
    spaced = 'fn cmd_g() {\n    let a = parts . get( 4 ) . unwrap_or( &"2" );\n}\n'
    check("spacing does not hide a site", counted(spaced), {"cmd_g": 1})

    if failures:
        print(f"\nselftest: {failures} case(s) FAILED", file=sys.stderr)
        return 1
    # Counted, not written down. The first draft of this line said 12 while the
    # suite ran 11, which is the same defect the rest of this file exists to stop:
    # a number in prose that nothing recomputes.
    print(f"\ncheck-absent-operand-default --self-test: OK ({ran} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
