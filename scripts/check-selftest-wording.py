#!/usr/bin/env python3
"""Guard the rule that a self-test rung asserts a rule, not a sentence.

The rule
--------
**Every fragment a `kshell::self_test` rung requires a command's output to
contain must be a fragment that command can actually print.**

Why a checker and not just a code review
----------------------------------------
Because the kernel cannot be tested on the host.  `kernel/Cargo.toml` carries
`test = false` on the `kernel` binary, for the reason recorded there: a
bare-metal binary supplies its own `panic_impl` lang item and cannot be linked
against the host `std`.  So `kshell::self_test` is a `pub fn` run from the boot
battery, and the *only* thing that ever executes a shell assertion is an
eleven-minute QEMU boot.  Every static gate in this directory --
`check-usage-status.py`, `check-option-refusal.py`, `check-query-status.py` and
the rest -- plus `cargo check`, can be green on a tree whose shell self-test
panics.  That is not hypothetical; it is exactly what happened on `adddc7459`,
and the cost was one boot cycle to discover and another to confirm the fix.

What went wrong there is worth stating precisely, because the obvious summary
is the wrong one.  Rung 67 checked that a missing operand is *reported*, by
looping a table of commands and asserting each one's complaint contained
`Usage:`::

    for (bare, control) in arity_cases {
        let out = capture_command(bare);
        assert_output_contains("a missing operand is reported", &out, b"Usage:");

`vd remove` was then converted from `…parse().unwrap_or(0)` to the
`required_id` helper and began saying::

    vd: remove: missing desktop id

which is *strictly better* -- it names the operand instead of reprinting a
synopsis -- and the rung panicked the kernel for the improvement.  The rung was
not wrong about the rule.  It was wrong to spell the rule as a sentence: a
table with one hard-coded marker for nine rows pins the wording of all nine to
whichever wording the first one happened to have.

So the defect class is not "someone typed `Usage:`".  It is **an assertion
whose expected text no longer belongs to the command under test** -- which
covers the stale marker above, a fragment copied from a neighbouring rung, and
a command renamed out from under its rung.  Keying the gate on the word
`Usage:` would put the blind spot back one level down, which is the mistake
`check-usage-status.py` already made once and design-decisions.md §299 exists
to name: a gate's trigger is part of its rule, and a trigger derived from the
wording of the last bug is a syntactic sweep wearing a semantic hat.

What is checked
---------------
For every `assert_output_contains(what, &out, b"<fragment>")` in `self_test`,
resolve the command that produced `out` back through the shell's dispatch table
to its `cmd_*` function, collect every string literal that function -- or
anything it calls inside `kshell.rs` -- passes to a print macro, and require
`<fragment>` to be producible from one of them.

"Producible" treats a `{}` placeholder as arbitrary text and lets the fragment
cover any *part* of the literal, so `` `abc' is not a window id `` is producible
from ``"{}: {}: `{}' is not a {} id"`` -- which is `required_id`'s message, and
is found without this file modelling `required_id` at all, because the call
graph walk reaches it.  Not modelling the helpers is deliberate: a checker that
re-derives what a helper prints will drift away from the helper.

`assert_output_lacks` is checked by the same test for the mirror-image defect.
A `lacks` fragment the command *cannot* print is not a failing assertion, it is
a **vacuous** one: it can never fire, so the rung records a guarantee it is not
providing.  Both directions are reported, separately, because they read
differently to whoever has to fix them.

Two forms of rung are parsed
----------------------------
* the sequential one -- `let out = capture_command("<literal>");` followed by
  the assertions that examine it, up to the next `capture_command`;
* the **table** one -- ``let cases: &[(&str, &str, &[u8])] = &[ … ];`` -- whose
  rows are read directly.  This is the form that broke, and a gate that could
  not read it would be a gate that could not have caught the thing it exists to
  catch.

Matching is done over *statements*, never lines.  `assert_output_contains` is a
four-argument call whose `b"…"` rustfmt is free to put on its own line; see
`statements()` in check-recursive-locks.py for why that distinction has cost
this directory 466 findings once already.

Blind spots, reported rather than hidden
----------------------------------------
A command word that the dispatch table does not resolve (a builtin, a shell
operator, a `capture_command` whose argument is a variable outside a table) is
counted and printed, not silently skipped -- a gate that quietly drops what it
cannot parse is a gate whose coverage nobody knows.

**The largest blind spot is reachability, and it is not fixable here.** This
gate answers "can this command print this text", never "will *this run* print
it".  A `contains` it passes can still fail the boot, because the command took a
different branch than the rung assumed -- most often because a lazily-initialised
subsystem was never initialised.  That is not hypothetical: the first assertion
written on this gate's advice, ``bright set 50`` expecting ``Brightness -> 50%``,
passed the gate and panicked the kernel, because `brightness::init_defaults()`
is called only from the `show` arm and a fresh boot has no display 1 to set.
The rung was missing a `bright show` opener, the way rung 74 opens with
`tile init`.

So the gate narrows what a rung *may* claim; it does not establish that the
claim holds.  A rung still has to set up the state its command needs, and a boot
is still the only thing that proves it did.  Note which way that limitation
points: it yields a rung that fails loudly on a correct kernel, never one that
passes while testing nothing -- the same direction as every other
over-approximation here, and the tolerable one.

Exit status: 0 clean, 1 unaccounted assertions found.
"""

import importlib.util
import pathlib
import re
import sys
from typing import NamedTuple

# `strip_noise` and friends are the directory's one self-tested Rust scanner.
# The filename's hyphens make it un-`import`able normally, hence the spec dance.
_SIBLING = pathlib.Path(__file__).resolve().parent / "check-recursive-locks.py"
_spec = importlib.util.spec_from_file_location("check_recursive_locks", _SIBLING)
if _spec is None or _spec.loader is None:  # pragma: no cover - packaging error
    print(f"error: cannot load {_SIBLING}", file=sys.stderr)
    raise SystemExit(2)
_rl = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_rl)

PATH = pathlib.Path(__file__).resolve().parent.parent / "kernel" / "src" / "kshell.rs"

# `"winsnap" | "wsnap" => cmd_winsnap(args),`
DISPATCH = re.compile(
    r'^\s*("(?:[a-z0-9_.:+-]+)"(?:\s*\|\s*"(?:[a-z0-9_.:+-]+)")*)\s*=>\s*'
    r"(cmd_[a-z0-9_]+)\s*\("
)
WORD = re.compile(r'"([a-z0-9_.:+-]+)"')

