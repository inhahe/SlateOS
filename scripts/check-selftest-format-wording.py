#!/usr/bin/env python3
"""Guard the rule that a module self-test asserts text the kernel can produce.

The rule
--------
**Every fragment a module's `self_test` requires a string to contain must be a
fragment some string literal in the kernel can actually produce.**

Why a second checker, next to `check-selftest-wording.py`
---------------------------------------------------------
The sibling gate guards the same defect for `kshell::self_test`, whose rungs run
a *command* and assert on its captured output.  It resolves the command back
through the shell dispatch table to its `cmd_*` function and asks what that
function can print.  None of that machinery applies to the other 715 files that
have a `self_test`, because they do not go through the shell at all::

    let formatted = format_interface_stats();
    assert!(formatted.contains("RX packets:"), "rx field");

There is no command word to resolve and no `capture_command` to key on, so the
sibling never looked at this line -- and 326 assertions of this shape were
guarded by nothing.  The kernel cannot be tested on the host (`kernel/Cargo.toml`
carries `test = false`, for the reason recorded there), so the only thing that
ever executes one is an eleven-minute QEMU boot, and a stale fragment costs a
whole boot cycle to discover and another to confirm the fix.

That is not a hypothetical either.  It is what happened on 2026-08-27, twice in
one task.  Splitting the network traffic counters by source replaced::

    RX packets: 12  bytes: 900
    TX packets: 40  bytes: 3699

with a table whose column header reads `RX packets` and carries no colon, and
`net::netstat`'s rung 7 still demanded `"RX packets:"`.  `cargo build`,
`cargo clippy` and every static gate in this directory were green; boot test 61
panicked at `netstat.rs:645`.

The second one is the more instructive, because it was *passing*.
`net::dashboard`'s rung 13 asserted the Prometheus endpoint contained
``"os_net_rx_bytes_total "`` -- with a trailing space, to mean "the start of a
sample line".  Once the metric gained a `source` label its samples read
``os_net_rx_bytes_total{source="eth0"} 1180``, with no space after the name, so
that fragment could only ever be matched by the `# HELP` and `# TYPE` lines,
which the endpoint emits whether or not it emits a single sample.  The
assertion went on nodding at output it was no longer inspecting.  A gate that
only caught the panicking direction would have missed it; this one does not,
because it asks the same question of both.

What is checked, and in what order
----------------------------------
A fragment is accepted if either stage accepts it:

1. **It appears verbatim in some produced literal, anywhere in the kernel.**
   Crate-wide on purpose: a module's self-test routinely asserts on text that
   belongs to another module.  `net::netstat` asserts `"ESTABLISHED"`, which is
   `TcpState`'s spelling in a different file, and it is right to.

2. **It is `producible` (the sibling's alignment test) from a format literal in
   the same file.**  This is the case of a fragment that straddles a `{}`:
   `Screen: 800x600` from `"Screen: {}x{}"`, where the digits come from a
   placeholder but `Screen: ` is fixed text no substitution can fake.  Note
   that this stage is *not* satisfied by punctuation alone -- `0.0.0.0:80`
   against `"{}:{}"` aligns on a lone `:` and is correctly refused, because
   `MIN_FIXED_RUN` demands four consecutive fixed bytes.  All 130 such
   fragments in the tree today are explained by their own file, which is why
   this stage is same-file: widening it to the crate makes almost anything
   producible from *some* format string, and a gate that accepts everything
   reports zero findings in exactly the same words as a clean tree.

What the pool is, and the one exclusion that matters
----------------------------------------------------
The pool is every string literal in `kernel/src`, **except those in a
string-testing position** -- the argument of `contains`, `starts_with`,
`ends_with`, `find`, `rfind`, `contains_key`, or `assert_output_contains`.

That exclusion is the whole design.  Without it the assertion's own literal is
in the pool and vouches for itself, and the gate can never report anything.
Excluding by *position* rather than enumerating the producing constructs is
deliberate: a taxonomy of producers (`push_str`, `format!`, `write!`, a bare
literal match arm, a `const &str`, ...) is a list that will be incomplete, and
every omission from it is a *false accusation* against correct code.  Excluding
consumers over-approximates the pool instead, which errs in the only tolerable
direction -- a pool that is too big lets a defect through, a pool that is too
small gets the gate switched off.

What this does NOT catch
------------------------
It does not check that the fragment belongs to *the string under test*.  An
assertion in module A demanding text that only module B produces passes stage 1.
Establishing otherwise needs the call graph the sibling builds, and building one
across 802 files means unioning every same-named helper in the kernel -- at
which scale the reachable-literal pool covers so much that the answer is always
yes.  So this gate is deliberately the weaker, crate-wide property.  It catches
the whole class of *text nobody produces any more* -- a renamed label, a
reformatted table, a dropped colon, a metric that gained a label -- which is the
class that has actually cost boot cycles here.

Exit status: 0 clean, 1 unaccounted assertions found, 2 the gate is broken.
"""

