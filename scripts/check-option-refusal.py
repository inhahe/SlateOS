#!/usr/bin/env python3
"""Guard the rule that no word the user typed may be dropped or invented.

The rule
--------
**A command-line word is either understood or refused. It is never discarded,
and a value is never invented for one that could not be read.**

design-decisions.md §600 states the rule; this file is the gate for it. The
rule has two halves, and both produce the same failure at the exit status:

* **A dropped word.** The command carries on as though it had not been typed,
  which means it runs a *different* command -- successfully. Almost always a
  wider one, because the dropped word is nearly always a filter or a
  restriction.
* **An invented value.** The word was read, could not be parsed, and a default
  was substituted. Identical in effect, and harder to see, because the default
  is usually a sensible-looking number sitting right there in the source.

Why a checker and not just a code review
----------------------------------------
Because the rule was violated in **nine commands at once** and nobody noticed
until a tenth thing was being fixed nearby. The sweep in `95d407c21` found
that `batch delete --dry-runn a b` deleted `a` and `b` for real -- the typo
matched no flag, so it was not a dry run, and was then filtered out of the
file list, so it was not an error either. It vanished between two pieces of
code that were each individually reasonable.

That is the signature of a rule with no gate: not one dramatic bug, but the
same small omission in every place the shape occurs, each one invisible from
inside the function that has it.

What is checked, and why it is three detectors
----------------------------------------------
§299 says a gate's trigger is part of its rule, and that a trigger derived
from the last bug's spelling is a syntactic sweep wearing a semantic hat. The
honest position here is that the *property* -- "can a word reach the end of
this parser without being either consumed or reported?" -- needs dataflow this
checker does not have. So it approximates the property with three triggers,
and says so rather than claiming completeness:

``D1 guessed-value``
    A word from the command line parsed with a fallback:
    ``s.parse::<T>().unwrap_or(D)``, ``…parse().ok()).unwrap_or(D)``,
    ``…unwrap_or_default()``. This conflates *absent* (where a default is
    right) with *present and unreadable* (where it is a guess). It is the
    exact shape of the `-maxdepth`/`-size`/`--min-size` bugs.

``D2 dropped-word``
    ``.filter(|w| !w.starts_with('-'))`` over an operand list. There is no
    reading of this that is not a silent discard: the word was typed, it was
    not an option the command knows, and it is now gone.

``D3 mute-parser``
    A loop that dispatches on option spellings -- two or more literals
    beginning with ``-`` compared against a word -- and contains no way to
    say no: no non-zero ``set_exit``, and no call to a helper that refuses on
    its behalf. This is the one keyed on the category rather than the shape,
    and it is the one that would have caught all nine.

D3 is the point; D1 and D2 are cheap and exact and were kept because they
catch real instances D3 cannot see (a value guessed *inside* an arm the loop
does handle).

The debt ledger
---------------
D1 has a large standing backlog -- the shape predates the rule by most of the
shell's history. Following §296, the debt is carried *here*, with counts, so
that it is visible, cannot grow, and shrinks only by being fixed: an entry
that matches fewer sites than it claims is itself reported, because that means
the site was fixed (lower the count) or renamed away (the entry is now
exempting something it was never meant to).

A count is per enclosing function, not per line number, because line numbers
drift on every edit and an allowlist that rots is a rubber stamp.

What is matched against, and why it is not a line
-------------------------------------------------
D1 and D2 are both regexes that span method calls, so they are matched against
a **statement** -- `check-recursive-locks.py::statements` -- never against a
line. The author does not decide where a chain's newlines go; `cargo fmt` does,
based on how long the surrounding names happen to be. Matching by the line
published 240 D1 findings and hid 466 more of the identical shape, and, exactly
like the `} else {` bug next door, an undercount has no symptom: the gate
prints a smaller number and looks like progress.

Exit status: 0 clean, 1 unaccounted sites found.
"""

import importlib.util
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from rust_scopes import scope_stack_per_line, classify  # noqa: E402

# `strip_noise` is the directory's one self-tested Rust scanner. The filename's
# hyphens make it un-`import`able normally, hence the spec dance.
_SIBLING = pathlib.Path(__file__).resolve().parent / "check-recursive-locks.py"
_spec = importlib.util.spec_from_file_location("check_recursive_locks", _SIBLING)
if _spec is None or _spec.loader is None:  # pragma: no cover - packaging error
    print(f"error: cannot load {_SIBLING}", file=sys.stderr)
    raise SystemExit(2)