# Everything whose first argument reaches the user's terminal or the serial
# log. `serial_*` is included because a rung may assert on either.
PRINT = re.compile(
    r"\b(?:shell_println|shell_print|console_println|console_print"
    r"|serial_println|serial_print|println|print|write|writeln)\s*!\s*\("
)
LITERAL = re.compile(r'"(?:[^"\\]|\\.)*"')

# A `match` arm whose pattern is one or more bare string literals --
# `"remove" =>`, `"create" | "addws" =>`. This is what a kshell command's
# subcommand dispatch looks like, and it is the granularity the narrowing in
# `subcommand_pool` needs.
ARM_HEAD = re.compile(r'((?:"[^"]*"\s*\|\s*)*"[^"]*")\s*=>')

# A call to a same-file function -- free, associated *or* method, and including
# the turbofish form. Two widenings over the shared scanner's `CALL`, each of
# which cost this gate a batch of false accusations:
#
#   * `CALL` stops at `name(`, missing `required_num::<u32>(`. Since the whole
#     operand-refusal burn-down is built on `required_num`, `required_id` and
#     `optional_num`, missing the turbofish meant missing the messages of every
#     command those helpers now speak for -- 30 freshly-passing rungs reported
#     as broken.
#   * `CALL` also refuses a name preceded by `.`, so a method call was invisible.
#     `cut`, `sed`, `fold` and `base64` put every diagnostic they own behind
#     `err.report()`, so their pools held none of their own error wording and
#     every refusal rung in four commands was reported as broken.
#
# Following a method by name alone cannot resolve the receiver's type, so the
# pool is a union over every same-named definition. See `reachable_literals`.
CALLEE = re.compile(r"(?<![\w:])([a-z_][a-z0-9_]*)\s*(?:::\s*<[^<>;{}]*>\s*)?\(")

# The three ways a rung runs a command line and keeps the output. `piped` and
# `plain` are per-rung local `fn`s that wrap `capture_command`; there are 293 of
# them against 251 direct calls, so a checker that knew only the direct form
# would cover the minority of the file.
CAPTURE = re.compile(r"\b(?:capture_command|piped|plain)\s*\(")
# Any other binding of a fresh output buffer. Seeing one and not recognising it
# must *clear* the command in hand: carrying a stale one forward attributes a
# later rung's assertions to an earlier rung's command, which is wrong in both
# directions -- it invents findings and it hides them.
REBIND = re.compile(r"\blet\s+(?:mut\s+)?\w+\s*(?::[^=;]*)?=")
# `let data: &[u8] = b"zz_a:zz_b:zz_c\n";` -- a rung's pipe input, named once and
# fed to a dozen `piped` calls. Without resolving the name, every assertion about
# what came back out of `cut`, `sed` and friends looks like an assertion about
# wording those commands own, and 60-odd correct rungs get reported as broken.
BYTES_LET = re.compile(r"\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=;]*)?=\s*(b\"(?:[^\"\\]|\\.)*\")\s*(?:;|$)")
ASSERT = re.compile(r"\bassert_output_(contains|lacks)\s*\(")
TABLE_DECL = re.compile(r"\blet\s+(\w+)\s*:\s*&\[\(.*?\)\]\s*=\s*&\[")
FOR_LOOP = re.compile(r"\bfor\s+(?:\(([^)]*)\)|(\w+))\s+in\s+(\w+)\b")
IDENT = re.compile(r"^&?\s*(\w+)$")
ROW = re.compile(r"\(([^()]*)\)")
BSTR = re.compile(r'b"((?:[^"\\]|\\.)*)"')
STR = re.compile(r'(?<!b)"((?:[^"\\]|\\.)*)"')

# Assertions whose expected text is right even though this checker cannot
# derive it. Keyed (command word, fragment); each needs a reason, and a stale
# entry is itself reported, because an exemption that no longer matches is
# either debt that was paid or an exemption now covering something it was never
# written for.
#
# All five are the same admission in different clothes: the fragment is not a
# wording any code owns, it is a *value* -- data the command rearranged, or a
# number it formatted. The witness rule catches the ones a filter passes
# through unchanged; these are the ones it takes apart and puts back together,
# and short of implementing `cut` and `sed` inside the checker there is nothing
# left to derive them from.
ALLOWED: dict[tuple[str, bytes], str] = {
    ("sed", b"zz_one zz_one"): (
        "the input is `zz_dup zz_dup` and `s/zz_dup/zz_one/g` rewrites both "
        "occurrences -- output composed from the rung's data by a substitution "
        "the checker does not perform"
    ),
    ("cut", b"zz_a:zz_c"): (
        "fields 3 and 1 of `zz_a:zz_b:zz_c`, rejoined by the delimiter. The "
        "point of the rung is precisely that the pieces come back in line "
        "order, so the expected text exists in no input and no literal"
    ),
    ("cut", b"zz_d"): (
        "character positions 1-3 and 7 of `zz_abcd` spliced together. Same "
        "shape as the entry above, at byte granularity"
    ),
    ("od", b"0000000"): (
        "od's offset column, formatted from a counter -- `{:07o}`-style output "
        "whose fixed part is the empty string"
    ),
    ("scap", b"(1920x1080)"): (
        "`Captured full screen #{} ({}x{})` with the *defaults* substituted in. "
        "The fixed frame around them is `(`, `x`, `)` -- below the fixed-run "
        "floor, and rightly so: nothing distinguishes it from a placeholder "
        "swallowing the fragment whole. The assertion is still worth making, "
        "because it is the only thing pinning `optional_num`'s default to the "
        "documented 1920x1080, so it is paired in the rung with a "
        "`Captured full screen` check the gate *can* derive"
    ),
    ("scap", b"(800x600)"): (
        "the same, for `scap window`'s trailing optional dimensions -- the "
        "control that converting the *id* to `required_id` did not make the "
        "dimensions required too"
    ),
    ("pmgr", b"selftestdisk 4GB"): (
        "`Disk #{}: {} {}GB` filled with two operands from the command line. "
        "The literal frame between them is one space and the two letters `GB`, "
        "which is under the fixed-run floor `producible` needs to tell a real "
        "match from a placeholder swallowing the whole fragment"
    ),
}