import bisect
import functools
import importlib.util
import pathlib
import re
import string
import sys
import time
from collections import defaultdict
from typing import NamedTuple

_HERE = pathlib.Path(__file__).resolve().parent
ROOT = _HERE.parent
SRC = ROOT / "kernel" / "src"


def _load(name: str, filename: str):
    """Import a hyphenated sibling script."""
    path = _HERE / filename
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:  # pragma: no cover - packaging error
        print(f"error: cannot load {path}", file=sys.stderr)
        raise SystemExit(2)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# `strip_noise` / `find_all_bodies` -- the directory's one self-tested Rust
# scanner.  `producible` / `unescape` / `PLACEHOLDER` / `MIN_FIXED_RUN` come
# from the sibling gate rather than being reimplemented: two copies of an
# alignment rule this subtle would drift, and the drift would be silent.
_rl = _load("check_recursive_locks", "check-recursive-locks.py")
_w = _load("check_selftest_wording", "check-selftest-wording.py")

producible = _w.producible
unescape = _w.unescape
PLACEHOLDER = _w.PLACEHOLDER
N = _w.MIN_FIXED_RUN

# Every Rust string literal form, which the sibling's `LITERAL` is not: it is
# `"(?:[^"\\]|\\.)*"` with no flags, so it matches neither of the two forms this
# file's pool turns out to depend on, and each omission is a batch of false
# accusations rather than a missed defect:
#
#   * a **raw** string. `net::dashboard` writes its whole JSON body as
#     `r#"{{"server":{{"http_running":{},"#`, so without this every one of the
#     43 assertions naming a JSON key was reported as text nothing produces.
#   * a literal broken across lines with a trailing backslash. `\\.` cannot
#     cross a newline without `re.S`, so `httpd`'s response templates --
#     `"HTTP/1.1 200 OK\r\n\` newline `     Content-Length: {}\r\n\` ... -- were
#     invisible, and every header assertion in the file was reported.
#
# `b"…"` and `br#"…"#` are the same strings with a byte type; the assertions
# compare bytes either way, so the prefix is simply consumed.
#
# The prefix is matched with a lookbehind rather than `\b`, because `\b` before
# an optional `b?` is tested between the two characters of `("` -- neither of
# which is a word character -- so it fails, and every plain literal in the tree
# disappears from the pool.
LIT_ANY = re.compile(
    r'(?<![A-Za-z0-9_])b?r(?P<hashes>#*)"(?P<raw>.*?)"(?P=hashes)'
    r'|(?<![A-Za-z0-9_])b?"(?P<esc>(?:[^"\\]|\\.)*)"',
    re.S,
)

# `writeln!`/`println!`/`serial_println!`/... append a newline that appears in
# no literal.  A self-test anchors on it constantly -- `"\nSeccomp:\t0\n"` is
# how `procfs` says "a whole line, not a prefix of a longer one" -- so a pool
# that stops at the literal cannot explain the last byte of such a fragment.
LINE_MACRO = re.compile(r"\b\w*(?:writeln|println)\s*!\s*\(")

# A string-testing position.  Its argument is what the test *demands*, not what
# the kernel *produces*, so its literals must stay out of the pool.
CONSUME = re.compile(
    r"\.\s*(?:contains|starts_with|ends_with|find|rfind|contains_key)\s*\("
    r"|\bassert_output_(?:contains|lacks)\s*\("
)

# `assert!(formatted.contains("RX packets:"), "rx field")` -- and the bare
# `x.contains("...")` inside any other assertion form.  Only a single string
# literal argument is picked up: `contains(&expected)` names a value this
# checker cannot see, and guessing at it would invent findings.
ASSERT_CONTAINS = re.compile(r"\.\s*contains\s*\(\s*(\"(?:[^\"\\]|\\.)*\")\s*\)")

# Polarity.  Not every `contains` in a `self_test` demands the text be present:
# `procfs`'s CPU-count guard is `if cpu_text.contains("processors:") { fail }`,
# which demands the opposite.  For those, "nothing in the tree produces this" is
# the state the test is *enforcing*, not a defect -- so reporting them would be
# accusing a correct regression guard of being stale.  A fragment is only a
# target when the assertion is a *presence* one, which is the case in exactly
# two of the four shapes:
#
#     assert!(x.contains(L))        presence   is_assert, not negated
#     assert!(!x.contains(L))       absence    is_assert, negated
#     if !x.contains(L) { fail }    presence   not is_assert, negated
#     if x.contains(L) { fail }     absence    not is_assert, not negated
#
# -- i.e. `positive = (is_assert != negated)`.
ASSERT_OPEN = re.compile(r"\b(?:debug_)?assert(?:_eq|_ne)?\s*!\s*\(")
IF_KW = re.compile(r"\bif\b")