_rl = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_rl)

ROOT = pathlib.Path(__file__).resolve().parent.parent
PATH = ROOT / "kernel" / "src" / "kshell.rs"

# --------------------------------------------------------------------------
# D1 -- a value invented for a word that could not be read.
# --------------------------------------------------------------------------
# `.parse::<T>()` followed, on the same line or after an `.ok()`, by a
# fallback.  The `and_then(|s| s.parse().ok())` form is included because it is
# the same defect wearing an `Option`: `parts.get(2).and_then(…).unwrap_or(20)`
# cannot tell "argument omitted" from "argument unreadable", and answers both
# with 20.
#
# `from_str_radix` is the same defect in a different spelling, and was added on
# 2026-08-29 after it was noticed that two functions with the defect
# (`cmd_pciids`, `cmd_sysrq`) were not in the ledger at all -- not exempted,
# *invisible*. The original pattern required a literal `.parse()`, and
# `u16::from_str_radix(v, 16).unwrap_or(0)` does not contain one, so nine sites
# across five functions were never counted and the burn-down total was an
# undercount by that much.
#
# This is the failure shape the shellcheck floor had (design-decisions §630): a
# gate that is green because it cannot see the thing it is for. Worth stating
# because the fix is not "add a pattern" but "notice that the count is evidence
# about the detector, not only about the code" -- a function that *should*
# appear and does not is the signal, and it is only visible to someone who goes
# looking at a specific function rather than at the total.
D1 = re.compile(
    r"(?:\.parse(?:::<[^>]*>)?\(\)|from_str_radix\()[^;]*?"
    r"\.unwrap_or(?:_default|_else)?\b"
)

# --------------------------------------------------------------------------
# D2 -- a word dropped for beginning with a dash.
# --------------------------------------------------------------------------
D2 = re.compile(r"\.filter\s*\([^)]*!\s*\w+(?:\.\w+\(\))?\.starts_with\(\s*'-'")

# --------------------------------------------------------------------------
# D3 -- a parser that dispatches on option spellings and cannot say no.
# --------------------------------------------------------------------------
# An option spelling as it appears in a comparison or a match arm: a string
# literal whose first character is `-` and which is not merely `-` or `--`
# (those are the end-of-options marker and the stdin convention, and a loop
# that handles only those is not dispatching on options).
OPTION_LITERAL = re.compile(r'"(-{1,2}[A-Za-z][\w-]*)"')
LOOP_OPEN = re.compile(r"^\s*(?:\}\s*)?(?:while|for)\b.*\{\s*$")

# A refusal inside the loop: any non-zero status, or a delegation to a helper
# that carries the refusal.  Helpers are matched by name-shape rather than
# enumerated, because the naming convention in this file is stable
# (`*_parse_*`, `*_split_*`) and a hand-listed set would go stale silently.
REFUSAL = re.compile(
    r"set_exit\(\s*(?!0\s*\))"          # set_exit(<non-zero>)
    r"|end_help_arm\("                   # resolves a catch-all help arm
    r"|\w+_(?:parse|split)_\w*\("        # e.g. backup_parse_flags, grep_split_args
    r"|return Err\("                     # a parser whose contract is a Result
)

# --------------------------------------------------------------------------
# D4 -- an operand read as a number, and dropped in silence when it will not.
# --------------------------------------------------------------------------
# The third shape of §600's defect, and the one that hid longest because it
# leaves *no* trace at all:
#
#     if let Some(id_str) = parts.get(1) {
#         if let Ok(id) = id_str.parse::<u32>() {
#             act_on(id);
#         }
#     } else {
#         shell_println!("Usage: ...");
#     }
#
# An unreadable word falls out of both branches.  Nothing is printed, no
# default is substituted, `set_exit` is never called -- so `ptime enable zzz`
# printed nothing and exited 0, which neither a person nor a script could tell
# from a configuration that really had been enabled.  D1's guessed value is a
# lesser fault than this: a guess at least leaves a wrong number on screen.
#
# The rule is narrow on purpose, and the narrowing is the interesting part.
# The raw shape "`if let Ok(..) = x.parse()` with no `else`" matched 39 sites
# in this file, of which only 22 were defects; the other 17 were *alternatives*
# being tried -- `resolve_container_ref` reads a container reference as an id
# and falls through to a name lookup, `parse_datetime_to_ns` reads epoch
# seconds and falls through to `YYYY-MM-DD`, `execute_select` follows POSIX in
# leaving the variable empty for non-numeric input, and `expand_brace_expr`
# reproduces bash's own `${x:abc}`.  Every one of those was confirmed by
# reading it, not by its shape.
#
# What separates the 22 from the 17 is where the fall-through *goes*: in a
# defect the `if let Ok` is the sole statement of an `if let Some(w) =
# parts.get(N)` block, so control leaves the command having done nothing.  In
# a legitimate alternative there is always something after it to reach.  D4
# therefore requires the nesting *and* the soleness, which is why it can sit
# at zero with no ledger -- the shape it describes has no benign instance.
D4_OUTER = re.compile(r"^\s*(?:\}\s*else\s+)?if let Some\(\s*(\w+)\s*\) = parts\.get\(")
D4_INNER = re.compile(
    r"^\s*if let Ok\(\s*(?:mut\s+)?\w+\s*\) = (\w+)"
    r"(?:\.\w+\(\))*\.parse(?:::<[^>]*>)?\(\)\s*\{\s*$"
)