def unescape(lit: str) -> bytes:
    """Rust literal escapes, resolved to the **bytes** the literal denotes.

    Bytes, not text, because that is what the assertion compares:
    `assert_output_contains` takes `&[u8]`, and a rung writes an arrow as
    `b"Gap \\xe2\\x86\\x92"` while the command writes it as the character `→`.
    Decoding each escape to a `char` and comparing as text makes those two
    different -- three characters against one -- and the rung is reported as
    asserting something the command cannot print when it is asserting exactly
    what the command does print.

    The line continuation matters more than it looks: `cargo fmt` wraps a long
    message as `"… \\` newline `   …"`, and `statements()` then collapses that
    newline to a space, so the same escape has to be recognised in both the raw
    text and the collapsed one. Hence "backslash followed by whitespace eats
    the whitespace" rather than "backslash-newline".
    """
    out = bytearray()
    i = 0
    simple = {"n": 10, "t": 9, "r": 13, "0": 0, "\\": 92, '"': 34, "'": 39}
    while i < len(lit):
        ch = lit[i]
        if ch != "\\":
            out += ch.encode("utf-8", "surrogateescape")
            i += 1
            continue
        i += 1
        if i >= len(lit):
            break
        esc = lit[i]
        if esc.isspace():
            while i < len(lit) and lit[i].isspace():
                i += 1
            continue
        if esc == "x" and i + 2 < len(lit):
            try:
                out.append(int(lit[i + 1 : i + 3], 16))
                i += 3
                continue
            except ValueError:
                pass
        if esc in simple:
            out.append(simple[esc])
        else:
            out += esc.encode("utf-8", "surrogateescape")
        i += 1
    return bytes(out)


def _ordered_embed(window: bytes, parts: list[bytes]) -> bool:
    """Do `parts` occur inside `window`, in order and without overlapping?

    Leftmost-greedy, which is optimal here: taking the earliest occurrence of
    each part leaves the most room for the ones after it, so if any placement
    exists this finds one.
    """
    pos = 0
    for s in parts:
        if not s:
            continue
        k = window.find(s, pos)
        if k < 0:
            return False
        pos = k + len(s)
    return True


def _shortest_run_head(seg: bytes, frag: bytes) -> int | None:
    """Shortest `a >= MIN_FIXED_RUN` with `frag[:a]` a suffix of `seg`.

    Shortest, because `a` is text consumed from the *start* of the fragment: a
    longer head leaves a smaller window for whatever must follow, and any head
    long enough to satisfy the run rule is as good as any other.
    """
    for a in range(MIN_FIXED_RUN, min(len(seg), len(frag)) + 1):
        if seg.endswith(frag[:a]):
            return a
    return None


def _shortest_run_tail(seg: bytes, frag: bytes) -> int | None:
    """Shortest `b >= MIN_FIXED_RUN` with `frag[-b:]` a prefix of `seg`."""
    n = len(frag)
    for b in range(MIN_FIXED_RUN, min(len(seg), n) + 1):
        if seg.startswith(frag[n - b :]):
            return b
    return None


PLACEHOLDER = re.compile(rb"\{[^{}]*\}")

# How many consecutive bytes of a literal's *fixed* text an alignment must use
# before it counts as a real match. Four, because the fixed text immediately
# around a placeholder is punctuation -- `": "`, `"' "`, `" #"`, `"x"` -- and
# any threshold at or below three lets that punctuation alone vouch for a
# fragment the rest of which a placeholder invented. Four is the first length
# at which the match must include a word.
MIN_FIXED_RUN = 4


def producible(frag: bytes, lit: bytes) -> bool:
    """Can some string the format `lit` produces contain `frag`?

    `lit` is a Rust format string, so `{}` / `{:?}` / `{:<6}` stand for text
    this checker cannot know. A fragment may straddle them -- `` `abc' is not a
    window id`` straddles two -- so the test is not a substring test but an
    alignment: a suffix of one fixed segment, then arbitrary text, then whole
    fixed segments, then a prefix of a later one.

    **The alignment must be carried by fixed text, not by the placeholders.**
    Taken literally, `"Error: {:?}"` can produce the string `Error: Usage:`, so
    a stale `Usage:` marker is "producible" from any command that has an error
    path -- which is not a subtlety, it is the first thing this checker got
    wrong, and it is why it reported nothing on the revision it was written to
    catch. But forbidding a placeholder at the fragment's edges outright is the
    opposite error: `Screen: 800x600` is a perfectly good assertion against
    `"Screen: {}x{}"`, and it *ends* inside a placeholder.

    So the rule is neither a boundary nor a share of the whole, but a *run*:
    somewhere in the alignment there must be `MIN_FIXED_RUN` consecutive bytes
    that came from the literal and not from a placeholder.

    A share of the whole was tried first and is wrong, because the noun in
    ``"{}: {}: `{}' is not a {}"`` is itself a placeholder: the fragment
    `` `abc' is not a horizontal coordinate `` is 36 bytes of which only 12 are
    fixed, and it is a completely sound assertion. What separates it from the
    vacuous `Usage:` is not how much fixed text it has but whether it has a
    *stretch* of it -- here `' is not a `, eleven bytes that no substitution can
    fake. `Usage:` against `"Error: {:?}"` can align only on the empty string,
    and against ``"{}: unknown subcommand '{}'"`` only on a lone `:`.
    """
    segs = PLACEHOLDER.split(lit)
    # The overwhelmingly common case: the fragment lies inside one fixed run.
    for seg in segs:
        if frag and frag in seg:
            return True
    n = len(frag)
    if n == 0:
        return False

    # The alignment is enumerated directly rather than expressed as a regex.
    # The regex form was both ruinously slow and *wrong*. Slow, because a
    # "suffix of `seg`" alternation has one branch per byte of the segment, and
    # `net::dashboard` formats its 16 KiB HTML page from one literal with 48
    # segments -- 1128 segment pairs, each compiling an alternation of thousands
    # of branches, which took minutes for a single fragment. Wrong, because
    # `.*?` between two alternations commits to one split (longest head,
    # shortest gap) and never reconsiders it, so an alignment that exists under
    # a different split was silently missed.
    #
    # Only three placements can ever be worth trying per segment pair, because
    # the run has to come from somewhere:
    #
    #   * a spanned segment carries it -- then head and tail are free, and 0/0
    #     leaves the largest window, so it is the placement most likely to fit;
    #   * the head carries it -- then take the SHORTEST qualifying head and no
    #     tail, again for the largest remaining window;
    #   * the tail carries it -- symmetrically.
    #
    # Each is optimal within its case (a bigger window is a superset of a
    # smaller one), so three tests decide the pair exactly.
    heads = [_shortest_run_head(seg, frag) for seg in segs]
    tails = [_shortest_run_tail(seg, frag) for seg in segs]

    for i in range(len(segs)):
        for j in range(i + 1, len(segs)):
            spanned = segs[i + 1 : j]
            # Segments only accumulate as `j` grows, so one that cannot fit in
            # the fragment rules out every longer span too.
            if spanned and len(spanned[-1]) > n:
                break
            if any(len(s) >= MIN_FIXED_RUN for s in spanned):
                if _ordered_embed(frag, spanned):
                    return True
                continue
            a = heads[i]
            if a is not None and _ordered_embed(frag[a:], spanned):
                return True
            b = tails[j]
            if b is not None and _ordered_embed(frag[: n - b], spanned):
                return True
    return False