# What a method's receiver may be made of, walking leftward.  Balanced `)`/`]`
# are jumped whole, so `a.b(c).d[i].contains(...)` resolves back to `a`.
RECEIVER_CHARS = set("_.:&*") | set(string.ascii_letters) | set(string.digits)

# Shorter than a fixed run cannot be judged: `producible` needs MIN_FIXED_RUN
# bytes of fixed text before it will call an alignment real, so a three-byte
# fragment is below the resolution of the question being asked.
MIN_FRAGMENT = _w.MIN_FIXED_RUN

# Assertions whose expected text is right even though this checker cannot
# derive it.  Keyed (file stem, fragment); each needs a reason.  A stale entry
# is reported too -- an exemption that no longer matches is either debt that was
# paid or an exemption now covering something it was never written for.
ALLOWED: dict[tuple[str, bytes], str] = {
    # Every entry below is text the code *computes* -- a formatted number, an
    # expanded variable, an escaped payload, a header pasted together from byte
    # slices -- rather than text some literal spells.  They divide into two
    # mechanisms, and neither can be admitted into the model without switching
    # the gate off:
    #
    # * **Concatenation.**  `http` writes a header as
    #   `buf.extend_from_slice(b"Content-Type: ")` followed by the value, and
    #   `journal` builds its key as `push_str(name); push_str("_hex\":\"")`.
    #   There is no format string, so there is no placeholder to substitute
    #   into.  Accepting "any fragment that splits into pool literals" would
    #   accept the bug this gate exists for: `RX packets:` is `RX packets` plus
    #   a colon, and a colon is a literal somewhere in every large program.
    # * **Arithmetic on the value.**  `3.0 MiB` is a byte count divided down,
    #   `Content-Length: 9` is the body's length, `YWRtaW46c2VjcmV0` is base64
    #   computed at run time, `..leading dot line` is the input with a dot
    #   prepended by the dot-stuffing loop.  No literal can spell these because
    #   they do not exist until the code runs.
    #
    # An exemption is a claim that the assertion is right, checked by hand once.
    # It is not a way to quiet a report: if one of these ever *does* go stale,
    # the boot test catches it, which is the position every assertion in the
    # tree was in before this gate existed.
    ("envvars", b"editor=vim"): "`expand` substitutes $EDITOR; the value is the test's own env var.",
    ("envvars", b"path=/bin"): "`expand` substitutes ${PATH}; the value is the test's own env var.",
    ("journal", b'"from_hex":"'): "Key pasted as push_str(name) + push_str('_hex\":\"') -- no format string.",
    ("journal", b'"path_hex":"'): "Key pasted as push_str(name) + push_str('_hex\":\"') -- no format string.",
    ("statusbar", b"3.0 MiB"): "A byte count scaled to MiB and formatted to one decimal.",
    ("statusbar", b"15 ms"): "The test's own `search_duration_ms`, formatted.",
    ("klog", b'\\"quotes\\"'): "Produced by the JSON escaper a byte at a time, not by a literal.",
    ("klog", b"\\\\backslash"): "Produced by the JSON escaper a byte at a time, not by a literal.",
    ("logpersist", b'"sev":"notice"'): "Key and value written separately; `notice` comes from the level table.",
    ("logpersist", b'"ns":"network.dhcp"'): "Namespace is the caller's string, written after the key.",
    ("logpersist", b'"ip":"10.0.2.15"'): "An address formatted from four octets.",
    ("dashboard", b'"ok"'): (
        "The value is the two-byte literal `ok`, below the four-byte floor the "
        "substitution rule requires -- which is why the sibling values "
        "`\"degraded\"` and `\"critical\"` are accepted and this one is not."
    ),
    ("dashboard", b'os_cpu_total_ticks{cpu="0"}'): "`prom_counter_labeled` substitutes name, label and value into one format string.",
    ("dashboard", b'os_cpu_idle_ticks{cpu="0"}'): "`prom_counter_labeled` substitutes name, label and value into one format string.",
    ("http", b"Content-Type: application/x-www-form-urlencoded"): "Header name is an `extend_from_slice` byte slice; the value is the caller's.",
    ("http", b"Content-Length: 9\r\n"): "The body's length, computed by `build`.",
    ("http", b"Authorization: Basic YWRtaW46c2VjcmV0"): "base64 of `admin:secret`, computed by `basic_auth_creds`.",
    ("netstat", b"10.0.2.15:12345"): "An address and port formatted from the connection table.",
    ("netstat", b"93.184.216.34:80"): "An address and port formatted from the connection table.",
    ("smtp", b"..leading dot line"): "The dot-stuffing loop prepends a dot to the test's own input line.",
    ("smtp", b"...double dots"): "The dot-stuffing loop prepends a dot to the test's own input line.",
    ("pciids", b"8086:ffff"): "Vendor and device IDs formatted as hex when the table has no name.",
}