# --------------------------------------------------------------------------
# Sites that match a detector and are *right* to.  Keyed (function, fragment).
# Each needs a reason; adding one is meant to require saying why.
# --------------------------------------------------------------------------
ALLOWED = {
    # D2: not an operand filter.  `backup restore`'s optional manifest id sits
    # between the positionals and the flags, so it is identified by *not*
    # beginning with a dash -- the dash-leading word is passed on to the flag
    # parser rather than dropped.
    ("cmd_backup", "let manifest_id = parts.get(3).copied().filter"):
        "selects the manifest id; the dash-leading word goes to backup_parse_flags, not the bin",
}

# --------------------------------------------------------------------------
# The D1 backlog, carried with counts (§296).  These are real instances of the
# rule being broken; they are recorded rather than silently exempted so that
# the number can only go down.  See known-issues.md
# `A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`.
#
# Generated by this checker with --ledger; do not hand-edit counts upward
# without a reason, and never add a *new* function to buy silence for new code.
# --------------------------------------------------------------------------


def load_ledger() -> dict:
    """The counted D1 backlog, read from the sidecar file next to this one."""
    ledger = ROOT / "scripts" / "option-refusal-ledger.txt"
    out: dict[str, int] = {}
    if not ledger.exists():
        return out
    for line in ledger.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        fn, _, count = line.rpartition(" ")
        out[fn.strip()] = int(count)
    return out


def outer_fn(stack) -> str:
    """The outermost enclosing function name, or `<top>`."""
    for sc in stack:
        if sc.kind == "fn":
            return sc.name
    return "<top>"


# Comments must never be read as code. Two of the three entries this file's
# ALLOWED table used to carry were doc-comment prose describing the very bug
# the detector looks for -- the explanation of what was fixed tripped the check
# that the fix was still in place. That is a defect in the detector, not an
# exemption to grant: a checker whose only workaround is "stop writing down
# what the bug was" teaches you to delete the explanation, which is the most
# valuable line in the fix. Both entries went when the stripping was added.
#
# That stripping was a line-local `strip_comments` understanding only `//` to
# end of line, one of five hand-rolled Rust scanners in this directory. It now
# uses the one that is self-tested and handles nested `/* */`, raw strings and
# char literals -- see `check-recursive-locks.py::strip_noise`.


def loop_bodies(struct: list[str]):
    """Yield `(open_index, close_index)` for every `while`/`for` block.

    `struct` must have comments *and* literals blanked: a brace in either is
    not structure, and a comment is exactly where an unbalanced one is allowed
    to appear.
    """
    for i, ln in enumerate(struct):
        if not LOOP_OPEN.match(ln):
            continue
        depth = 0
        for k in range(i, min(i + 400, len(struct))):
            s = struct[k]
            depth += s.count("{") - s.count("}")
            if depth <= 0 and k > i:
                yield (i, k)
                break
            if depth <= 0 and k == i and "{" in s and "}" in s:
                yield (i, k)
                break


def close_brace(struct: list[str], line: int, col: int, limit: int = 400):
    """Match the `{` at `(line, col)`; return its `}` as `(line, col)`.

    Character-level, not line-level, and the difference is not pedantry: the
    first version of this counted `{` and `}` per line, so `} else {` netted
    zero and a block with an `else` never appeared to close.  D4 then landed at
    zero on kshell.rs for the wrong reason -- it was skipping every site whose
    outer block had an `else`, which is most of them -- and the fixture below
    is the only thing that said so.  A brace on the same line as its partner is
    common enough in this file that the shortcut was never safe.

    `struct` must have comments and literals blanked, for the reason
    `loop_bodies` gives.
    """
    depth = 0
    for k in range(line, min(line + limit, len(struct))):
        s = struct[k]
        start = col if k == line else 0
        for c in range(start, len(s)):
            if s[c] == "{":
                depth += 1
            elif s[c] == "}":
                depth -= 1
                if depth == 0:
                    return (k, c)
    return None