def call_args(text: str, open_paren: int) -> list[str]:
    """Top-level comma-separated arguments of the call whose `(` is at `open_paren`.

    Split over the text as given; the callers pass a comment-free view, and a
    comma inside a string literal is stepped over by tracking quote state, so a
    message containing a comma does not split into two arguments.
    """
    args: list[str] = []
    depth = 0
    start = open_paren + 1
    i = open_paren
    in_str = False
    while i < len(text):
        ch = text[i]
        if in_str:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_str = False
            i += 1
            continue
        if ch == '"':
            in_str = True
        elif ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
            if depth == 0:
                args.append(text[start:i])
                return args
        elif ch == "," and depth == 1:
            args.append(text[start:i])
            start = i + 1
        i += 1
    return args


def real_args(text: str, open_paren: int) -> list[str]:
    """`call_args` without the empty tail rustfmt's trailing comma produces.

    A wrapped call is spelled `f(\n  a,\n  b,\n)`, so splitting on top-level
    commas yields a final empty argument. Reading the last element blindly
    therefore found whitespace rather than the `b"…"` for every assertion long
    enough to wrap -- which is 175 of this file's 339, and they are the long
    ones, so the loss was biased towards the most elaborate rungs. Same shape
    as the `statements()` lesson: rustfmt decides the layout, not the author.
    """
    return [a for a in call_args(text, open_paren) if a.strip()]


def as_cmd(lit: str) -> str:
    """A command line, as text -- the one place a literal is *not* bytes.

    Only the first two words are ever used, to find the dispatch entry and the
    subcommand arm, and both are ASCII identifiers.
    """
    return unescape(lit).decode("utf-8", "replace")


def callees(body_struct: str, known: set[str]) -> set[str]:
    """Same-file functions the body calls, turbofish included."""
    return {m.group(1) for m in CALLEE.finditer(body_struct) if m.group(1) in known}


def print_literals(code: str, struct: str, span: tuple[int, int]) -> set[bytes]:
    """Every literal the code in `span` hands to a print macro, as bytes."""
    lits: set[bytes] = set()
    lo, hi = span
    for m in PRINT.finditer(struct, lo, hi):
        args = real_args(code, m.end() - 1)
        for arg in args:
            for lm in LITERAL.finditer(arg):
                lits.add(unescape(lm.group(0)[1:-1]))
    return lits


class Closure:
    """Which literals a call to each same-file function may reach.

    Every definition of a name contributes, because the name is all this
    scanner has: `cut`'s diagnostics live in `fn report` on one error enum and
    `sed`'s in `fn report` on another, and nothing short of type inference says
    which one `err.report()` reaches. Unioning them makes the pool larger than
    the truth, which is the only direction a gate may err in -- a pool that is
    too big lets a real defect through, a pool that is too small accuses
    correct code, and only the second gets the gate switched off.

    A *least fixpoint* over the call graph, by condensing strongly connected
    components and unioning in reverse topological order. The obvious
    depth-first alternative -- recurse, cut the walk off on a repeated name,
    memoise only the entry point -- is both wrong and slow: wrong because every
    member of a cycle then keeps whatever partial answer the entry point
    happened to reach first, and slow because with no per-name cache the same
    subtree is re-walked once per path into it. On the graph with turbofish and
    method calls followed, that is the difference between nine seconds and not
    finishing.

    Sets are carried as integer bitmasks over an interned literal table.
    `|` on a Python int is one machine-word loop rather than a hash-set copy,
    and 20k literals fit in 2.5 KiB, which is what makes a per-name closure
    affordable at all.
    """

    def __init__(
        self,
        bodies: dict[str, list[tuple[int, int]]],
        code: str,
        struct: str,
    ) -> None:
        known = set(bodies)
        self.lits: list[bytes] = []
        bit_of: dict[bytes, int] = {}

        direct: dict[str, int] = {}
        edges: dict[str, set[str]] = {}
        for name, spans in bodies.items():
            mask = 0
            outs: set[str] = set()
            for lo, hi in spans:
                for lit in print_literals(code, struct, (lo, hi)):
                    b = bit_of.get(lit)
                    if b is None:
                        b = len(self.lits)
                        bit_of[lit] = b
                        self.lits.append(lit)
                    mask |= 1 << b
                outs |= callees(struct[lo:hi], known)
            direct[name] = mask
            edges[name] = outs - {name}
        self._bit_of = bit_of

        # Tarjan, iterative: kshell.rs defines thousands of functions and a
        # recursive walk overruns Python's stack long before it runs out of work.
        counter = 0
        idx: dict[str, int] = {}
        low: dict[str, int] = {}
        pending: list[str] = []
        onstack: set[str] = set()
        comp_of: dict[str, int] = {}
        comp_mask: list[int] = []

        for root in bodies:
            if root in idx:
                continue
            idx[root] = low[root] = counter
            counter += 1
            pending.append(root)
            onstack.add(root)
            work = [(root, iter(edges[root]))]
            while work:
                node, it = work[-1]
                descended = False
                for w in it:
                    if w not in idx:
                        idx[w] = low[w] = counter
                        counter += 1
                        pending.append(w)
                        onstack.add(w)
                        work.append((w, iter(edges[w])))
                        descended = True
                        break
                    if w in onstack:
                        low[node] = min(low[node], idx[w])
                if descended:
                    continue
                work.pop()
                if work:
                    low[work[-1][0]] = min(low[work[-1][0]], low[node])
                if low[node] != idx[node]:
                    continue
                cid = len(comp_mask)
                members: list[str] = []
                while True:
                    w = pending.pop()
                    onstack.discard(w)
                    comp_of[w] = cid
                    members.append(w)
                    if w == node:
                        break
                mask = 0
                for w in members:
                    mask |= direct[w]
                # Successors are in already-closed components, so their masks
                # are final. An edge to a component still open would mean the
                # two are mutually reachable, i.e. the same component -- which
                # the low-link test has just ruled out.
                for w in members:
                    for v in edges[w]:
                        c = comp_of.get(v)
                        if c is not None and c != cid:
                            mask |= comp_mask[c]
                comp_mask.append(mask)

        self.mask: dict[str, int] = {n: comp_mask[comp_of[n]] for n in bodies}
        self._decoded: dict[int, set[bytes]] = {}

    def decode(self, mask: int) -> set[bytes]:
        got = self._decoded.get(mask)
        if got is None:
            got = {self.lits[b] for b in range(mask.bit_length()) if mask >> b & 1}
            self._decoded[mask] = got
        return got

    def mask_of(self, lits: set[bytes]) -> int:
        """A mask for literals read straight out of a span, not from a name."""
        mask = 0
        for lit in lits:
            b = self._bit_of.get(lit)
            if b is None:
                b = len(self.lits)
                self._bit_of[lit] = b
                self.lits.append(lit)
            mask |= 1 << b
        return mask

    def of(self, fn: str) -> int:
        return self.mask.get(fn, 0)