# `--profile`: per-phase and per-file timings on stderr.  Kept in the script
# rather than reconstructed each time, because the two occasions this gate went
# quiet for minutes were both settled by measurement and neither by inspection.
PROFILE = False


class Finding(NamedTuple):
    path: pathlib.Path
    line: int
    fn: str
    fragment: bytes


class Report(NamedTuple):
    """What one `analyse` run concluded."""

    findings: list[Finding]
    stale: list[tuple[str, bytes]]  # `ALLOWED` keys matching no assertion
    checked: int  # presence assertions considered, exemptions included


class Scan(NamedTuple):
    """One file's contribution: what it can produce, and what it demands."""

    produced: set[bytes]
    fmt_lits: list[bytes]
    targets: list[Finding]


def consumed_spans(struct: str) -> list[tuple[int, int]]:
    """`(open_paren, close_paren)` of every string-testing call in the file.

    Over the *struct* view, so a parenthesis inside a string literal or a
    comment cannot unbalance the depth count.
    """
    spans: list[tuple[int, int]] = []
    for m in CONSUME.finditer(struct):
        open_paren = struct.find("(", m.end() - 1)
        if open_paren < 0:
            continue
        depth = 0
        i = open_paren
        while i < len(struct):
            c = struct[i]
            if c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        spans.append((open_paren, i))
    spans.sort()
    return spans


def literals(code: str):
    """`(start, bytes, newline_terminated)` for every string literal in `code`.

    `newline_terminated` marks the format argument of a `writeln!`-family macro
    -- the first literal inside its parentheses -- because the newline that
    macro appends is part of what the code produces and part of what a self-test
    asserts on.
    """
    macro_opens = [m.end() for m in LINE_MACRO.finditer(code)]
    found: list[tuple[int, int, bytes]] = []
    for m in LIT_ANY.finditer(code):
        if m.group("raw") is not None:
            # A raw string has no escapes at all; its bytes are its text.
            body = m.group("raw").encode("utf-8", "surrogateescape")
        else:
            body = unescape(m.group("esc"))
        found.append((m.start(), m.end(), body))

    # A macro's format argument is the first literal after its `(`.
    first_after = set()
    starts = [s for s, _, _ in found]
    for open_at in macro_opens:
        k = bisect.bisect_left(starts, open_at)
        if k < len(found):
            first_after.add(found[k][0])

    return [(s, b, s in first_after) for s, _, b in found]


def _receiver_start(struct: str, dot: int) -> int:
    """Index of the first byte of the receiver whose `.contains` is at `dot`."""
    i = dot - 1
    while i >= 0:
        c = struct[i]
        if c.isspace():
            i -= 1
            continue
        if c in ")]":
            close, opener = c, "(" if c == ")" else "["
            depth = 0
            while i >= 0:
                if struct[i] == close:
                    depth += 1
                elif struct[i] == opener:
                    depth -= 1
                    if depth == 0:
                        break
                i -= 1
            i -= 1
            continue
        if c in RECEIVER_CHARS:
            i -= 1
            continue
        break
    return i + 1


def asserts_presence(struct: str, dot: int) -> bool:
    """Does the `.contains` at `dot` demand the text be *present*?

    Read over the *struct* view, so a `;` or `{` inside a string literal cannot
    be mistaken for the start of the statement -- otherwise
    `assert!(x.contains("if y;"))` would find structure in its own payload.
    """
    start = _receiver_start(struct, dot)
    j = start - 1
    while j >= 0 and struct[j].isspace():
        j -= 1
    negated = j >= 0 and struct[j] == "!"

    # The enclosing statement: everything back to the nearest `;`, `{` or `}`.
    at = j if negated else start
    stmt = max(struct.rfind(ch, 0, at) for ch in ";{}") + 1
    prefix = struct[stmt:at]

    last_assert = max((m.start() for m in ASSERT_OPEN.finditer(prefix)), default=-1)
    last_if = max((m.start() for m in IF_KW.finditer(prefix)), default=-1)
    is_assert = last_assert > last_if

    return is_assert != negated