def silent_operand_sites(struct: list[str]):
    """Yield `(line_index, word_var)` for D4 -- see the D4 comment block."""
    for i, ln in enumerate(struct):
        m = D4_OUTER.match(ln)
        if not m:
            continue
        word = m.group(1)
        brace = ln.find("{", m.end())
        if brace < 0:
            continue
        outer = close_brace(struct, i, brace)
        if outer is None or outer[0] == i:
            continue
        outer_close = outer[0]
        # The inner `if let Ok(..) = <word>.parse…` must be the block's sole
        # statement.  "Sole" is the whole rule: a fall-through that reaches
        # more code is an *alternative* being tried, which is legitimate and
        # is what the other seventeen candidate sites in this file turned out
        # to be.  Only a block that ends right there has nowhere for the
        # unreadable word to go.
        body = [k for k in range(i + 1, outer_close) if struct[k].strip()]
        if not body:
            continue
        inner_line = struct[body[0]]
        inner = D4_INNER.match(inner_line)
        if not inner or inner.group(1) != word:
            continue
        ib = inner_line.rfind("{")
        end = close_brace(struct, body[0], ib)
        if end is None:
            continue
        inner_close, inner_col = end
        if inner_close >= outer_close:
            continue
        # An `else` on the inner block means the unreadable word *is* handled;
        # D4 is only about its absence.  Anything else after the inner block is
        # somewhere for the fall-through to go, which is the legitimate case.
        tail = struct[inner_close][inner_col + 1:].strip()
        after = [k for k in range(inner_close + 1, outer_close) if struct[k].strip()]
        if tail or after:
            continue
        yield (i, word)


def allowed(fn: str, text: str) -> bool:
    return any(fn == f and frag in text for (f, frag) in ALLOWED)


# --------------------------------------------------------------------------
# Self-test.
# --------------------------------------------------------------------------
# D4 landed at zero on the file it was written for, which is the outcome one
# wants and also the outcome an inert detector produces.  §634's lesson --
# *a gate's own output cannot audit its coverage* -- says the only way to tell
# them apart is to name a thing you believe it should catch and check that it
# agrees.  The fixture below is that, in both directions: three shapes that
# must be reported and five that must not, the five being condensed from the
# actual sites that the raw shape matched and that reading proved benign.
_D4_FIXTURE = '''
fn cmd_bad_one(parts: &[&str]) {
    if let Some(id_str) = parts.get(1) {
        if let Ok(id) = id_str.parse::<u32>() {
            act(id);
        }
    } else {
        shell_println!("Usage: x y <id>");
        set_exit(1);
    }
}

fn cmd_bad_two(parts: &[&str]) {
    if let Some(w) = parts.get(2) {
        if let Ok(n) = w.trim().parse::<usize>() {
            act(n);
        }
    }
}

fn cmd_bad_three(parts: &[&str]) {
    if x {
        act(0);
    } else if let Some(w) = parts.get(1) {
        if let Ok(n) = w.parse() {
            act(n);
        }
    }
}

fn cmd_good_has_else(parts: &[&str]) {
    if let Some(id_str) = parts.get(1) {
        if let Ok(id) = id_str.parse::<u32>() {
            act(id);
        } else {
            shell_println!("x: y: `{}` is not an id", id_str);
            set_exit(1);
        }
    }
}

fn cmd_good_falls_through(parts: &[&str]) {
    if let Some(w) = parts.get(1) {
        if let Ok(id) = w.parse::<u32>() {
            if info(id).is_some() {
                return Some(id);
            }
        }
        return by_name(w);
    }
}

fn cmd_good_refuses_after(parts: &[&str]) {
    if let Some(w) = parts.get(1) {
        if let Ok(n) = w.parse::<u32>() {
            act(n);
            return;
        }
        shell_println!("x: y: `{}` is not a number", w);
        set_exit(1);
    }
}

fn cmd_good_not_an_operand(parts: &[&str]) {
    if let Some(w) = other.get(1) {
        if let Ok(n) = w.parse::<u32>() {
            act(n);
        }
    }
}

fn cmd_good_brace_in_literal(parts: &[&str]) {
    if let Some(w) = parts.get(1) {
        shell_println!("{{");
        if let Ok(n) = w.parse::<u32>() {
            act(n);
        }
    }
}
'''