def arm_end(struct: str, pos: int, hi: int) -> int:
    """End of the `match` arm whose `=>` ends at `pos`.

    A braced arm ends at its closing brace; a bare-expression arm ends at the
    comma that separates it from the next, or at the brace that closes the
    match if it is the last one. Counted over the struct view, so a comma or
    brace inside a message is not structure.
    """
    i = pos
    while i < hi and struct[i] in " \t\r\n":
        i += 1
    if i < hi and struct[i] == "{":
        depth = 0
        while i < hi:
            if struct[i] == "{":
                depth += 1
            elif struct[i] == "}":
                depth -= 1
                if depth == 0:
                    return i + 1
            i += 1
        return hi
    depth = 0
    while i < hi:
        ch = struct[i]
        if ch in "([{":
            depth += 1
        elif ch in ")]":
            depth -= 1
        elif ch == "}":
            if depth == 0:
                return i
            depth -= 1
        elif ch == "," and depth == 0:
            return i
        i += 1
    return hi


def top_arms(code: str, struct: str) -> list[tuple[set[str], int, int]]:
    """The string-patterned arms of a function's *outermost* `match`.

    Offsets are relative to the strings passed in, which are one function body.
    Only arms at brace depth 1 are collected -- the body itself is depth 0 and
    the subcommand `match` opens depth 1, so depth 1 is exactly the subcommand
    dispatch. A nested `match parts.get(1) { "on" => .., "off" => .. }` inside
    one of those arms sits at depth 3 and is deliberately left alone: narrowing
    to it would need to know which *operand* the rung passed, which the command
    line does not reliably say, and over-narrowing turns this gate into a
    false-positive generator.
    """
    n = len(struct)
    # Brace depth *before* each character, counted over the struct view only.
    depth_at = [0] * (n + 1)
    depth = 0
    for i, ch in enumerate(struct):
        depth_at[i] = depth
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
    depth_at[n] = depth

    arms: list[tuple[set[str], int, int]] = []
    end_so_far = 0
    # The pattern text only survives in `code`, so the scan runs there -- but a
    # `"` cannot be located in `struct`, which blanks the quotes along with the
    # contents. What tells a real arm head from a `=>` inside a message is that
    # the arrow itself survives blanking: in `struct` a literal's `=>` is two
    # spaces, and a genuine arm's is still `=>`.
    for m in ARM_HEAD.finditer(code):
        if m.start() < end_so_far:
            continue  # nested inside an arm already taken
        if struct[m.end() - 2 : m.end()] != "=>":
            continue
        if depth_at[m.start()] != 1:
            continue
        end = arm_end(struct, m.end(), n)
        arms.append((set(WORD.findall(m.group(1))), m.start(), end))
        end_so_far = end
    return arms


def mask_other_arms(code: str, struct: str, sub: str) -> tuple[str, str] | None:
    """Blank the sibling subcommand arms, keeping the one `sub` selects.

    Returns `None` when no arm claims `sub`, which is the signal to fall back
    to the whole function: the command may not dispatch on a subcommand at all,
    or the rung may be deliberately typing one that does not exist. Narrowing
    on a guess would report the fallback cases as findings.

    What survives is the selected arm, everything outside the match (the
    preamble that parses `parts` and the trailer that prints a summary), and
    every arm whose pattern is *not* a bare string list -- `_ =>`,
    `Some(x) =>`, a guard -- because those can run for any subcommand.
    """
    arms = top_arms(code, struct)
    if not any(sub in pats for pats, _, _ in arms):
        return None
    keep_code = list(code)
    keep_struct = list(struct)
    for pats, start, end in arms:
        if sub in pats:
            continue
        for k in range(start, end):
            keep_code[k] = " "
            keep_struct[k] = " "
    return "".join(keep_code), "".join(keep_struct)


def line_starts(text: str) -> list[int]:
    starts = [0]
    for i, ch in enumerate(text):
        if ch == "\n":
            starts.append(i + 1)
    return starts