def scan_file(path: pathlib.Path, text: str) -> Scan:
    # Two views of the same bytes at the same offsets: `code` keeps literal
    # characters (they are the payload), `struct` blanks them (so braces and
    # parens inside a string cannot be mistaken for structure).
    code = _rl.strip_noise(text, keep_literals=True)
    struct = _rl.strip_noise(text)

    spans = consumed_spans(struct)
    starts = [lo for lo, _ in spans]

    def consumed(at: int) -> bool:
        k = bisect.bisect_right(starts, at) - 1
        return k >= 0 and at <= spans[k][1]

    produced: set[bytes] = set()
    fmt_lits: list[bytes] = []
    for at, lit, newline_terminated in literals(code):
        if consumed(at):
            continue
        if newline_terminated:
            lit += b"\n"
        produced.add(lit)
        # `{{` and `}}` are how a format string spells a single brace, so the
        # JSON a `format!` emits contains braces its literal spells doubled.
        # Added as a second entry rather than in place: a literal that is not a
        # format string means its braces literally, and losing that would be a
        # false accusation in the other direction.
        if b"{{" in lit or b"}}" in lit:
            produced.add(lit.replace(b"{{", b"{").replace(b"}}", b"}"))
        if b"{" in lit:
            fmt_lits.append(lit)

    targets: list[Finding] = []
    for name, body_spans in _rl.find_all_bodies(struct).items():
        if "self_test" not in name:
            continue
        for lo, hi in body_spans:
            for m in ASSERT_CONTAINS.finditer(code, lo, hi):
                frag = unescape(m.group(1)[1:-1])
                if len(frag) < MIN_FRAGMENT:
                    continue
                if not asserts_presence(struct, m.start()):
                    continue
                line = code.count("\n", 0, m.start()) + 1
                targets.append(Finding(path, line, name, frag))
    return Scan(produced, fmt_lits, targets)


def analyse(
    files: list[tuple[pathlib.Path, str]],
    allowed: dict[tuple[str, bytes], str] | None = None,
) -> Report:
    """Findings, plus `allowed` keys that no longer match anything."""
    allowed = ALLOWED if allowed is None else allowed
    t0 = time.perf_counter()
    scans = {path: scan_file(path, text) for path, text in files}
    if PROFILE:
        print(f"scan {time.perf_counter() - t0:.1f}s", file=sys.stderr)

    # One haystack rather than 63k membership tests per fragment: the crate-wide
    # stage runs for every target, and `in` on one large bytes object is a
    # single C-level search.  NUL separates, so a fragment cannot be assembled
    # across two unrelated literals.
    pool: set[bytes] = set()
    for scan in scans.values():
        pool |= scan.produced
    haystack = b"\x00".join(sorted(pool))
    # Only literals long enough to carry a run can justify a placeholder, and
    # restricting the set up front keeps the membership test in `substitutable`
    # off the 60k short ones.
    substitutions = frozenset(lit for lit in pool if len(lit) >= N)

    findings: list[Finding] = []
    used: set[tuple[str, bytes]] = set()
    for path, scan in scans.items():
        t_file = time.perf_counter()
        # Stage 1 first, for the whole file, and only then the per-file index.
        # Nearly every file is settled entirely by the verbatim test -- 313
        # assertions across the tree, of which 68 reach stage 2, from 14 files
        # -- and indexing every format literal of the other ~190 files that have
        # a self-test is most of the run time for no answer.
        pending: list[tuple[Finding, list[bytes]]] = []
        for target in scan.targets:
            key = (path.stem, target.fragment)
            if key in allowed:
                used.add(key)
                continue
            # A leading newline is the *previous* line's terminator, emitted by
            # a different call, so no single literal can ever contain it.  A
            # self-test writes one deliberately -- `"\nSeccomp:\t0\n"` is how
            # `procfs` demands a whole line rather than a prefix of a longer
            # name -- and dropping it costs nothing, because the rest of the
            # fragment still has to be explained in full.
            candidates = [target.fragment]
            if target.fragment.startswith(b"\n"):
                candidates.append(target.fragment[1:])
            if any(c in haystack for c in candidates):
                continue
            pending.append((target, candidates))

        if not pending:
            continue
        t_index = time.perf_counter()
        index = _same_file_index(scan.fmt_lits)
        if PROFILE:
            print(
                f"  {path.name}: {len(pending)} pending,"
                f" {len(scan.fmt_lits)} fmt lits,"
                f" index {time.perf_counter() - t_index:.1f}s",
                file=sys.stderr,
            )
        for target, candidates in pending:
            t_frag = time.perf_counter()
            explained = any(
                _straddles(c, scan.fmt_lits, index, substitutions) for c in candidates
            )
            if PROFILE:
                print(
                    f"    {'ok  ' if explained else 'FIND'}"
                    f" {time.perf_counter() - t_frag:6.1f}s {target.fragment!r}",
                    file=sys.stderr,
                )
            if not explained:
                findings.append(target)
        if PROFILE:
            print(
                f"  {path.name}: total {time.perf_counter() - t_file:.1f}s",
                file=sys.stderr,
            )

    stale = [k for k in allowed if k not in used]
    findings.sort(key=lambda f: (str(f.path), f.line, f.fragment))
    checked = sum(len(scan.targets) for scan in scans.values())
    return Report(findings, stale, checked)


class Index(NamedTuple):
    """One file's format literals, indexed for both acceptance stages."""

    ngram: dict[bytes, set[int]]  # N-gram of a fixed segment -> literals
    whole: dict[bytes, set[int]]  # whole fixed segment -> literals
    seg_last: dict[int, set[int]]  # byte a segment ends with -> literals
    seg_first: dict[int, set[int]]  # byte a segment starts with -> literals