_D4_EXPECT_REPORTED = ["cmd_bad_one", "cmd_bad_two", "cmd_bad_three"]


def self_test() -> int:
    struct = _rl.strip_noise(_D4_FIXTURE).splitlines()
    lines = _D4_FIXTURE.splitlines()
    stacks = scope_stack_per_line(lines)
    hit = {outer_fn(stacks[i]) for i, _ in silent_operand_sites(struct)}

    failures = []
    for fn in _D4_EXPECT_REPORTED:
        if fn not in hit:
            failures.append(f"D4 did not report {fn}, which is the defect it exists for")
    for fn in sorted(hit):
        if fn not in _D4_EXPECT_REPORTED:
            failures.append(f"D4 reported {fn}, which is correct code")
    # `cmd_good_brace_in_literal` is the one control that is about the scanner
    # rather than the rule: the `{{` in the format string is not structure, and
    # a detector that counted it would mis-locate every block after it.
    if failures:
        print("", file=sys.stderr)
        for f in failures:
            print(f"  self-test: {f}", file=sys.stderr)
        return 1
    print(
        f"[option-refusal] self-test: D4 reports all {len(_D4_EXPECT_REPORTED)} "
        f"defective fixtures and none of the "
        f"{len(_D4_FIXTURE.split('fn ')) - 1 - len(_D4_EXPECT_REPORTED)} correct ones"
    )
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    text = PATH.read_text(encoding="utf-8", errors="replace")
    # Three views of one file, identically numbered because `strip_noise`
    # blanks in place: verbatim for reporting, comments-gone/literals-kept for
    # matching (every detector here looks for literal option spellings), and
    # both-gone for the only place braces are counted.
    lines = text.splitlines()
    code_lines = _rl.strip_noise(text, keep_literals=True).splitlines()
    struct = _rl.strip_noise(text).splitlines()
    stacks = scope_stack_per_line(lines)
    rel = str(PATH.relative_to(ROOT)).replace("\\", "/")

    def is_production(i: int) -> bool:
        return classify(stacks[i], rel) == "production"

    guessed: list[tuple[int, str, str]] = []
    dropped: list[tuple[int, str, str]] = []
    mute: list[tuple[int, str, str]] = []
    silent: list[tuple[int, str, str]] = []

    # D1 and D2 are matched per *statement*, not per line, because both regexes
    # span method calls and `cargo fmt` -- not the author -- decides where the
    # newlines fall. Matching by the line published 240 D1 findings and hid 466
    # more of the exact same shape, whose only difference was that the names
    # around them were long enough to make rustfmt wrap the chain. See
    # `statements` for the full argument and known-issues.md
    # `A-KSHELL-THE-OPTION-GATE-COUNTS-ONE-LINE-AND-RUSTFMT-USES-FOUR`.
    for start, stmt in _rl.statements(code_lines, struct):
        if not is_production(start):
            continue
        fn = outer_fn(stacks[start])
        if D1.search(stmt) and not allowed(fn, stmt):
            guessed.append((start + 1, fn, stmt[:96]))
        if D2.search(stmt) and not allowed(fn, stmt):
            dropped.append((start + 1, fn, stmt[:96]))

    for open_i, word in silent_operand_sites(struct):
        if not is_production(open_i):
            continue
        fn = outer_fn(stacks[open_i])
        frag = lines[open_i].strip()
        if allowed(fn, frag):
            continue
        silent.append((open_i + 1, fn, f"{frag}  (`{word}' is dropped when unreadable)"))

    for open_i, close_i in loop_bodies(struct):
        if not is_production(open_i):
            continue
        # Comments are stripped here too, and in both directions: an option
        # spelling quoted in a comment would push a loop over the two-spelling
        # threshold it never reached in code, and the word `set_exit` in a
        # comment would silence a loop that has no refusal at all.
        body = "\n".join(code_lines[open_i:close_i + 1])
        spellings = {m.group(1) for m in OPTION_LITERAL.finditer(body)}
        if len(spellings) < 2:
            continue
        if REFUSAL.search(body):
            continue
        fn = outer_fn(stacks[open_i])
        text = lines[open_i].strip()
        if allowed(fn, text):
            continue
        mute.append((open_i + 1, fn, text[:96]))

    if "--ledger" in argv:
        counts: dict[str, int] = {}
        for _, fn, _ in guessed:
            counts[fn] = counts.get(fn, 0) + 1
        for fn in sorted(counts):
            print(f"{fn} {counts[fn]}")
        return 0

    if "--sites" in argv:
        # `--ledger` says how much debt a function carries; this says *where*,
        # which is what burning it down actually needs.  Without it each batch
        # began by re-deriving the site list with a throwaway script -- a
        # second, slightly-different matcher that could disagree with the gate
        # about what counts, which is the last thing a burn-down wants.  Same
        # `guessed` list the gate grades against, so the two cannot drift.
        #
        # Grouped by function and ordered densest-first, because the shapes
        # co-locate: an arm careless enough to guess one operand usually
        # guesses the next, so a function is a better batch unit than a
        # pattern.
        rest = [a for a in argv[1:] if not a.startswith("-")]
        want = rest[0] if rest else None
        by_fn: dict[str, list[tuple[int, str]]] = {}
        for lineno, fn, text in guessed:
            if want and want not in fn:
                continue
            by_fn.setdefault(fn, []).append((lineno, text))
        for fn in sorted(by_fn, key=lambda f: (-len(by_fn[f]), f)):
            print(f"{fn}  ({len(by_fn[fn])} site(s))")
            for lineno, text in by_fn[fn]:
                print(f"  {rel}:{lineno}  {text}")
        total = sum(len(v) for v in by_fn.values())
        print(f"\n{total} site(s) across {len(by_fn)} function(s)")
        return 0

    ledger = load_ledger()
    seen: dict[str, int] = {}
    unaccounted_guessed = []
    for lineno, fn, text in guessed:
        if seen.get(fn, 0) < ledger.get(fn, 0):
            seen[fn] = seen.get(fn, 0) + 1
            continue
        unaccounted_guessed.append((lineno, fn, text))

    stale = [
        (fn, n - seen.get(fn, 0))
        for fn, n in ledger.items()
        if seen.get(fn, 0) < n
    ]

    problems = bool(unaccounted_guessed or dropped or mute or stale or silent)
    if not problems:
        carried = sum(seen.values())
        print(
            f"[option-refusal] kshell.rs: no word is silently dropped and no new value "
            f"is guessed ({len(ALLOWED)} allowed, {carried} guessed-value site(s) "
            f"carried as known debt across {len(ledger)} function(s))"
        )
        return 0

    print("", file=sys.stderr)
    if mute:
        print(
            f"{len(mute)} option-parsing loop(s) dispatch on option spellings and have "
            f"no way to refuse a word they do not recognise (design-decisions.md §600):",
            file=sys.stderr,
        )
        for lineno, fn, text in mute:
            print(f"  {rel}:{lineno}  {fn}", file=sys.stderr)
            print(f"      {text}", file=sys.stderr)
    if silent:
        print("", file=sys.stderr)
        print(
            f"{len(silent)} operand(s) are read as a number and dropped in silence when "
            f"the word will not read -- nothing printed, no default, no non-zero exit "
            f"(design-decisions.md §600). Use `readable_num`/`readable_hex`:",
            file=sys.stderr,
        )
        for lineno, fn, text in silent:
            print(f"  {rel}:{lineno}  {fn}", file=sys.stderr)
            print(f"      {text}", file=sys.stderr)
    if dropped:
        print("", file=sys.stderr)
        print(
            f"{len(dropped)} site(s) drop a command-line word for beginning with a dash:",
            file=sys.stderr,
        )
        for lineno, fn, text in dropped:
            print(f"  {rel}:{lineno}  {fn}", file=sys.stderr)
            print(f"      {text}", file=sys.stderr)
    if unaccounted_guessed:
        print("", file=sys.stderr)
        print(
            f"{len(unaccounted_guessed)} NEW site(s) invent a value for a word that "
            f"could not be read (the ledger accounts for the pre-existing ones):",
            file=sys.stderr,
        )
        for lineno, fn, text in unaccounted_guessed:
            print(f"  {rel}:{lineno}  {fn}", file=sys.stderr)
            print(f"      {text}", file=sys.stderr)
    if stale:
        print("", file=sys.stderr)
        print(
            "Ledger entries that matched fewer sites than they claim (fixed? renamed?) "
            "-- lower or remove the count in scripts/option-refusal-ledger.txt:",
            file=sys.stderr,
        )
        for fn, missing in stale:
            print(f"  {fn}: {missing} fewer than expected", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