# A miniature of the file this gate grades, carrying one of each thing that can
# go wrong and one of each thing that must not be flagged.
#
# The fixture is written as the `adddc7459` bug itself: a table of arity cases,
# a loop over it, and a single `b"Usage:"` marker spelled in the loop body that
# is true of one row and false of the other. If this gate ever stops reporting
# the `vd remove` rows, it has stopped being able to catch the only bug it was
# built for -- and it would report that as a clean tree, which is the failure
# mode `--self-test` exists to make loud.
#
# Note `zzecho`, whose whole body is `shell_println!("{}", args)`. Its pool is
# one bare placeholder, so *nothing* is producible from it: every assertion in
# its rung passes only by the witness rule. That makes it the control for the
# widest and least obviously-sound rule in the gate -- if the witness ever stops
# being planted, the fixture's echo rung starts failing and says so.
_SELFTEST_FIXTURE = r'''
fn dispatch_line(cmd: &str, args: &str) {
    match cmd {
        "vd" => cmd_vdesktop(args),
        "zzecho" => cmd_zzecho(args),
        _ => shell_println!("unknown command"),
    }
}

fn required_id(cmd: &str, sub: &str, what: &str, raw: Option<&str>) -> Result<u32, ()> {
    match raw {
        None => {
            shell_println!("{}: {}: missing {} id", cmd, sub, what);
            Err(())
        }
        Some(s) => match s.parse() {
            Ok(v) => Ok(v),
            Err(_) => {
                shell_println!("{}: {}: `{}' is not a {} id", cmd, sub, s, what);
                Err(())
            }
        },
    }
}

fn cmd_vdesktop(args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    match parts.first().copied().unwrap_or("") {
        "remove" => {
            let id = match required_id("vd", "remove", "desktop", parts.get(1).copied()) {
                Ok(v) => v,
                Err(()) => return,
            };
            shell_println!("Removed desktop {}", id);
        }
        "create" => {
            shell_println!("Usage: vd create <name>");
        }
        _ => shell_println!("vd - virtual desktops"),
    }
}

fn cmd_zzecho(args: &str) {
    shell_println!("{}", args);
}

pub fn self_test() {
    {
        let arity_cases: &[(&str, &str, &[u8])] = &[
            ("vd remove", "vd remove 1", b"missing desktop id"),
            ("vd create", "vd create zz_name", b"Usage:"),
        ];
        for (bare, _full, _want) in arity_cases {
            let out = capture_command(bare);
            assert_output_contains("a missing operand is reported", &out, b"Usage:");
        }
    }
    {
        let out = capture_command("vd remove");
        assert_output_contains("a missing id is named", &out, b"missing desktop id");
        assert_output_lacks("and no synopsis is dumped", &out, b"Usage: vd remove");
    }
    {
        let out = capture_command("zzecho zz_witness_token");
        assert_output_contains("echo prints its operand", &out, b"zz_witness_token");
    }
    {
        fn piped(cmd: &str, input: &[u8]) -> Vec<u8> {
            capture_command_with_input(cmd, input)
        }
        let data: &[u8] = b"zz_piped_line\n";
        let out = piped("vd remove", data);
        assert_output_contains("a piped command is still dispatched", &out, b"Usage:");
    }
}
'''


def self_test() -> int:
    """Grade this gate against a fixture with known answers.

    Run with `--self-test`, and run *before* the real pass, because a gate that
    has quietly stopped grading anything reports zero findings in exactly the
    same words as a clean tree. That is not a theoretical worry here: the
    analysis is a call-graph closure over a view of a file that has been blanked
    twice, and every one of its collapse modes -- a missed turbofish, a missed
    method call, a `self_test` span cut short by a brace inside a comment --
    made findings *disappear*, never appear.
    """
    import tempfile

    failures = 0
    tmp = pathlib.Path(tempfile.gettempdir()) / "_selftest_wording_fixture.rs"
    tmp.write_text(_SELFTEST_FIXTURE, encoding="utf-8")
    try:
        # No exemptions: an ALLOWED list written for `kshell.rs` would either
        # mask a fixture finding or report itself stale against the fixture.
        rep = analyse(tmp, {})
    finally:
        tmp.unlink(missing_ok=True)

    if rep is None:
        print("FAIL: no `self_test` body found in the fixture", file=sys.stderr)
        return 1

    got = {(cmd, kind, frag) for _, _, cmd, kind, frag in rep.findings}

    def one(want: tuple[str, str, bytes], why: str) -> None:
        nonlocal failures
        if want not in got:
            failures += 1
            print(f"FAIL: no finding for {why}: expected {want}", file=sys.stderr)

    # The table-driven stale marker, against both spellings of the row it is
    # false of. Both must be reported: the loop asserts it of every cell.
    one(("vd remove", "contains", b"Usage:"), "a stale table marker (bare form)")
    one(("vd remove 1", "contains", b"Usage:"), "a stale table marker (full form)")
    # The mirror-image defect: a `lacks` for text the arm cannot print is a
    # guarantee the rung is not providing.
    one(("vd remove", "lacks", b"Usage: vd remove"), "a vacuous `lacks`")
    # `piped` is a per-rung local wrapper, and there are more of those in
    # kshell.rs than there are direct `capture_command` calls. If it stopped
    # counting as a capture the rung would be attributed to no command at all.
    one(("vd remove", "contains", b"Usage:"), "a stale marker behind `piped`")

    # Controls. Each is correct code, and each is passed by a different rule:
    # arm narrowing keeps `Usage:` legal for `create`, the call-graph closure
    # reaches `required_id`'s wording for `remove`, and the witness rule vouches
    # for text the rung itself planted.
    for cmd, frag, why in (
        ("vd create", b"Usage:", "arm narrowing kept `Usage:` legal for `create`"),
        ("vd create zz_name", b"Usage:", "same, in the row's full form"),
        ("vd remove", b"missing desktop id", "the closure reaches `required_id`"),
        ("zzecho zz_witness_token", b"zz_witness_token", "the witness rule"),
    ):
        if any(c == cmd and f == frag for c, _, f in got):
            failures += 1
            print(f"FAIL: reported correct code -- {why}", file=sys.stderr)

    if rep.stale:
        failures += 1
        print(f"FAIL: stale exemptions reported against no list: {rep.stale}", file=sys.stderr)
    if rep.unresolved:
        failures += 1
        print(f"FAIL: fixture words left undispatched: {rep.unresolved}", file=sys.stderr)
    if rep.supplied_skips != 1:
        failures += 1
        print(
            f"FAIL: expected exactly 1 witness-supplied skip, got {rep.supplied_skips}",
            file=sys.stderr,
        )

    if failures:
        print(f"\n[selftest-wording self-test] {failures} failure(s)", file=sys.stderr)
        return 1
    print("[selftest-wording self-test] OK", file=sys.stderr)
    return 0


class Report(NamedTuple):
    """What one run of `analyse` concluded, kept apart from how it is printed.

    The split exists so `--self-test` can assert on the verdict itself rather
    than on the text of a message. A fixture test that greps stderr passes for
    the wrong reason the day someone rewords an error.
    """

    findings: list[tuple[int, str, str, str, bytes]]
    stale: list[tuple[str, bytes]]
    checked: int
    supplied_skips: int
    unresolved: dict[str, int]
    dispatch: dict[str, str]