def _same_file_index(fmt_lits: list[bytes]) -> Index:
    """Prefilter indexes, derived from `producible`'s own success condition.

    It succeeds only when the alignment lands `MIN_FIXED_RUN` consecutive
    *fixed* bytes inside the fragment, and there are exactly three places that
    run can come from:

    * a suffix of the segment before the first placeholder, matching at the
      START of the fragment -- so `frag[:N]` lies inside that segment;
    * a prefix of the segment after the last placeholder, matching at the END --
      so `frag[-N:]` lies inside that segment;
    * a segment spanned whole, which must therefore occur whole in the fragment.

    Querying with *every* N-gram of the fragment is also sound but useless: a
    fragment containing four spaces matches thousands of literals, and
    `producible` is quadratic in the segment count.

    `seg_last` / `seg_first` serve the *substitution* stage, whose run comes
    from a placeholder and so leaves the N-gram index blind.  The anchoring is
    still exact, and that is what they exploit: the alignment's leading group is
    a suffix of a segment sitting at the START of the fragment, so if it is
    non-empty that segment ends with `frag[0]`; its trailing group is a prefix
    of a segment at the END, so if non-empty that segment starts with
    `frag[-1]`.  Without this, the four unexplained fragments in `procfs` alone
    would each be tried against all 2704 format literals in the file.
    """
    ngram: dict[bytes, set[int]] = defaultdict(set)
    whole: dict[bytes, set[int]] = defaultdict(set)
    seg_last: dict[int, set[int]] = defaultdict(set)
    seg_first: dict[int, set[int]] = defaultdict(set)
    for lid, lit in enumerate(fmt_lits):
        for seg in PLACEHOLDER.split(lit):
            if not seg:
                continue
            seg_last[seg[-1]].add(lid)
            seg_first[seg[0]].add(lid)
            if len(seg) < N:
                continue
            whole[seg].add(lid)
            for k in range(len(seg) - N + 1):
                ngram[seg[k : k + N]].add(lid)
    return Index(ngram, whole, seg_last, seg_first)


@functools.lru_cache(maxsize=None)
def _segments(lit: bytes) -> tuple[bytes, ...]:
    """`lit`'s fixed text, split at its placeholders."""
    return tuple(PLACEHOLDER.split(lit))


def contains_known(frag: bytes, pool: frozenset[bytes]) -> bool:
    """Does some whole pool literal of length >= N occur inside `frag`?

    The precondition for `substitutable` to have any chance, and cheap to
    decide: a fragment is a few dozen bytes, so enumerating its substrings is a
    couple of thousand set lookups -- against 60k membership tests the other way
    round, or a full pass over every format literal in the file.
    """
    n = len(frag)
    for a in range(n - N + 1):
        for b in range(a + N, n + 1):
            if frag[a:b] in pool:
                return True
    return False


def substitutable(frag: bytes, lit: bytes, pool: frozenset[bytes]) -> bool:
    """`producible`, but a placeholder may be filled by a known literal.

    `producible` insists the alignment be carried by `MIN_FIXED_RUN` bytes of
    the format string's own *fixed* text, and it is right to: without that,
    `"Error: {:?}"` produces `Error: Usage:` and any stale marker is producible
    from any code path that formats anything.

    But a placeholder is not always unknowable.  `net::dashboard` emits every
    metric through::

        fn prom_gauge(buf: &mut String, name: &str, help: &str, value: ...) {
            write!(buf, "# HELP {n} {h}\\n# TYPE {n} gauge\\n{n} {v}\\n", ...)
        }

    so the text `os_http_requests_total ` -- the name, then the space before the
    value -- is spelled by no literal anywhere: the name is `"os_…_total"` at
    the call site and the space belongs to the format string.  Under
    `producible` alone the alignment is carried by that one space, one byte, and
    fifteen correct assertions were reported as text nothing produces.

    So the fragment is accepted when it splits as `A + W + B` where `A` is a
    suffix of the fixed text before some placeholder, `B` is a prefix of the
    fixed text after that same placeholder, and `W` is *itself a whole literal
    from the pool*, at least `MIN_FIXED_RUN` long.

    Whole, not a substring: a substring of some literal is very nearly any
    string, and admitting one would let a fragment supply its own justification.
    That is what still reports `RX packets:` -- no literal in the kernel spells
    it, and `RX packets` is text inside a longer header, not a literal of its
    own, so there is nothing to substitute.

    One placeholder, not several, and `A`/`B` on either side of that same one.
    A fragment carried by the format's *fixed* text is the question `producible`
    already answers, over every pair of segments, reached through the N-gram
    index; this stage exists only for the run a placeholder contributes.
    Enumerating the splits directly rather than through a regex matters for more
    than speed: `.*?` between two alternations commits to one split -- the
    longest `A` and shortest `W` -- and silently gives up on the others.
    """
    segs = _segments(lit)
    if len(segs) < 2:
        return False
    n = len(frag)
    for i in range(len(segs) - 1):
        before, after = segs[i], segs[i + 1]
        # `A` is a suffix of `before` *and* a prefix of the fragment; `B` is a
        # prefix of `after` and a suffix of the fragment.  Both lists are
        # normally one or two entries -- the empty overlap and at most one real
        # one -- so the product below is a handful of lookups, not a search.
        heads = [k for k in range(min(len(before), n) + 1) if before.endswith(frag[:k])]
        tails = [
            k for k in range(min(len(after), n) + 1) if after.startswith(frag[n - k :])
        ]
        for head in heads:
            for tail in tails:
                if head + tail + N > n:
                    continue
                if frag[head : n - tail] in pool:
                    return True
    return False


def _straddles(
    frag: bytes,
    fmt_lits: list[bytes],
    index: Index,
    pool: frozenset[bytes],
) -> bool:
    spans_whole: set[int] = set()
    for seg, lids in index.whole.items():
        if seg in frag:
            spans_whole |= lids

    cands = index.ngram.get(frag[:N], set()) | index.ngram.get(frag[-N:], set())
    cands |= spans_whole
    if any(producible(frag, fmt_lits[lid]) for lid in cands):
        return True

    # The substitution rule's run comes from a placeholder, which the N-gram
    # index -- built from fixed segments -- cannot predict.  Two conditions gate
    # it, both of them necessary rather than heuristic: there has to be a whole
    # literal in the fragment to substitute at all, and the alignment's anchored
    # ends restrict which literals can carry it.
    if not contains_known(frag, pool):
        return False
    sub_cands = spans_whole
    sub_cands |= index.seg_last.get(frag[0], set())
    sub_cands |= index.seg_first.get(frag[-1], set())
    return any(substitutable(frag, fmt_lits[lid], pool) for lid in sub_cands)


_FIXTURE_STALE = '''
//! Fixture: a module self-test demanding text nothing produces.

fn format_interface_stats() -> String {
    let mut out = String::new();
    out.push_str("  source  RX packets       RX bytes\\n");
    out.push_str(&format!("  {}  {:>10}\\n", name, packets));
    out
}

pub fn self_test() {
    let formatted = format_interface_stats();
    assert!(formatted.contains("RX packets:"), "rx field");
}
'''

_FIXTURE_CLEAN = '''
//! Fixture: the same rung, corrected.

fn format_interface_stats() -> String {
    let mut out = String::new();
    out.push_str("  source  RX packets       RX bytes\\n");
    out.push_str(&format!("  {}  {:>10}\\n", name, packets));
    out
}

pub fn self_test() {
    let formatted = format_interface_stats();
    assert!(formatted.contains("RX packets"), "rx column");
}
'''

_FIXTURE_STRADDLE = '''
//! Fixture: a fragment that legitimately straddles a placeholder.

fn format_mode() -> String {
    format!("Screen: {}x{}", width, height)
}

pub fn self_test() {
    let out = format_mode();
    assert!(out.contains("Screen: 800x600"), "mode line");
}
'''

# The other side of the same rule.  An alignment carried by punctuation alone
# must NOT be accepted, or a stale marker becomes producible from any file that
# formats anything -- which is the first bug `producible` had, and the reason it
# reported nothing on the revision it was written to catch.  Here the fragment
# does align against the literal, but only on the lone `:`, one byte short of
# three of the four `MIN_FIXED_RUN` demands.  Kept as a fixture so that raising
# the pool's reach, or lowering that threshold, cannot quietly switch the second
# stage into accepting everything.
_FIXTURE_VACUOUS = '''
//! Fixture: an alignment carried by punctuation alone.

fn format_listener() -> String {
    format!("{}:{}  LISTEN", addr, port)
}

pub fn self_test() {
    let out = format_listener();
    assert!(out.contains("0.0.0.0:80"), "listen addr");
}
'''


# A regression guard asserts text is *absent*, and is correct precisely because
# nothing produces it.  Both shapes appear in `procfs`: the `if ... { panic }`
# form guards the CPU-count format, the `assert!(!...)` form guards the absence
# of a field.  Reporting either would accuse a working guard of being stale, and
# the only way to silence the accusation would be to weaken the guard.
_FIXTURE_ABSENCE = '''
//! Fixture: assertions that text is absent.

fn format_report() -> String {
    format!("cpus: {}", n)
}

pub fn self_test() {
    let out = format_report();
    if out.contains("processors:") {
        panic!("cpu count came from the wrong field");
    }
    assert!(!out.contains("acpi_id_missing"), "no fallback marker");
}
'''