def analyse(path: pathlib.Path, allowed: dict[tuple[str, bytes], str] = ALLOWED):
    """Grade every `assert_output_*` in `path`'s `self_test`, or `None` if none.

    `allowed` is a parameter rather than a straight read of the module global so
    the fixture can be graded with no exemptions at all: an exemption list
    written for `kshell.rs` would either mask a fixture finding or be reported
    as stale, and either way the fixture would stop testing what it is for.
    """
    text = path.read_text(encoding="utf-8", errors="surrogateescape")

    # Three views of one file, identically numbered because `strip_noise`
    # blanks in place rather than deleting: `lines` verbatim for reporting,
    # `code` (comments gone, literals kept) for reading messages out of, and
    # `struct` (both gone) as the only thing brackets may be counted over.
    code_text = _rl.strip_noise(text, keep_literals=True)
    struct_text = _rl.strip_noise(text)
    code = code_text.split("\n")
    struct = struct_text.split("\n")

    # Over the *struct* view, never the raw text. `find_bodies` counts braces,
    # and a `{` in a comment or a string literal is not one; feeding it the raw
    # file put `self_test`'s closing brace 700 lines early and quietly dropped
    # every rung after it -- the clean-direction wrongness this directory has
    # now been bitten by three times.
    bodies = _rl.find_all_bodies(struct_text)
    if "self_test" not in bodies:
        return None

    starts = line_starts(text)

    def line_of(off: int) -> int:
        lo, hi = 0, len(starts) - 1
        while lo < hi:
            mid = (lo + hi + 1) // 2
            if starts[mid] <= off:
                lo = mid
            else:
                hi = mid - 1
        return lo

    st_lo, st_hi = bodies["self_test"][-1]
    st_first, st_last = line_of(st_lo), line_of(st_hi)

    # Command word -> cmd_* function. Read from the whole file rather than from
    # the dispatch function's span, because the table is split across several
    # `match` statements and a span-limited read would silently cover only one.
    dispatch: dict[str, str] = {}
    for ln in code:
        m = DISPATCH.match(ln)
        if m:
            for w in WORD.findall(m.group(1)):
                dispatch.setdefault(w, m.group(2))

    closure = Closure(bodies, code_text, struct_text)
    pool_memo: dict[tuple[str, str], set[bytes]] = {}

    def literals_for(word: str, sub: str) -> set[bytes] | None:
        """Literals the invocation `word sub …` can print.

        The narrowing to `sub` is what makes this gate able to see the bug it
        exists for. `vd remove` asserted `Usage:`, and `cmd_vdesktop` as a
        whole prints `Usage:` in plenty of places -- just not in the arm that
        `remove` reaches. Checked against the whole function, the stale marker
        is producible and the gate reports nothing; checked against the arm, it
        is not.
        """
        fn = dispatch.get(word)
        if fn is None:
            return None
        key = (fn, sub)
        if key in pool_memo:
            return pool_memo[key]
        mask = 0
        for lo, hi in bodies.get(fn, ()):
            body_code, body_struct = code_text[lo:hi], struct_text[lo:hi]
            masked = mask_other_arms(body_code, body_struct, sub) if sub else None
            if masked is None:
                mask |= closure.of(fn)
                continue
            mc, ms = masked
            mask |= closure.mask_of(print_literals(mc, ms, (0, len(ms))))
            for callee in callees(ms, set(bodies)):
                if callee != fn:
                    mask |= closure.of(callee)
        pool = closure.decode(mask)
        pool_memo[key] = pool
        return pool

    findings: list[tuple[int, str, str, str, bytes]] = []
    unresolved: dict[str, int] = {}
    checked = 0
    supplied_skips = 0
    seen_allowed: set[tuple[str, bytes]] = set()

    def check(idx: int, cmd_line: str, kind: str, frag: bytes):
        """Check one `assert_output_*(.., frag)` against one command line.

        A fragment drawn from the rung's own *witness* text is skipped rather
        than checked. The witness is everything the rung has put into the
        system in this block: every command line it has run and every byte it
        has piped in -- so `zz_a:zz_b:zz_c` fed to `cut`, the file name
        `zz_full.txt` an earlier line created, the `ZZMSG=` an earlier line
        exported. Such a fragment is not a wording the command owns, and no
        pool of the command's own literals can ever vouch for it: the filter
        commands print *the caller's data*, and the rest of these rungs plant a
        `zz_`- or `selftest`-prefixed witness precisely so that "this line is
        mine" is decidable. Checking them anyway reported 121 findings that
        were all correct code, and a gate that cries wolf on correct code is a
        gate that gets switched off.

        The witness deliberately excludes assertion fragments and table
        markers, which are the very things under test. That is what keeps the
        rule sound for the defect the gate exists for: `Usage:` is a marker,
        never a command line, so the stale one is still checked.
        """
        nonlocal checked, supplied_skips
        words = cmd_line.split()
        word = words[0] if words else ""
        lits = literals_for(word, words[1] if len(words) > 1 else "")
        if lits is None:
            unresolved[word] = unresolved.get(word, 0) + 1
            return
        if frag and frag in witness:
            supplied_skips += 1
            return
        checked += 1
        if (word, frag) in allowed:
            seen_allowed.add((word, frag))
            return
        if any(producible(frag, lit) for lit in lits):
            return
        findings.append((idx + 1, word, cmd_line, kind, frag))

    # `current` is a *list* because of the table-driven rungs: a loop's
    # `capture_command(bare)` stands for every command in the table it iterates,
    # and the whole point of the defect this gate exists for is that a marker
    # spelled once in the loop body has to hold for all of them.
    current: list[str] = []
    byte_lets: dict[str, bytes] = {}
    tables: dict[str, list[str]] = {}
    loop_bindings: set[str] = set()
    loop_cmds: list[str] = []

    # The witness accumulates across a rung and is dropped between rungs. Brace
    # depth over the struct view is what says which: a rung is a `{ ... }` block
    # in `self_test`'s body, so depth 0 is the gap between two of them. Scoping
    # it matters -- a witness that accumulated over all 74 rungs would eventually
    # contain a substring of almost anything and the gate would stop reporting.
    depth_line: dict[int, int] = {}
    d = 0
    for ln in range(st_first + 1, st_last + 1):
        depth_line[ln] = d
        d += struct[ln].count("{") - struct[ln].count("}")

    witness = bytearray()

    def plant(text: bytes) -> None:
        witness.extend(b"\n")
        witness.extend(text)

    for i, stmt in _rl.statements(code, struct):
        if i < st_first or i > st_last:
            continue
        if depth_line.get(i, 0) == 0:
            witness.clear()
            byte_lets.clear()

        tm = TABLE_DECL.search(stmt)
        if tm:
            body = stmt[tm.end() :]
            cells: list[str] = []
            for rm in ROW.finditer(body):
                row = rm.group(1)
                cmds = [as_cmd(c) for c in STR.findall(row)]
                frags = [unescape(f) for f in BSTR.findall(row)]
                cells.extend(cmds)
                for c in cmds:
                    plant(c.encode("utf-8", "surrogateescape"))
                # A row that carries its own marker is resolved here: every
                # string column of a row names the same command with different
                # operands -- one triggers the guard, the other does not -- so
                # each is checked against every marker in the row.
                for c in cmds:
                    for f in frags:
                        check(i, c, "contains", f)
            tables[tm.group(1)] = cells
            current, loop_bindings, loop_cmds = [], set(), []
            continue

        fm = FOR_LOOP.search(stmt)
        if fm:
            name = fm.group(3)
            # A marker spelled *in the loop body* rather than in the table is
            # the shape that panicked the kernel on `adddc7459`: nine rows, one
            # `b"Usage:"`, and the row that stopped saying `Usage:` took the
            # whole boot down. Binding the loop to its table is what lets the
            # marker be checked against every row it is asserted over.
            if name in tables:
                loop_bindings = {
                    b.strip().lstrip("&").strip()
                    for b in (fm.group(1) or fm.group(2) or "").split(",")
                    if b.strip()
                }
                loop_cmds = tables[name]
            else:
                loop_bindings, loop_cmds = set(), []
            current = []
            continue

        cm = CAPTURE.search(stmt)
        if cm:
            args = real_args(stmt, cm.end() - 1)
            arg = args[0].strip() if args else ""
            lit = STR.fullmatch(arg)
            if lit:
                current = [as_cmd(lit.group(1))]
            else:
                ident = IDENT.match(arg)
                current = (
                    loop_cmds if ident and ident.group(1) in loop_bindings else []
                )
            for c in current:
                plant(c.encode("utf-8", "surrogateescape"))
            # Everything after the command line is stdin: `piped(cmd, b"a\nb\n")`
            # written inline, or `piped(cmd, data)` naming a literal bound
            # earlier. Planted so `check` can tell a wording the command owns
            # from the rung's own data coming back out the far side of a filter.
            for a in args[1:]:
                for bm in BSTR.finditer(a):
                    plant(unescape(bm.group(1)))
                ident = IDENT.match(a.strip())
                if ident and ident.group(1) in byte_lets:
                    plant(byte_lets[ident.group(1)])
            # A statement may both capture and assert; fall through.
        else:
            bl = BYTES_LET.search(stmt)
            if bl:
                byte_lets[bl.group(1)] = unescape(bl.group(2)[2:-1])
            if REBIND.search(stmt):
                current = []

        am = ASSERT.search(stmt)
        # Every byte literal in a statement that asserts nothing is setup, not
        # expectation: `Vfs::write_file(p, b"zz_from_file\n")`,
        # `let pipe: &[u8] = b"zz_from_pipe\n"`. Rungs plant witnesses through
        # the VFS as readily as through a command, and a witness is a witness
        # however it got there. Assertions and table rows are excluded because
        # their literals are the very thing under test -- which is what keeps
        # `b"Usage:"` checkable.
        #
        # Plain literals in the same statements are planted too, but only the
        # whitespace-free ones: `Vfs::write_file(Path::new("/tmp/x/zz.txt"), ..)`
        # names a witness, while the third argument of `assert_eq!(last_exit(),
        # 1, "find: an unreadable size is an error")` is prose *about* the
        # command and would blind the gate to half its own vocabulary. Every
        # witness these rungs plant -- a path, a file name, a variable, a
        # command word -- is one token by construction.
        if am is None:
            for bm in BSTR.finditer(stmt):
                plant(unescape(bm.group(1)))
            for sm in STR.finditer(stmt):
                token = unescape(sm.group(1))
                if token and not any(c in b" \t" for c in token):
                    plant(token)

        if am and current:
            args = real_args(stmt, am.end() - 1)
            if len(args) >= 3:
                bm = BSTR.search(args[-1])
                if bm:
                    frag = unescape(bm.group(1))
                    for cmd_line in current:
                        check(i, cmd_line, am.group(1), frag)

    return Report(
        findings=findings,
        stale=sorted(set(allowed) - seen_allowed),
        checked=checked,
        supplied_skips=supplied_skips,
        unresolved=unresolved,
        dispatch=dispatch,
    )