# The same two shapes with the opposite polarity.  Both demand presence, and
# both must still be reported -- otherwise the polarity rule would be a way to
# switch the gate off by rephrasing an assertion.
_FIXTURE_PRESENCE = '''
//! Fixture: the same two shapes, asserting presence.

fn format_report() -> String {
    format!("cpus: {}", n)
}

pub fn self_test() {
    let out = format_report();
    if !out.contains("processors:") {
        panic!("cpu count missing");
    }
    assert!(out.contains("acpi_id_missing"), "fallback marker");
}
'''


def self_test() -> int:
    """Grade the gate against its own fixture before it grades the tree.

    Every way this checker can break -- a `self_test` span cut short, a
    consuming position it stops recognising, an alignment it stops making --
    makes findings *disappear*, and it reports that in the same words as a clean
    tree.  So the fixture is boot test 61's bug in miniature, plus the two
    shapes that must NOT be reported.
    """
    failures = 0

    def check(label: str, condition: bool) -> None:
        nonlocal failures
        if not condition:
            failures += 1
            print(f"[selftest-format-wording self-test] FAIL: {label}", file=sys.stderr)

    stale = pathlib.Path("fixture_stale.rs")
    clean = pathlib.Path("fixture_clean.rs")
    strad = pathlib.Path("fixture_straddle.rs")
    vacuous = pathlib.Path("fixture_vacuous.rs")

    found, _, _ = analyse([(stale, _FIXTURE_STALE)], allowed={})
    check(
        "the stale fragment is reported",
        len(found) == 1 and found[0].fragment == b"RX packets:",
    )

    found, _, _ = analyse([(clean, _FIXTURE_CLEAN)], allowed={})
    check("the corrected rung is not reported", not found)

    found, _, _ = analyse([(strad, _FIXTURE_STRADDLE)], allowed={})
    check("a fragment straddling a placeholder is not reported", not found)

    found, _, _ = analyse([(vacuous, _FIXTURE_VACUOUS)], allowed={})
    check(
        "an alignment carried by punctuation alone is still reported",
        len(found) == 1 and found[0].fragment == b"0.0.0.0:80",
    )

    absence = pathlib.Path("fixture_absence.rs")
    found, _, _ = analyse([(absence, _FIXTURE_ABSENCE)], allowed={})
    check("an assertion of absence is not reported", not found)

    presence = pathlib.Path("fixture_presence.rs")
    found, _, _ = analyse([(presence, _FIXTURE_PRESENCE)], allowed={})
    check(
        "the same two shapes asserting presence are reported",
        sorted(f.fragment for f in found)
        == [b"acpi_id_missing", b"processors:"],
    )

    # The crate-wide stage: the stale fragment becomes legitimate the moment
    # some *other* file produces it.  This is the stage that keeps the gate from
    # accusing `net::netstat` of asserting `TcpState`'s own spelling.
    other = pathlib.Path("other.rs")
    found, _, _ = analyse(
        [(stale, _FIXTURE_STALE), (other, 'fn f() { p("RX packets: {}"); }')],
        allowed={},
    )
    check("another file producing the text clears the finding", not found)

    # A literal in a consuming position must NOT vouch for itself: this is the
    # exclusion the whole design rests on, and losing it silently switches the
    # gate off for the entire tree.
    found, _, _ = analyse(
        [(stale, _FIXTURE_STALE), (other, 'fn f() { if s.contains("RX packets: ") {} }')],
        allowed={},
    )
    check("a literal in a consuming position does not vouch for itself", len(found) == 1)

    # A stale exemption is itself reported.
    _, stale_keys, _ = analyse(
        [(clean, _FIXTURE_CLEAN)], allowed={("clean", b"nothing here"): "reason"}
    )
    check("a stale ALLOWED entry is reported", stale_keys == [("clean", b"nothing here")])

    if failures:
        return 1
    print("[selftest-format-wording self-test] OK")
    return 0


def main(argv: list[str]) -> int:
    global PROFILE
    PROFILE = "--profile" in argv
    if "--self-test" in argv:
        return self_test()

    files = [
        (p, p.read_text(encoding="utf-8", errors="surrogateescape"))
        for p in sorted(SRC.rglob("*.rs"))
    ]
    findings, stale, checked = analyse(files)

    for key in stale:
        print(
            f"stale ALLOWED entry: {key[0]} {key[1]!r} matches no assertion",
            file=sys.stderr,
        )

    for f in findings:
        rel = f.path.relative_to(ROOT) if f.path.is_absolute() else f.path
        print(f"{rel}:{f.line}: {f.fn} asserts {f.fragment!r}", file=sys.stderr)
        print("    no string literal in the kernel can produce it", file=sys.stderr)

    if findings or stale:
        print(
            f"\n{len(findings)} unaccounted assertion(s), "
            f"{len(stale)} stale exemption(s)",
            file=sys.stderr,
        )
        return 1

    print(
        f"[selftest-format-wording] {len(files)} file(s), {checked} presence"
        f" assertion(s) name text the kernel can produce"
        f" ({len(ALLOWED)} allowed)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