def main(argv):
    if "--self-test" in argv[1:]:
        return self_test()

    # An explicit path is how this checker gets tested against real history: run
    # it on the revision whose boot it would have saved --
    # `git show adddc7459:kernel/src/kshell.rs > /tmp/old.rs` -- and it must
    # report the `vd remove` / `Usage:` row. A checker nobody has watched fail
    # is a checker nobody knows works.
    path = pathlib.Path(argv[1]) if len(argv) > 1 else PATH
    rep = analyse(path)
    if rep is None:
        print("error: no self_test body found", file=sys.stderr)
        return 2
    findings, stale, checked = rep.findings, rep.stale, rep.checked
    supplied_skips, unresolved, dispatch = (
        rep.supplied_skips,
        rep.unresolved,
        rep.dispatch,
    )

    if not findings and not stale:
        # The unresolved words are named, not just counted. A word with no
        # dispatch entry is this checker's blind spot, and a blind spot that
        # reports as a number is one nobody ever looks into -- the same way a
        # gate that undercounts reads as progress.
        blind = ", ".join(
            f"{w or '<empty>'}x{n}" for w, n in sorted(unresolved.items())
        )
        print(
            f"[selftest-wording] kshell.rs: {checked} self-test assertion(s) name text "
            f"their command can print ({len(ALLOWED)} allowed, "
            f"{supplied_skips} echoing the rung's own input"
            + (f", not dispatched: {blind}" if blind else "")
            + ")"
        )
        return 0

    print("", file=sys.stderr)
    if findings:
        print(
            f"{len(findings)} self-test assertion(s) name text the command under test "
            f"cannot print:", file=sys.stderr
        )
        for ln, word, cmd_line, kind, frag in findings:
            why = (
                "the rung fails a correct kernel"
                if kind == "contains"
                else "the assertion can never fire"
            )
            print(f"  {path}:{ln}  `{cmd_line}' -> {dispatch.get(word)}", file=sys.stderr)
            print(f"      assert_output_{kind}(.., {frag!r}) -- {why}", file=sys.stderr)
    if stale:
        print("", file=sys.stderr)
        print(
            "ALLOWED entries that matched nothing (fixed? renamed?) -- remove them:",
            file=sys.stderr,
        )
        for word, frag in stale:
            print(f"  ({word!r}, {frag!r})", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
