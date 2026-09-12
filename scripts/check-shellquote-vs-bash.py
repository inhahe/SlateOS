#!/usr/bin/env python3
"""Cross-check kernel/src/shellquote.rs's rules against real bash.

Ports the Rust scanner byte-for-byte, then asks bash what it actually does
with the same input.  A disagreement means the Rust is wrong, not bash.

Requires WSL; see `bashprobe.py` for why that keeps it out of the boot test.

**The port is the weak point, and is guarded.**  A hand copy of the scanner
can drift away from the Rust it claims to model, and a drifted copy reports
"0 disagreements" about a scanner that no longer exists -- worse than no
checker, because it looks like evidence.  `assert_port_matches_rust()` reads
`shellquote.rs` and refuses to run unless three things still hold: the set of
quoting contexts is the set `scan()` models, the escape alphabet is the one
ported, and the whole of what is ported still hashes to `PORTED_DIGEST`.

That third layer replaced a single `DQ_ESCAPABLE` regex on 2026-09-12, after
lane A showed what a one-table guard buys.  They gave the Rust scanner a
fourth context, `Ctx::DollarSingle`, so that `echo $'hi'` stopped printing
`$hi`.  The change does not touch `DQ_ESCAPABLE`, so the guard passed, the
port kept three contexts, and any `$'...'` case would have been graded against
semantics that no longer shipped.  The alphabet had been serving as a proxy
for "the scanner" and quietly stopped being one.  See
`requests/a-b-shellquote-port-has-drifted-and-the-guard-cannot-see-it.md`.

WHAT THIS GATE DOES NOT GRADE, STATED PLAINLY
---------------------------------------------
It does not grade the scanner, and the digest does not change that -- it
changes only whether a moved scanner is noticed.  Nothing here executes a line
of Rust; `scan()` below is a hand port, so a *defect* in the real scanner is
still something this file cannot see.  What it can now see is that the real
scanner has moved, which is the difference between "wrong answer" and "answer
about the wrong question".

Measured, not assumed, both before and after.  Replacing the `Ctx::Single`
arm's `let structural = b == b'\''` with `let structural = false` -- a scanner
in which a single-quoted string can never close -- used to leave this file
printing `0 failure(s)`, exiting 0, and passing its own self-test.  As of the
digest it refuses to run at all, naming the change as a rule inside an
existing context.  Refusing is not grading: this file still cannot tell you
that mutation is *wrong*, only that it is not what was ported.

Telling you it is wrong is already covered where it belongs:
`shellquote.rs::self_test()` runs those rungs at boot, and the mutation above
fails the rung at shellquote.rs:568.  The boot test is what grades the
implementation.  This gate exists for the question the rungs *cannot* answer
-- whether what they assert is what a real shell does, since a rung whose
expectation was transcribed wrongly is confidently, permanently green.

So, concretely, this file answers four questions and no others:

  1. is the Rust scanner still the one ported here -- contexts, escape
     alphabet, and the digest of all three ported regions
     (`assert_port_matches_rust`);
  2. is every rung of the three graded functions either checked here or
     explicitly excused (`assert_every_rung_is_accounted_for`) -- the check
     that would have noticed rungs nobody had transcribed;
  3. do the `strip_quotes` rungs assert what real bash produces, comparing
     three ways: the expectation read out of shellquote.rs, the transcription
     in `STRIP_RUNGS`, and bash itself;
  4. does the ported scanner agree with bash on `CASES`, and with the rungs on
     the byte offsets in `OFFSET_RUNGS` -- which bash cannot be asked, because
     "at which byte does the first bare `>` occur" is not a question a shell
     exposes an answer to.  Question 4's offsets are therefore graded against
     the port, which is weaker than bash and is labelled as such below.
"""
import contextlib
import hashlib
import io
import pathlib
import re
import sys
import typing

import bashprobe
import rustrungs

UNQ, SGL, DBL, DSQ = "U", "S", "D", "A"
DQ_ESCAPABLE = set(b'"\\$`\n')
BACKSLASH = 0x5C

RUST = pathlib.Path(__file__).resolve().parent.parent / "kernel" / "src" / "shellquote.rs"

# A blessed state of `shellquote.rs`: the declarations this file hand-ports,
# the quoting contexts `scan()` models, and why this state is acceptable.
#
# Named regions rather than the whole file, because the rest of
# `shellquote.rs` is consumers of the scanner (`find_bare`, `quote_word`,
# `self_test`) which this file either ports separately or does not claim to
# model at all, and pinning them would fire on edits that cannot affect a
# single result here.
class _State(typing.NamedTuple):
    regions: tuple[str, ...]
    ctx: frozenset[str]
    label: str
    note: str


_SCANNER = ("pub enum Ctx", "const DQ_ESCAPABLE", "impl Iterator for QuoteScan")

# WHY A DIGEST AND NOT A THIRD HAND-PICKED TABLE.  Until 2026-09-12 the entire
# tether between this file and `shellquote.rs` was the `DQ_ESCAPABLE` regex,
# and lane A demonstrated what that buys: they gave the Rust scanner a fourth
# context, `Ctx::DollarSingle`, so that `echo $'hi'` stopped printing `$hi`.
# The change does not touch `DQ_ESCAPABLE`.  So the guard passed, the port kept
# three contexts, and every `$'...'` case would have been graded against
# semantics that no longer shipped -- `0 failure(s)` about the wrong scanner,
# filed as `requests/a-b-shellquote-port-has-drifted-and-the-guard-cannot-
# see-it.md`.  The alphabet had been serving as a proxy for "the scanner" and
# quietly stopped being one; a proxy is exactly what a digest is not.
#
# The cost is honest and worth naming: an edit to a trailing comment inside
# these regions fires this guard, because the stripper drops whole-line
# comments only and does not parse Rust string literals to find the others.
# Re-blessing is one command (`--print-digest`) and a re-read of `scan()`,
# which is the re-read the drift above shows is needed anyway.
#
# ONE ENTRY IS THE NORMAL STATE.  Two only while a change is in flight across
# lanes, and never for longer -- an extra entry is a declaration, and a
# declaration that has stopped being true is the failure this tree spends its
# days pulling out of documents.  The port below models the CURRENT state; a
# run against an older one prints, loudly, what its results therefore do not
# cover.  See `_transition_warning`.
PORTED_STATES = {
    "c6e289003e50b572e592c6a6e36ef6c142d38aa7a4ca3891a8804e5f1ec1cf55": _State(
        regions=_SCANNER,
        ctx=frozenset({"Unquoted", "Single", "Double"}),
        label="OUTGOING (three contexts, no $'...')",
        note=(
            "main's scanner before lane A's DollarSingle work merges "
            "(kernel/src/shellquote.rs @ 3650a966c, 2026-09-12). DELETE THIS "
            "ENTRY once origin/main carries the four-context scanner: keeping "
            "it would let a revert of that merge pass unnoticed, which is the "
            "whole failure this guard exists to stop."
        ),
    ),
    "d3a64dc75a0a19652d62bd63aa357029eb14ecc707f65224154261765b2785c4": _State(
        # `fn hex_val`, not `const fn hex_val`. Lane A dropped the `const` when
        # `b - b'0'` tripped `arithmetic_side_effects` and the repair became
        # `char::from(b).to_digit(16)`, which is not const. The shorter key
        # also matches `const fn hex_val` as a substring, so it survives the
        # `const` coming back -- a region key should name the thing, not its
        # qualifiers.
        regions=_SCANNER + ("fn hex_val", "fn decode_ansi_c"),
        ctx=frozenset({"Unquoted", "Single", "Double", "DollarSingle"}),
        label="CURRENT (four contexts, ANSI-C quoting)",
        note=(
            "lane A's scanner at 53c84786f, read from origin/lane-a rather "
            "than from a pasted description -- a transcription is exactly the "
            "failure mode this digest exists to catch, and manufacturing it "
            "to prove the guard works would be silly. They sent the digest "
            "too; it was recomputed here from their branch and agreed, which "
            "is corroboration rather than a second copy of one number. "
            "`hex_val` and `decode_ansi_c` are pinned because the decode "
            "table is part of what this file ports, and a table left outside "
            "the digest is the 2026-09-12 defect one level down."
        ),
    ),
}

# The state the port below actually models.  Used for diagnosis when nothing
# matches, and to decide whether a run needs the transition warning.
CURRENT_DIGEST = "d3a64dc75a0a19652d62bd63aa357029eb14ecc707f65224154261765b2785c4"
PORTED_CTX = PORTED_STATES[CURRENT_DIGEST].ctx

# Char and string literals, removed before brace counting so that a `b'{'` in
# the Rust cannot unbalance the region extractor.  Rust lifetimes (`'_`, `'a`)
# look like unterminated char literals and therefore do not match, which is the
# behaviour wanted -- `impl Iterator for QuoteScan<'_> {` must keep its brace.
_RUST_LITERAL = re.compile(r"b?'(?:\\.|[^'\\])*'|b?\"(?:\\.|[^\"\\])*\"")


def _normalise(lines: list[str]) -> str:
    """Drop whole-line comments and attributes; collapse runs of whitespace."""
    kept = []
    for line in lines:
        bare = line.strip()
        if not bare or bare.startswith("//") or bare.startswith("#["):
            continue
        kept.append(" ".join(bare.split()))
    return "\n".join(kept)


def _region(src: str, header: str) -> list[str] | None:
    """The declaration introduced by `header`, as a list of source lines.

    Brace-delimited declarations run to their matching close brace; a `const`
    runs to the first line whose scrubbed form ends in `;`.
    """
    lines = src.splitlines()
    start = None
    for i, line in enumerate(lines):
        if line.lstrip().startswith(header):
            start = i
            break
    if start is None:
        return None
    scrubbed = _RUST_LITERAL.sub("_", lines[start])
    if "{" not in scrubbed:
        for j in range(start, len(lines)):
            if _RUST_LITERAL.sub("_", lines[j]).rstrip().endswith(";"):
                return lines[start : j + 1]
        return None
    depth = 0
    for j in range(start, len(lines)):
        text = _RUST_LITERAL.sub("_", lines[j])
        depth += text.count("{") - text.count("}")
        if depth == 0:
            return lines[start : j + 1]
    return None


def ported_source(src: str, regions: tuple[str, ...] | None = None) -> tuple[str, str | None]:
    """The normalised text of every ported region, and the first one missing."""
    chunks = []
    for header in PORTED_STATES[CURRENT_DIGEST].regions if regions is None else regions:
        lines = _region(src, header)
        if lines is None:
            return "", header
        chunks.append(_normalise(lines))
    return "\n".join(chunks), None


def ported_digest(src: str, regions: tuple[str, ...] | None = None) -> str:
    text, missing = ported_source(src, regions)
    if missing is not None:
        return ""
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _ctx_variants(src: str) -> set[str] | None:
    lines = _region(src, "pub enum Ctx")
    if lines is None:
        return None
    body = _normalise(lines)
    inner = body[body.index("{") + 1 : body.rindex("}")]
    return {v.strip() for v in inner.split(",") if v.strip()}


def assert_port_matches_rust(src: str | None = None, states: dict | None = None):
    """Refuse to run if the Rust scanner is not one this file has been told about.

    Three layers, most specific first, because a digest mismatch says only
    that something changed and a reader needs to know *what*:

      1. the set of quoting contexts (`enum Ctx`) -- the drift that actually
         happened, and the one whose message can name the missing context;
      2. the escape alphabet (`DQ_ESCAPABLE`) -- the rule easiest to get
         silently wrong, and the only thing this guard checked before;
      3. the digest of all three ported regions -- complete where the first
         two are proxies. A rule changed *inside* an existing context passes
         both of the above and cannot pass this.

    `src` is injectable so the self-test can drive this against a fixture
    rather than against the real `shellquote.rs`. That is not a convenience:
    `shellquote.rs` is **lane A's file**, so a self-test that read it would be
    a lane-B test that lane A can turn red by editing its own code -- the one
    thing lane A's cross-lane rule says a self-test must never be able to do
    (`requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md`
    §4: a self-test reads only fixtures the checker carries in its own source).

    With `src=None` -- the real run -- it reads the real file, which is where
    the drift it exists to catch actually happens.
    """
    if src is None:
        try:
            src = RUST.read_text(encoding="utf-8")
        except OSError as e:
            raise SystemExit(f"cannot read {RUST}: {e}") from e
    states = PORTED_STATES if states is None else states
    modelled = states[max(states, key=lambda d: len(states[d].ctx))].ctx

    # --- 1. the contexts ----------------------------------------------------
    theirs_ctx = _ctx_variants(src)
    if theirs_ctx is None:
        raise SystemExit(
            "`pub enum Ctx` was renamed or reshaped in shellquote.rs.\n"
            "  This checker's port can no longer be shown to match it, so its\n"
            "  verdict would be about a scanner that is not the one shipping."
        )
    if not any(theirs_ctx == st.ctx for st in states.values()):
        gained = sorted(theirs_ctx - modelled)
        lost = sorted(modelled - theirs_ctx)
        detail = []
        if gained:
            detail.append(
                "  shellquote.rs has quoting context(s) this file does not "
                "model: " + ", ".join(gained) + "\n"
                "    Any input reaching one of them is graded here against\n"
                "    semantics that no longer ship, and reported as agreement."
            )
        if lost:
            detail.append(
                "  this file models context(s) shellquote.rs no longer has: "
                + ", ".join(lost)
            )
        raise SystemExit(
            "PORT HAS DRIFTED -- every result below would be about the wrong "
            "scanner.\n" + "\n".join(detail)
        )

    # --- 2. the escape alphabet --------------------------------------------
    m = re.search(r"const DQ_ESCAPABLE: \[u8; \d+\] = \[([^\]]*)\];", src)
    if not m:
        raise SystemExit(
            "DQ_ESCAPABLE was renamed or reshaped in shellquote.rs.\n"
            "  This checker's port can no longer be shown to match it, so its\n"
            "  verdict would be about a scanner that is not the one shipping."
        )
    # `b'"'`, `b'\\'`, `b'$'`, `b'`'`, `b'\n'` -> the bytes themselves.
    escapes = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", "'": "'"}
    theirs = set()
    for lit in re.findall(r"b'((?:\\.|[^'])+)'", m.group(1)):
        theirs.add(ord(escapes[lit[1]] if lit.startswith("\\") else lit))
    if theirs != DQ_ESCAPABLE:
        raise SystemExit(
            "PORT HAS DRIFTED -- every result below would be about the wrong "
            "scanner.\n"
            f"  shellquote.rs: {sorted(theirs)}\n"
            f"  this file    : {sorted(DQ_ESCAPABLE)}"
        )

    # --- 3. the whole of what is ported ------------------------------------
    #
    # Only the states whose context set the file actually has are candidates,
    # so the digest reported on failure is the one a reader would compare
    # against rather than an unrelated state's.
    candidates = {d: st for d, st in states.items() if st.ctx == theirs_ctx}
    seen = {}
    for digest, st in candidates.items():
        text, missing = ported_source(src, st.regions)
        if missing is not None:
            # A MISSING REGION IS REPORTED ALONGSIDE THE DIGEST, NOT INSTEAD OF
            # IT. This used to raise immediately, which cost lane A three runs:
            # they renamed `const fn hex_val` to `fn hex_val` in the same commit
            # that changed the `\c` semantics inside `decode_ansi_c`, and the
            # guard stopped at "renamed or reshaped" without ever reaching the
            # comparison -- naming the trivial half of what it had detected and
            # hiding the substantive half. Their fix, taken as offered: say
            # which keys resolved and which did not, and hash the ones that did.
            resolved = [r for r in st.regions if _region(src, r) is not None]
            partial, _ = ported_source(src, tuple(resolved))
            seen[digest] = (
                hashlib.sha256(partial.encode("utf-8")).hexdigest()
                + f"  (over {len(resolved)} of {len(st.regions)} region(s);"
                f" `{missing}` not found)",
                st,
            )
            continue
        got = hashlib.sha256(text.encode("utf-8")).hexdigest()
        if got == digest:
            return digest, st
        seen[digest] = (got, st)
    raise SystemExit(
        "PORT HAS DRIFTED -- every result below would be about the wrong "
        "scanner.\n"
        "  The contexts and the escape alphabet still match, so the change\n"
        "  is a rule INSIDE one of them, or a pinned declaration was renamed.\n"
        "  Those are the changes the two checks above cannot see, and are why\n"
        "  this third one exists.\n"
        + "".join(
            f"  {st.label}\n"
            f"    regions      : {', '.join(st.regions)}\n"
            f"    shellquote.rs: {got}\n"
            f"    recorded here: {digest}\n"
            for digest, (got, st) in seen.items()
        )
        + "  A region key is a substring match on the declaration's first line,\n"
        "  so prefer the shortest that is unambiguous -- `fn hex_val` matches\n"
        "  `const fn hex_val` too, and does not need touching when a qualifier\n"
        "  comes or goes.\n"
        "  Re-read `scan()` against the port, then re-bless with\n"
        "    python scripts/check-shellquote-vs-bash.py --print-digest"
    )


def _transition_warning(digest: str, st: "_State") -> str | None:
    """What a run against a non-current blessed state does NOT cover.

    Printed rather than raised, and printed *after* the transport check: a
    `run_checker --may-skip` gate takes the checker's first line of output as
    the reason it declined, so a warning emitted earlier would become the
    explanation for a gate that did not run.
    """
    if digest == CURRENT_DIGEST:
        return None
    missing = sorted(PORTED_CTX - st.ctx)
    return (
        f"TRANSITION: shellquote.rs is the {st.label} state.\n"
        f"  {st.note}\n"
        "  The port below models the CURRENT state, so every result here is\n"
        "  the port measured against bash. For "
        + (", ".join(missing) or "the difference")
        + " that says NOTHING\n"
        "  about the scanner in this tree, which does not have that context.\n"
        "  Read `0 failure(s)` accordingly until this entry is deleted."
    )



def _hex_val(b: int) -> int | None:
    """Port of `shellquote.rs::hex_val`."""
    if 0x30 <= b <= 0x39:
        return b - 0x30
    if 0x61 <= b <= 0x66:
        return b - 0x61 + 10
    if 0x41 <= b <= 0x46:
        return b - 0x41 + 10
    return None


# `$'...'` escapes that stand for exactly one byte and consume exactly one.
# `\e` and `\E` are both escape; bash accepts either.
_ANSI_C_ONE = {
    ord("n"): 0x0A, ord("t"): 0x09, ord("r"): 0x0D, ord("a"): 0x07,
    ord("b"): 0x08, ord("f"): 0x0C, ord("v"): 0x0B,
    ord("e"): 0x1B, ord("E"): 0x1B,
    BACKSLASH: BACKSLASH, ord("'"): 0x27, ord('"'): 0x22, ord("?"): 0x3F,
}


def decode_ansi_c(after: bytes) -> tuple[bytes, int]:
    """Port of `shellquote.rs::decode_ansi_c` -> (output bytes, bytes consumed).

    `consumed == 0` means no escape was recognised: the backslash stands for
    itself and the byte after it has not been eaten, which is how `$'\\q'` keeps
    both of its bytes.  Every edge here is bash 5.2.37's, measured by lane A
    rather than read off the manual, and the measurements are what make the
    edges surprising:

      * `\\x` takes **one or two** hex digits, so `$'\\x4'` is 0x04;
      * octal needs no leading zero (`$'\\101'` is `A`) and takes at most
        three digits, so `$'\\0101'` is 0x08 followed by a literal `1`;
      * `$'\\777'` wraps into a byte (0xff) rather than erroring;
      * `\\8` and `\\q` keep their backslash -- unrecognised is not an error;
      * `\\u`/`\\U` emit UTF-8 and are the only family yielding more than one
        byte, which is why `scan` needs a pending-output queue at all.
    """
    if not after:
        # Trailing backslash before the closing quote: literal backslash.
        return bytes([BACKSLASH]), 0
    c = after[0]
    if c in _ANSI_C_ONE:
        return bytes([_ANSI_C_ONE[c]]), 1
    if c in (ord("x"), ord("X")):
        val = n = 0
        while n < 2:
            d = _hex_val(after[n + 1]) if n + 1 < len(after) else None
            if d is None:
                break
            val, n = val * 16 + d, n + 1
        return (bytes([BACKSLASH]), 0) if n == 0 else (bytes([val & 0xFF]), n + 1)
    if c == ord("c"):
        # Control-X: X with bit 6 cleared. `\cA` is 0x01.
        #
        # The two special operands are lane A's repair to the defect this port
        # found on its first run (`requests/b-a-dollar-single-c-escape-swallows-
        # the-closing-quote.md`). The closing quote is NOT an operand -- it
        # terminates, and `\c` before it is a literal backslash-c -- so taking
        # it made `$'\c' tail` one word where bash makes two, a word boundary
        # lost in silence. A backslash IS an operand but is spelled with two
        # bytes: `$'\c\\'` is control-backslash, 0x1c, consuming three. Without
        # that arm the first backslash becomes the operand and the second
        # escapes the terminator, which is the stray-quote symptom.
        if len(after) < 2 or after[1] == ord("'"):
            return bytes([BACKSLASH]), 0
        if after[1] == BACKSLASH:
            return (bytes([after[2] & 0x1F]), 3) if len(after) > 2 else (bytes([BACKSLASH]), 0)
        return bytes([after[1] & 0x1F]), 2
    if c in (ord("u"), ord("U")):
        limit = 4 if c == ord("u") else 8
        val = n = 0
        while n < limit:
            d = _hex_val(after[n + 1]) if n + 1 < len(after) else None
            if d is None:
                break
            val, n = val * 16 + d, n + 1
        if n == 0:
            return bytes([BACKSLASH]), 0
        # A surrogate or out-of-range value has no UTF-8 encoding. Emitting
        # nothing would silently delete what the user typed, so the backslash
        # stands and the digits stay as themselves: visibly wrong beats
        # invisibly absent.
        if val > 0x10FFFF or 0xD800 <= val <= 0xDFFF:
            return bytes([BACKSLASH]), 0
        return chr(val).encode("utf-8"), n + 1
    if ord("0") <= c <= ord("7"):
        val = n = 0
        while n < 3 and n < len(after) and ord("0") <= after[n] <= ord("7"):
            val, n = val * 8 + (after[n] - ord("0")), n + 1
        return bytes([val & 0xFF]), n
    # Not an escape: bash keeps the backslash AND the character. Consuming
    # zero leaves the character to be scanned as an ordinary literal.
    return bytes([BACKSLASH]), 0


def scan(bs: bytes):
    """Yield (off, byte, ctx, escaped, structural) -- the Rust `Tok`."""
    i = 0
    ctx = UNQ
    pending = False
    queue, queue_at, queue_off = b"", 0, 0
    n = len(bs)
    while True:
        # A multi-byte `$'\u...'` escape yields its remaining bytes before the
        # input advances again, and is checked BEFORE the end-of-input test on
        # purpose: the escape may be the last thing on the line, and draining
        # afterwards would silently shorten the word. Getting this order wrong
        # is the one thing lane A said they would expect a blind port to miss.
        if queue_at < len(queue):
            b = queue[queue_at]
            queue_at += 1
            yield (queue_off, b, DSQ, True, False)
            continue
        if i >= n:
            return
        b = bs[i]
        off = i
        i += 1
        if pending:
            pending = False
            yield (off, b, ctx, True, False)
            continue
        if ctx == SGL:
            structural = b == ord("'")
            if structural:
                ctx = UNQ
            yield (off, b, SGL, False, structural)
        elif ctx == DBL:
            if b == BACKSLASH and i < n and bs[i] in DQ_ESCAPABLE:
                pending = True
                yield (off, b, DBL, False, True)
            else:
                structural = b == ord('"')
                if structural:
                    ctx = UNQ
                yield (off, b, DBL, False, structural)
        elif ctx == DSQ:
            # Unlike `'...'`, a backslash here IS special -- that is the entire
            # difference between the two constructs.
            if b == ord("'"):
                ctx = UNQ
                yield (off, b, DSQ, False, True)
            elif b == BACKSLASH:
                out, consumed = decode_ansi_c(bs[i:])
                i += consumed
                # Bytes 1.. are queued; byte 0 is yielded now. They all report
                # the offset of the backslash, the only offset that means
                # anything for a run of input that produced them jointly.
                queue, queue_at, queue_off = out, 1, off
                yield (off, out[0], DSQ, consumed > 0, False)
            else:
                yield (off, b, DSQ, False, False)
        else:
            if b == BACKSLASH and i < n:
                pending = True
                yield (off, b, UNQ, False, True)
            elif b == ord("$") and i < n and bs[i] == ord("'"):
                # Both the `$` and the `'` are syntax, so ONE structural token
                # stands for the pair and the `'` is stepped over rather than
                # seen as an ordinary opening quote. One token for two bytes is
                # why the `$` stops surviving quote removal -- without this arm
                # it fell to the literal catch-all and `echo $'hi'` printed
                # `$hi`.
                i += 1
                ctx = DSQ
                yield (off, b, DSQ, False, True)
            elif b == ord("'"):
                ctx = SGL
                yield (off, b, SGL, False, True)
            elif b == ord('"'):
                ctx = DBL
                yield (off, b, DBL, False, True)
            else:
                yield (off, b, UNQ, False, False)


def is_bare(t):
    return t[2] == UNQ and not t[3] and not t[4]


def strip_quotes(bs):
    # The NUL filter is `shellquote.rs::strip_quotes`'s, not the decoder's, so
    # `scan()` stays a faithful description of the input. See DIVERGENCES for
    # why dropping rather than truncating is the deliberate choice.
    return bytes(t[1] for t in scan(bs)
                 if not t[4] and not (t[1] == 0 and t[2] == DSQ))


def split_bare_words(bs):
    out, start, quoted = [], None, False
    for off, b, ctx, esc, st in scan(bs):
        if b in (32, 9) and is_bare((off, b, ctx, esc, st)):
            if start is not None:
                out.append((start, off, quoted))
                start = None
            quoted = False
        else:
            if start is None:
                start = off
            if st:
                quoted = True
    if start is not None:
        out.append((start, len(bs), quoted))
    return out


def find_bare(bs, needle):
    for t in scan(bs):
        if t[1] == needle and is_bare(t):
            return t[0]
    return None


def bare_positions(bs, needle):
    """The first two bare occurrences, as `shellquote.rs::bare_positions`."""
    hits = [t[0] for t in scan(bs) if t[1] == needle and is_bare(t)]
    return (hits[0] if hits else None,
            hits[1] if len(hits) > 1 else None)


def _rust_option(v) -> str:
    """Render the port's answer in the Rust the rung is written in."""
    if isinstance(v, tuple):
        return "(" + ", ".join(_rust_option(x) for x in v) + ")"
    return "None" if v is None else f"Some({v})"


def bash_words(line: bytes):
    """The exact word list bash produces for `line`.

    Delegated to bashprobe.  The original version of this function passed the
    script as an argv element to `bash -c` and read `printf '%s\\n'` output,
    and BOTH halves of that were wrong in ways that quietly weakened this
    file's verdict:

      * the argv round trip through Windows/wsl.exe ate backslashes, so every
        backslash case below was compared against a *different input* than
        the one written down -- in a file whose whole subject is backslashes;
      * `printf` reruns its format at least once, so zero words and one empty
        word both printed a single blank line.

    Both are why this file once reported 0 failures while the transport was
    silently mangling its own test data.  bashprobe delivers the bytes on
    stdin and counts words with `set --`/`$#`, and proves the transport is
    faithful before any case runs.
    """
    return bashprobe.words(line.decode("latin-1"), setup="")


def ours_words(line: bytes):
    return [strip_quotes(line[s:e]) for s, e, _q in split_bare_words(line)]


CASES = [
    # the two known-issues bugs
    b'"it\'s fine"',
    b'"don\'t.txt"',
    # backslash per context
    b"a\\ b",
    b"a\\'b",
    b"a\\>b",
    b'"C:\\dir"',
    b'"say \\"hi\\""',
    b"'it\\'",
    b"a\\\\",
    # quoting basics
    b"'a > b'",
    b"''",
    b"'' x",
    b'"" x',
    b"a'b'c",
    b'a"b"c',
    b"'a'\\''b'",
    # ANSI-C quoting, `$'...'`.  These ARE `$`-bearing and they belong here
    # anyway, which is worth stating because the note below says the opposite
    # about every other `$`: `$'...'` is not expansion at all, it is quote
    # removal with decoding, so bash's answer and ours are the same kind of
    # thing.  `$'$HOME'` proves it -- nothing inside expands.
    #
    # Every one of these was measured against bash 5.2.37 rather than read off
    # the manual, and three of them are why that matters: `\x` takes ONE or two
    # hex digits, octal needs no leading zero and stops at three, and an
    # unrecognised escape keeps its backslash instead of erroring.
    b"$'hi'",
    b"echo $'hi'",
    b"$''",
    b"$'a b'",
    b"$'x' $'y'",
    b"a$'\\n'b",
    b"$'\\u0041'z",
    b"$'a\\tb'",
    b"$'\\e'",
    b"$'\\E'",
    b"$'\\?'",
    b"$'\\\"'",
    b"$'\\''",
    b"$'\\x41'",
    b"$'\\x4'",
    b"$'\\x'",
    b"$'\\xg'",
    b"$'\\101'",
    b"$'\\0101'",
    b"$'\\777'",
    b"$'\\8'",
    b"$'\\q'",
    b"$'\\z\\n'",
    b"$'\\$'",
    b"$'\\$HOME'",
    b"$'\\`'",
    b"$'\\u'",
    b"$'\\u0041'",
    b"$'\\u00e9'",
    b"$'\\U0001F600'",
    b"$'\\cA'",
    # Fixed by lane A on 2026-09-12 after this port found them disagreeing with
    # bash; they are permanent coverage now rather than an open finding.
    b"$'\\c'",
    b"$'\\c' tail",
    b"$'\\c\\\\'",
    b"$'x\\c'y",
    b"$'x\\x41y'",
    b"$'\\x41'$'\\x42'",
    b"$'\\n'",
    b"$'\\t'",
    b"$'re\\xffport.txt'",
    b"$'\\x00'",
    b"$'\\u0000'",
    b"$'\\0'",
    # NOTE: the rest of the `\c` family, and `\u`/`\U` values with no scalar
    # (surrogates, and anything above U+10FFFF), are NOT here.  They disagree
    # with bash today -- measured, ten cases -- and the fault is in
    # `shellquote.rs`, not in this port, so they are filed to lane A as
    # `requests/b-a-dollar-single-c-escape-swallows-the-closing-quote.md`
    # rather than pinned here.  Pinning a defect in a table called CASES would
    # make it pass; pinning it in DIVERGENCES would call it a decision.  It is
    # neither: it is a bug with an owner.
    #
    # `$'\c '` is also held out even though it AGREES, which is the subtler
    # reason: it agrees by coincidence.  `\c` + space is 0x00, bash truncates
    # the word there and we drop the byte, and both roads end at one empty
    # word.  A case that passes for a reason other than the one it is named
    # for is worse than no case.
    #
    # NOTE: no OTHER `$`-bearing cases here.  bash expands before quote removal
    # and this harness only does quote removal, so any `$` case compares apples
    # to oranges.  The `$` question -- "which context is it in?" -- is asked
    # separately below, which is the only part kshell's expander needs.
    b"a b  c",
    b"  lead",
    b'"a b" c',
    b"x'y z'w",
    b'"a\\tb"',
    b'"a\\\\b"',
    b"\\\\",
    # NOTE: `a\` -- a *trailing* backslash -- is deliberately not here. It is
    # not a scanner question at all, and kshell answers it differently from
    # bash on purpose. See DIVERGENCES below, which tests it properly.
]


# Cases where kshell is intentionally NOT bash, with both answers pinned.
#
# These must not sit in CASES, because there they are failures, and a failure
# that is expected trains the reader to ignore the count. They must not be
# deleted either: an intended divergence is exactly the thing that needs a
# test, since nothing else will notice when it stops being intended.
#
# (line, bash's words, kshell's words, why they differ)
DIVERGENCES = [
    (
        b"a\\",
        [b"a"],
        [b"a\\"],
        "A trailing backslash is a backslash-NEWLINE to bash, so bash splices\n"
        "     the next input line and the backslash disappears. kshell has no\n"
        "     continuation prompt, so the line really does end there and the\n"
        "     backslash is data. shellquote.rs:250 says exactly this.\n"
        "     WHEN THE CONTINUATION PROMPT LANDS (shellquote.rs:147 plans one)\n"
        "     this entry is the thing that should start failing -- at which\n"
        "     point the question moves to the line editor and `a\\` stops being\n"
        "     answerable by a scanner at all.",
    ),
    (
        b"$'a\\0b'",
        [b"a"],
        [b"ab"],
        "A NUL inside $'...'. Bash TRUNCATES the word there, because it\n"
        "     carries words as C strings and cannot represent the rest. We\n"
        "     carry them as Vec<u8> and could keep the byte, but a NUL is not\n"
        "     a legal path byte under design.txt, so carrying it only defers a\n"
        "     rejection further from its cause. Dropping rather than\n"
        "     truncating is the deliberate part: strip_quotes is handed a whole\n"
        "     LINE by some callers and a single word by others and cannot tell\n"
        "     which, so truncating would discard later words bash keeps --\n"
        "     silently losing commands, to save one byte in a construct with no\n"
        "     known user. shellquote.rs::strip_quotes says this; todo.txt\n"
        "     Judgment Calls names the better fix (truncate in the per-word\n"
        "     callers, which know where the boundaries are).",
    ),
    # `\u`/`\U` with no Unicode scalar. Three cases rather than one, because
    # the argument for the divergence is in the THIRD: bash is not
    # self-consistent, so "match bash" does not name a single behaviour.
    #
    # Reported to lane A as finding 2 of
    # `requests/b-a-dollar-single-c-escape-swallows-the-closing-quote.md`, and
    # declared by them on 2026-09-12. Their comment used to say a surrogate
    # "has no UTF-8 encoding"; it has no *valid* one, and bash emits WTF-8
    # regardless. That premise is now corrected in `shellquote.rs` and the
    # behaviour deliberately stands.
    (
        b"$'\\ud800'",
        [b"\xed\xa0\x80"],
        [b"\\ud800"],
        "A lone surrogate. Bash emits WTF-8 -- the UTF-8 *shape* of a value\n"
        "     that is not a scalar. We hand back what was typed, so the user\n"
        "     can see it did not decode.",
    ),
    (
        b"$'\\U00110000'",
        [b"\xf4\x90\x80\x80"],
        [b"\\U00110000"],
        "Above U+10FFFF, the Unicode maximum. Bash encodes it anyway, four\n"
        "     bytes, in a form no decoder will accept.",
    ),
    (
        b"$'\\Uffffffff'",
        [b""],
        [b"\\Uffffffff"],
        "THE CASE THAT DECIDES THE OTHER TWO. Here bash emits NOTHING -- the\n"
        "     silently-absent outcome the fallback exists to avoid -- having\n"
        "     emitted WTF-8 for the two above. So bash is not self-consistent\n"
        "     and 'match bash' does not name one behaviour. A uniform 'hand\n"
        "     back what was typed' is predictable and lossless where bash is\n"
        "     neither, and if this ever has to match bash, matching it\n"
        "     INCLUDING the silent far end would be the wrong half to copy.\n"
        "     Declared by lane A 2026-09-12.",
    ),
]

# --- the rungs in shellquote.rs::self_test(), read out of the Rust ---------
#
# `GRADED_FUNCS` is what `assert_every_rung_is_accounted_for` sweeps for.  Every
# `assert_eq!` rung calling one of these must appear in `STRIP_RUNGS`,
# `OFFSET_RUNGS`, `DIVERGENCES` or `EXCUSED_RUNGS`, or this file refuses to
# return a verdict.  That is the check that would have caught the thing this
# rewrite is about -- not a rung asserting something false, but a rung nobody
# had transcribed, which a table-driven oracle cannot see it is missing.
GRADED_FUNCS = ("strip_quotes", "find_bare", "bare_positions")

# (rung call as typed in shellquote.rs, its input bytes, bash's words)
#
# The input bytes are transcribed rather than derived from the call text, and
# then checked against it. Deriving them would be less code and less value: the
# transcription is the third witness, and a reader that decoded `b"a\\'b"`
# wrongly would otherwise ask bash about the same wrong bytes it fed the
# comparison, and agree with itself.
STRIP_RUNGS = [
    (r'''strip_quotes(b"a\\'b")''', b"a\\'b", [b"a'b"]),
    (r'''strip_quotes(b"\"C:\\dir\"")''', b'"C:\\dir"', [b"C:\\dir"]),
    (r'''strip_quotes(b"\"say \\\"hi\\\"\"")''', b'"say \\"hi\\""',
     [b'say "hi"']),
    (r'''strip_quotes(b"'it\\'")''', b"'it\\'", [b"it\\"]),
    (r'''strip_quotes(b"a\\ b")''', b"a\\ b", [b"a b"]),
    # `strip_quotes(b"a\\")` is NOT here: it is the intended divergence, and
    # lives in DIVERGENCES where both answers are pinned. The sweep accepts it
    # from there, so removing it from that list does not silently uncover it.
]

# The intended divergence, as a rung.
#
# DIVERGENCES pins what bash and our port do with the line `a\`. This pins the
# third side of it: that shellquote.rs's rung still asserts *our* answer. It is
# deliberately not in STRIP_RUNGS, where bash disagreeing would read as a
# failure -- which is the confusion DIVERGENCES was created to prevent. But it
# must be pinned somewhere, because an intended divergence is exactly the thing
# nothing else notices when it stops being intended.
DIVERGENCE_RUNG = r'''strip_quotes(b"a\\")'''
DIVERGENCE_RUNG_EXPECTS = [b"a\\"]

# (rung call as typed in shellquote.rs, input bytes, needle, the port's answer)
#
# Graded against the PORT, not bash, and the difference matters. A shell has no
# way to report "the first bare `>` is at byte 17"; it either runs the redirect
# or does not. So these three legs are the rung, this table, and our own
# reimplementation -- which is a weaker oracle than bash, because the port and
# the Rust are both ours and can be wrong in the same direction. What it does
# catch is the two of them drifting apart, which is the failure that happens.
OFFSET_RUNGS = [
    (r'''find_bare(b"echo \"it's fine\" > out", b'>')''',
     b'echo "it\'s fine" > out', ord(">"), "Some(17)"),
    (r'''find_bare(b"echo a\\ b > out", b' ')''',
     b"echo a\\ b > out", ord(" "), "Some(4)"),
    (r'''find_bare(b"echo a\\>b", b'>')''',
     b"echo a\\>b", ord(">"), "None"),
    (r'''find_bare(b"echo 'a > b'", b'>')''',
     b"echo 'a > b'", ord(">"), "None"),
    (r'''find_bare(b"cat < \"don't.txt\"", b'<')''',
     b'cat < "don\'t.txt"', ord("<"), "Some(4)"),
    (r'''bare_positions(b"a>b>c", b'>')''',
     b"a>b>c", ord(">"), "(Some(1), Some(3))"),
    (r'''bare_positions(b"a'>'b", b'>')''',
     b"a'>'b", ord(">"), "(None, None)"),
]

# Rungs the sweep finds that cannot be graded from here, each with the reason.
#
# An excuse list is how a coverage check stays honest instead of becoming a
# thing people delete when it is inconvenient -- but it is only honest if the
# reasons are real, so each is a property of the rung and not of our appetite.
# A rung that loops over a LITERAL table, which `rustrungs.expectations` cannot
# read: the expected side at the call site is a loop variable, so the twenty
# answers live in the table above it.
#
# Excusing it would have been quick and would have been FALSE. The standing
# excuse for a loop rung is "its input is a loop variable, so there is no fixed
# case to ask bash about" -- true of `quote_word`'s round-trip loop, where the
# input is generated. Here every case is a literal a shell can be asked, so the
# excuse would have been a sentence that was accurate about the call and wrong
# about the rung. That is the exact shape this file exists to refuse.
#
# Graded three ways, the same as any other rung: the table is read out of the
# Rust and compared with the transcription here (a corrupted expectation in a
# twenty-case table is invisible to a reader and fatal to a boot); each answer
# is checked against the port; and each input is required to be in CASES, where
# bash grades it directly. The third is what keeps this from being the port
# agreeing with itself.
TABLE_RUNGS = [
    (
        "strip_quotes(ansi)",
        "let cases: &[(&[u8], &[u8])] = &[",
        [
            (b"$'hi'", b"hi"),
            (b"$'a b'", b"a b"),
            (b"echo $'hi'", b"echo hi"),
            (b"$'\\n'", b"\n"),
            (b"$'\\t'", b"\t"),
            (b"$'\\e'", b"\x1b"),
            (b"$'\\E'", b"\x1b"),
            (b"$'x\\x41y'", b"xAy"),
            (b"$'\\x4'", b"\x04"),
            (b"$'\\xg'", b"\\xg"),
            (b"$'\\101'", b"A"),
            (b"$'\\0101'", b"\x081"),
            (b"$'\\777'", b"\xff"),
            (b"$'\\8'", b"\\8"),
            (b"$'\\q'", b"\\q"),
            (b"$'\\cA'", b"\x01"),
            (b"$'re\\xffport.txt'", b"re\xffport.txt"),
            (b"$'\\u00e9'", b"\xc3\xa9"),
            (b"$'a\\0b'", b"ab"),
            # The two regressions from
            # `requests/b-a-dollar-single-c-escape-swallows-the-closing-quote.md`,
            # added by lane A with the fix. `$'\c'` is the literal two bytes,
            # not nothing -- the closing quote terminates and is never a `\c`
            # operand -- and `$'\c\\'` is control-backslash, where the operand
            # is spelled with two bytes.
            (b"$'\\c'", b"\\c"),
            (b"$'\\c\\\\'", b"\x1c"),
            (b"$'\\x41'$'\\x42'", b"AB"),
        ],
    ),
]

# Inputs a table rung may carry that no real shell is asked about. EMPTY, and
# the emptiness is the finding: five cases were written here first, with
# plausible reasons -- a newline or tab word would be reshaped by the probe's
# framing, and `$'re\xffport.txt'` carries a raw 0xff that the latin-1
# transport surely could not round-trip. Every one of those five was then
# measured and every one was wrong. bash carries all of them, 0xff included,
# because bashprobe frames words with NUL rather than with whitespace.
#
# Kept as a live hatch rather than deleted: the next case that genuinely cannot
# be asked needs somewhere to go that is not silence. But an entry here costs a
# case its only independent oracle, so the bar is a measurement showing the
# probe fails -- not a reason it ought to.
TABLE_RUNG_NOT_IN_CASES: dict[bytes, str] = {}

EXCUSED_RUNGS = {
    "strip_quotes(&q)":
        "inside the quote_word round-trip loop, so its input is a loop "
        "variable rather than a literal -- there is no fixed case to ask bash "
        "about. The property it asserts (quote_word output survives "
        "strip_quotes for any byte) is a Rust-side invariant, not a claim "
        "about bash.",
    "find_bare(&q, b'>')":
        "same loop, same reason: `q` is quote_word's output for each name in "
        "turn, not a literal this file could transcribe.",
    "strip_quotes(raw)":
        "inside the quote_suffix round-trip loop (self_test section 9), where "
        "`raw` is a slice of a line assembled from three loop variables -- a "
        "filename, a split point, and a quoting context -- so the rung stands "
        "for ~700 cases and not one literal. What it asserts is a Rust-side "
        "invariant, that quote_suffix's output parses back to the name it was "
        "given, rather than a claim about bash. The claim about bash is made "
        "separately and IS graded against the real shell: "
        "check-kshell-rungs-vs-bash.py's SUFFIX_CASES puts the three context "
        "rules to bash directly, and pins each one's Rust literal verbatim, "
        "so excusing it here does not leave quote_suffix ungraded anywhere.",
}


# Delimiter visibility: the redirect bug, asked directly.
#
# Overlaps OFFSET_RUNGS by four cases, deliberately. These are the port's own
# freestanding cases and are checked port-vs-table; OFFSET_RUNGS checks
# rung-vs-port. Same data, opposite directions, and either one alone leaves a
# way for the pair to agree while both being wrong.
DELIM = [
    (b'echo "it\'s fine" > out', ord(">"), 17),
    (b"cat < \"don't.txt\"", ord("<"), 4),
    (b"echo 'a > b'", ord(">"), None),
    (b'echo "a" > b', ord(">"), 9),
    (b"echo a\\>b", ord(">"), None),
]


# Which context is a `$` in?  This is the whole of what kshell's expander needs
# from the scanner, and getting it wrong is bug #2 in known-issues.md:
# `echo "it's $HOME"` prints $HOME literally because the expander tracks only
# `'` and treats the apostrophe inside the double quotes as opening a region.
CTX = [
    (b'echo "it\'s $HOME"', DBL),   # expands  -- the bug
    (b"echo 'it \"is\" $HOME'", SGL),   # does not expand
    (b"echo $HOME", UNQ),           # expands
    (b'echo "\\$HOME"', DBL),       # inside "..."; `escaped` is what stops it
    (b"echo \\' $HOME", UNQ),       # `\'` must not flip quoting for the line
]


#: Floors on how much this file must still be pinning -- not targets. Set
#: below the real counts by enough that ordinary editing does not trip them,
#: and far enough above zero that a gutted table, or a merge that took one
#: side of a conflict, does. `DIVERGENCES` has exactly one entry and its floor
#: is therefore its size: a single intended divergence cannot be thinned, only
#: deleted, and deleting it is precisely the event worth refusing to grade.
MIN_CASES = 15
MIN_DIVERGENCES = 1
MIN_DELIM = 4
MIN_CTX = 4
MIN_STRIP_RUNGS = 5
MIN_OFFSET_RUNGS = 7


def _assert_tables_are_not_gutted() -> None:
    """Refuse to grade tables too thin to be the ones this file was written on.

    An emptied or truncated table sails through the loops below and prints
    `0 failure(s)`, which is spelled exactly like a clean run. No fixture can
    catch that, because the fixture *is* the input that went missing -- so the
    assertion has to be on the real run. (Lane A's framing, in `requests/
    a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md` §2: a
    floor on discovery, not a target.)

    Raises rather than returning a verdict, deliberately: a breach means the
    question was not answered, not that the answer was no.
    """
    for name, table, floor in (
        ("CASES", CASES, MIN_CASES),
        ("DIVERGENCES", DIVERGENCES, MIN_DIVERGENCES),
        ("DELIM", DELIM, MIN_DELIM),
        ("CTX", CTX, MIN_CTX),
        # These two floors are the real rung counts, not a margin below them.
        # Unlike CASES, they are not a list someone tunes: they are a
        # transcription of what shellquote.rs asserts, so thinning one is
        # always either a rung that went away (which the sweep reports) or
        # coverage quietly dropped (which nothing else would).
        ("STRIP_RUNGS", STRIP_RUNGS, MIN_STRIP_RUNGS),
        ("OFFSET_RUNGS", OFFSET_RUNGS, MIN_OFFSET_RUNGS),
    ):
        if len(table) < floor:
            raise SystemExit(
                f"only {len(table)} case(s) in {name}, below the floor of "
                f"{floor}. Either this table has been gutted or a merge took "
                f"one side of a conflict; both want a human, and reporting "
                f"'0 failure(s)' over a table this thin would be the failure "
                f"this checker exists to prevent.")


def _read_rust() -> str:
    try:
        return RUST.read_text(encoding="utf-8")
    except OSError as e:
        raise SystemExit(f"cannot read {RUST}: {e}") from e


def assert_every_rung_is_accounted_for(src: str | None = None) -> None:
    """Refuse to grade if shellquote.rs asserts something no table names.

    Injectable `src` for the usual reason -- the self-test must never read lane
    A's file -- but note that the *gate* absolutely does, and must: this is a
    question about the real tree and cannot be answered from a fixture.

    Raises rather than counting a failure. A rung this file has never heard of
    is not a wrong answer, it is an unasked question, and the distinction is
    the entire subject here: `0 failure(s)` over a table that silently omits
    three rungs is spelled exactly like a clean run.
    """
    if src is None:
        src = _read_rust()
    found = rustrungs.assert_eq_calls(src, GRADED_FUNCS)
    if not found:
        raise SystemExit(
            f"no `assert_eq!` rung in {RUST.name} calls any of "
            f"{', '.join(GRADED_FUNCS)}.\n"
            "  Either self_test() was gutted or the reader is broken. Both\n"
            "  want a human: every check below would pass over nothing.")
    known = ({c for c, _b, _w in STRIP_RUNGS}
             | {c for c, _b, _n, _e in OFFSET_RUNGS}
             | {c for c, _d, _p in TABLE_RUNGS}
             | set(EXCUSED_RUNGS)
             | {DIVERGENCE_RUNG})
    strays = sorted({c for _line, c in found} - known)
    if strays:
        raise SystemExit(
            f"{len(strays)} rung(s) in {RUST.name} are graded by nothing "
            "here:\n\n  "
            + "\n  ".join(strays)
            + "\n\nA rung grades the implementation; it cannot grade itself, "
            "which is what\nthis file is for. Add each to STRIP_RUNGS (if "
            "bash can answer it),\nOFFSET_RUNGS (if only the port can), or "
            "EXCUSED_RUNGS with a reason.\nRefusing rather than reporting "
            "'0 failure(s)': the omission is the finding.")


def score_strip_rungs(src, rungs, quiet=False) -> int:
    """Rung expectation vs. transcription vs. bash, for `strip_quotes`."""
    fails = 0
    if not quiet:
        print("\n--- strip_quotes rungs: Rust vs. this file vs. bash ---")
    for call, line, want in rungs:
        sites = rustrungs.expectations(src, call)
        if not sites:
            fails += 1
            print(f"FAIL no rung `{call}` -- this file grades a call the tree "
                  f"does not make")
            continue
        # The input, decoded from the same Rust text the rung is written in,
        # must be the bytes we are about to hand bash. This is the check that
        # caught a doubled backslash in the sibling oracle.
        got_in = rustrungs.literals_in(call)
        if got_in != [line]:
            fails += 1
            print(f"FAIL {call} -- input transcribed as {line!r} but the Rust "
                  f"literal decodes to {got_in!r}")
            continue
        for lineno, expr, rung in sites:
            where = f"{call} at shellquote.rs:{lineno}"
            if not rung:
                fails += 1
                print(f"FAIL {where} -- expected side has no string literal: "
                      f"{expr}")
                continue
            if rung != want:
                fails += 1
                print(f"FAIL {where}")
                print(f"       rung asserts  : {rung}")
                print(f"       this file says: {want}")
                continue
            theirs = bash_words(line)
            ok = theirs == want
            if not ok:
                fails += 1
            if not quiet or not ok:
                print(f"{'ok  ' if ok else 'FAIL'} {where}")
                print(f"       rung={rung!r}")
                print(f"       bash={theirs!r}")
                if not ok:
                    print("       <-- the RUNG is wrong: bash is the oracle "
                          "here, not the subject")
    return fails


def read_rust_table(src: str, decl: str) -> list[tuple[bytes, bytes]] | None:
    """The literal `(input, want)` pairs of a Rust `let cases … = &[…];` table.

    Returns None if the declaration is absent -- which is a finding, not an
    empty table: a rung looping over a table this file cannot find would
    otherwise be graded against nothing and report agreement.
    """
    at = src.find(decl)
    if at < 0:
        return None
    body = src[at + len(decl):]
    end = _match_close(body)
    if end is None:
        return None
    # Whole-line comments carry byte literals inside prose (`\\x` above), so
    # they are dropped before any literal is read out.
    text = "\n".join(
        ln for ln in body[:end].splitlines() if not ln.strip().startswith("//")
    )
    pairs = []
    for element in rustrungs.split_top_level(text):
        lits = rustrungs.literals_in(element)
        if len(lits) == 2:
            pairs.append((lits[0], lits[1]))
    return pairs


def _match_close(text: str) -> int | None:
    """Index of the `]` closing the `[` the caller has already consumed."""
    depth = 1
    i = 0
    while i < len(text):
        c = text[i]
        if c == "\\":
            i += 2
            continue
        if c in "\"'":
            quote, i = c, i + 1
            while i < len(text) and text[i] != quote:
                i += 2 if text[i] == "\\" else 1
        elif c == "[":
            depth += 1
        elif c == "]":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return None


def _bash_is_asked(line: bytes) -> bool:
    """Is `line` put to a real shell anywhere in this file?

    DIVERGENCES counts. It pins bash's answer as well as ours, so a case
    living there has been asked and the answer recorded -- it is not an
    ungraded case, it is a graded one whose two sides differ on purpose.
    Missing this was a real hole: `$'a\\0b'` sat in DIVERGENCES and the first
    version of the table check called it port-only.
    """
    return (line in CASES
            or any(line == d[0] for d in DIVERGENCES)
            or line in TABLE_RUNG_NOT_IN_CASES)


def score_table_rungs(src, rungs, quiet=False) -> int:
    """A looped rung's literal table: Rust vs. this file vs. the port."""
    fails = 0
    if not quiet:
        print("\n--- looped rungs over a literal table ---")
    for call, decl, want in rungs:
        pairs = read_rust_table(src, decl)
        if pairs is None:
            fails += 1
            print(f"FAIL {call} -- `{decl}` is not in shellquote.rs, so the "
                  f"{len(want)} case(s) here grade nothing")
            continue
        if pairs != want:
            fails += 1
            print(f"FAIL {call} -- the Rust table is not what this file "
                  f"transcribes")
            print(f"       Rust      : {len(pairs)} case(s)")
            print(f"       this file : {len(want)} case(s)")
            for a, b in zip(pairs, want):
                if a != b:
                    print(f"       first difference: Rust {a!r} vs here {b!r}")
                    break
            continue
        bad = 0
        for src_bytes, expected in pairs:
            got = strip_quotes(src_bytes)
            if got != expected:
                bad += 1
                fails += 1
                print(f"FAIL {call} {src_bytes!r}")
                print(f"       rung asserts: {expected!r}")
                print(f"       the port says: {got!r}")
            elif not _bash_is_asked(src_bytes):
                bad += 1
                fails += 1
                print(f"FAIL {call} {src_bytes!r} is graded against the port "
                      f"only")
                print("       Add it to CASES so bash grades it, to "
                      "DIVERGENCES if bash deliberately disagrees, or to "
                      "TABLE_RUNG_NOT_IN_CASES with the reason bash cannot "
                      "be asked. A case the port checks against itself is "
                      "not evidence.")
        if not bad and not quiet:
            asked = sum(1 for s, _ in pairs if _bash_is_asked(s))
            print(f"ok   {call} -- {len(pairs)} case(s) transcribed exactly, "
                  f"{asked} of them also put to bash in CASES")
    return fails


def score_offset_rungs(src, rungs, quiet=False) -> int:
    """Rung expectation vs. transcription vs. the port. Needs no bash."""
    fails = 0
    if not quiet:
        print("\n--- byte-offset rungs: Rust vs. this file vs. the port ---")
    for call, line, needle, want in rungs:
        sites = rustrungs.expectations(src, call)
        if not sites:
            fails += 1
            print(f"FAIL no rung `{call}` -- this file grades a call the tree "
                  f"does not make")
            continue
        fn = bare_positions if call.startswith("bare_positions") else find_bare
        ours = _rust_option(fn(line, needle))
        for lineno, expr, _lits in sites:
            where = f"{call} at shellquote.rs:{lineno}"
            # Whitespace is the one difference a rewrap legitimately makes.
            rung = " ".join(expr.split())
            if rung != want:
                fails += 1
                print(f"FAIL {where}")
                print(f"       rung asserts  : {rung}")
                print(f"       this file says: {want}")
            elif ours != want:
                fails += 1
                print(f"FAIL {where}")
                print(f"       rung and this file agree on: {want}")
                print(f"       the ported scanner says     : {ours}")
                print("       <-- port and Rust have drifted; which is wrong "
                      "needs a human")
            elif not quiet:
                print(f"ok   {where} -> {want}")
    return fails


def score_divergence_rung(src, quiet=False) -> int:
    """The rung's side of the intended divergence. Needs no bash."""
    fails = 0
    sites = rustrungs.expectations(src, DIVERGENCE_RUNG)
    if not sites:
        print(f"FAIL no rung `{DIVERGENCE_RUNG}` -- the intended divergence "
              f"from bash is asserted by nothing")
        return 1
    for lineno, _expr, rung in sites:
        where = f"{DIVERGENCE_RUNG} at shellquote.rs:{lineno}"
        if rung != DIVERGENCE_RUNG_EXPECTS:
            fails += 1
            print(f"FAIL {where} -- the divergence stopped being the one "
                  f"pinned here")
            print(f"       rung asserts  : {rung}")
            print(f"       this file says: {DIVERGENCE_RUNG_EXPECTS}")
        elif not quiet:
            print(f"ok   {where} -> {rung} (bash says [b'a'], on purpose)")
    return fails


def score_cases(cases) -> int:
    """Compare our port's word split to bash's, for each case. Needs bash."""
    fails = 0
    for line in cases:
        theirs = bash_words(line)
        if theirs is None:
            # A skip is not a failure, so this branch is how a case disappears
            # from the verdict without disturbing the "0 failures" line. Every
            # case in CASES is one bash accepts -- that is a property of the
            # list, checked here rather than assumed -- so reaching this is a
            # defect in the list or in the probe, and either way the count
            # below would be a lie about how much was tested. Counting it is
            # the whole fix: `a\` sat in this branch printing SKIP for as long
            # as the file existed.
            print(f"FAIL (bash rejected a case that should parse): {line!r}")
            fails += 1
            continue
        mine = ours_words(line)
        ok = mine == theirs
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} {line!r}\n      bash={theirs!r}\n      ours={mine!r}")
    return fails


def score_divergences(divergences) -> int:
    """Check both pinned sides of each intended divergence. Needs bash."""
    fails = 0
    print("\n--- intended divergences from bash (BOTH sides pinned) ---")
    for line, want_bash, want_ours, why in divergences:
        got_bash = bash_words(line)
        got_ours = ours_words(line)
        ok_bash = got_bash == want_bash
        ok_ours = got_ours == want_ours
        ok = ok_bash and ok_ours
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} {line!r}")
        print(f"      bash={got_bash!r} (want {want_bash!r}){'' if ok_bash else '  <-- CHANGED'}")
        print(f"      ours={got_ours!r} (want {want_ours!r}){'' if ok_ours else '  <-- CHANGED'}")
        if not ok:
            print(f"     {why}")
    return fails


def score_ours_half(divergences) -> int:
    """Check only the *our-side* half of each divergence. Needs no bash.

    Split out from `score_divergences` so the self-test can assert the half
    that is ours -- what the ported scanner does with `a\\` -- on a host that
    is not being asked to run bash at all. The bash half stays where it is:
    it is a claim about bash, and only bash can answer it.
    """
    fails = 0
    for line, _want_bash, want_ours, why in divergences:
        got = ours_words(line)
        ok = got == want_ours
        if not ok:
            fails += 1
            print(f"FAIL {line!r} ours={got!r} (want {want_ours!r})\n     {why}")
    return fails


def score_delim(delim) -> int:
    """Where is the first *bare* delimiter? Needs no bash."""
    fails = 0
    print("\n--- bare-delimiter offsets ---")
    for line, needle, want in delim:
        got = find_bare(line, needle)
        ok = got == want
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} {line!r} {chr(needle)} -> {got} (want {want})")
    return fails


def score_ctx(ctx) -> int:
    """Which quoting context is the `$` in? Needs no bash."""
    fails = 0
    print("\n--- context of the `$` ---")
    for line, want in ctx:
        toks = [t for t in scan(line) if t[1] == ord("$")]
        got = toks[0][2] if toks else None
        esc = toks[0][3] if toks else None
        # For the escaped case the `$` is data, which the expander sees as
        # "escaped" rather than "quoted"; the context is unchanged either way.
        ok = got == want
        if not ok:
            fails += 1
        print(f"{'ok  ' if ok else 'FAIL'} {line!r} -> ctx={got} escaped={esc} (want {want})")
    return fails


def _selftest() -> int:
    """Exercise everything in this file that does not need bash.

    That is most of it, and this is the one of the four bash oracles where
    that is true: `scan` and its four consumers are a *port*, not a probe, so
    `DELIM`, `CTX` and the our-side half of `DIVERGENCES` are answerable here
    with no WSL and no subprocess. The gate is wired `--may-skip` and declines
    on a host without WSL, so a self-test that needed WSL would be absent from
    exactly the runs where it was the only coverage left -- and here it would
    be absent while being *able* to answer three of the four tables.

    `assert_port_matches_rust` is driven through its injected `src` and never
    against the real `shellquote.rs`, which is lane A's file: see that
    function's docstring.
    """
    checks = bad = 0

    def check(label, ok):
        nonlocal checks, bad
        checks += 1
        if ok:
            print(f"ok   {label}")
        else:
            print(f"selftest FAIL: {label}", file=sys.stderr)
            bad += 1

    def quietly(fn, *a):
        """Run a scorer with its output captured, and hand back both."""
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = fn(*a)
        return rc, buf.getvalue()

    # --- the port-drift guard, in every direction it can go wrong. --------
    #
    # Driven entirely through fixtures this file carries, never against the
    # real `shellquote.rs` -- see `assert_port_matches_rust`'s docstring for
    # why a lane-B self-test must not read a lane-A file.
    def fixture(ctx=None, alphabet=None, body=None, comment="// the scanner"):
        variants = "Unquoted,\n    Single,\n    Double," if ctx is None else ctx
        table = (
            "const DQ_ESCAPABLE: [u8; 5] = [b'\"', b'\\\\', b'$', b'`', b'\\n'];"
            if alphabet is None else alphabet
        )
        inner = "        self.i += 1;" if body is None else body
        return (
            "pub enum Ctx {\n"
            f"    {variants}\n"
            "}\n"
            "\n"
            f"{table}\n"
            "\n"
            f"    {comment}\n"
            "impl Iterator for QuoteScan<'_> {\n"
            "    fn next(&mut self) -> Option<Tok> {\n"
            f"{inner}\n"
            "    }\n"
            "}\n"
        )

    baseline = fixture()
    blessed = ported_digest(baseline, _SCANNER)
    states = {blessed: _State(regions=_SCANNER,
                              ctx=frozenset({"Unquoted", "Single", "Double"}),
                              label="the self-test fixture",
                              note="carried in this file, not read from lane A")}
    check("the fixture's three regions are found and digested",
          len(blessed) == 64 and blessed != hashlib.sha256(b"").hexdigest())

    try:
        assert_port_matches_rust(baseline, states=states)
    except SystemExit as exc:
        check(f"a matching Rust scanner passes ({exc})", False)
    else:
        check("a matching Rust scanner passes", True)

    # 1. THE DRIFT THAT ACTUALLY HAPPENED, 2026-09-12: lane A added a fourth
    #    quoting context (`Ctx::DollarSingle`, for `$'...'`) and nothing here
    #    could see it, because the change does not touch `DQ_ESCAPABLE`.
    try:
        assert_port_matches_rust(
            fixture(ctx="Unquoted,\n    Single,\n    Double,\n    DollarSingle,"),
            states=states,
        )
    except SystemExit as exc:
        check("a context the port does not model is refused",
              "PORT HAS DRIFTED" in str(exc))
        check("...and the refusal names the context rather than only the fact",
              "DollarSingle" in str(exc))
    else:
        check("a context the port does not model is refused", False)

    # ...and the other direction, which is not the same failure: a port that
    # models MORE than the Rust grades cases the Rust cannot reach. Worth
    # refusing, but the message must not accuse the Rust of hiding something.
    try:
        assert_port_matches_rust(
            fixture(ctx="Unquoted,\n    Single,"), states=states)
    except SystemExit as exc:
        check("a context the Rust no longer has is refused too",
              "PORT HAS DRIFTED" in str(exc) and "Double" in str(exc))
    else:
        check("a context the Rust no longer has is refused too", False)

    # 2. The drift this file was already guarded against: a rule changed in
    #    the Rust and not here. `\n` dropped from the alphabet is the
    #    realistic shape of it, because it is the member a reader is least
    #    likely to remember is there.
    drifted = "const DQ_ESCAPABLE: [u8; 4] = [b'\"', b'\\\\', b'$', b'`'];"
    try:
        assert_port_matches_rust(fixture(alphabet=drifted), states=states)
    except SystemExit as exc:
        check("a drifted Rust table is refused", "PORT HAS DRIFTED" in str(exc))
        check("...and the refusal prints both sides, not just a verdict",
              "shellquote.rs:" in str(exc) and "this file" in str(exc))
    else:
        check("a drifted Rust table is refused", False)

    # 3. THE CASE NEITHER OF THE ABOVE CAN SEE, and the reason the digest
    #    exists: a rule changed INSIDE an existing context. The contexts match,
    #    the alphabet matches, and the scanner is a different scanner.
    try:
        assert_port_matches_rust(
            fixture(body="        self.i = self.i.wrapping_sub(1);"),
            states=states,
        )
    except SystemExit as exc:
        check("a rule changed inside a context is refused",
              "PORT HAS DRIFTED" in str(exc))
        check("...and the refusal says it is an inside-a-context change",
              "INSIDE one of them" in str(exc))
    else:
        check("a rule changed inside a context is refused", False)

    # ...but NOT on a comment or an attribute, which is what the normaliser is
    # for. Without this the guard is noise, and a noisy guard gets bypassed --
    # which costs more than the drift it was added to catch.
    try:
        assert_port_matches_rust(
            fixture(comment="// rewritten comment, same scanner"),
            states=states,
        )
    except SystemExit as exc:
        check(f"a comment-only change does NOT fire the guard ({exc})", False)
    else:
        check("a comment-only change does NOT fire the guard", True)

    # 4. The ways the guard can go blind: a declaration renamed, so the
    #    extractor matches nothing and a carelessly-written check would
    #    compare empty against empty and pass.
    for renamed, label in (
        ("pub enum QuoteCtx", "enum Ctx"),
        ("const DQ_ESC", "DQ_ESCAPABLE"),
        ("impl Iterator for Scanner", "the scanner impl"),
    ):
        broken = fixture()
        broken = broken.replace(
            {"pub enum QuoteCtx": "pub enum Ctx",
             "const DQ_ESC": "const DQ_ESCAPABLE",
             "impl Iterator for Scanner": "impl Iterator for QuoteScan"}[renamed],
            renamed, 1)
        try:
            assert_port_matches_rust(broken, states=states)
        except SystemExit as exc:
            check(f"a renamed {label} is refused rather than read as empty",
                  "renamed or reshaped" in str(exc))
        else:
            check(f"a renamed {label} is refused rather than read as empty",
                  False)

    # 5. And the extractor itself: a `{` inside a byte literal must not
    #    unbalance the region, or the impl would end early and the digest
    #    would be taken over a fragment -- green having inspected a third of
    #    what it names.
    braced = fixture(body="        if b == b'{' { self.i += 1; }")
    lines = _region(braced, "impl Iterator for QuoteScan")
    check("a `{` inside a byte literal does not truncate the region",
          lines is not None and lines[-1].strip() == "}" and len(lines) == 5)

    # --- the transition table, which is the thing that can go stale. ------
    #
    # Two blessed states is a declaration, and a declaration outliving its
    # reason is the failure the digest exists to stop, one level up. So the
    # non-current one must ANNOUNCE itself on every run.
    current = PORTED_STATES[CURRENT_DIGEST]
    check("the port models the CURRENT state, so it needs no warning",
          _transition_warning(CURRENT_DIGEST, current) is None)
    for digest, st in PORTED_STATES.items():
        if digest == CURRENT_DIGEST:
            continue
        warning = _transition_warning(digest, st)
        check(f"the {st.label} state warns that it is not what the port models",
              warning is not None and "TRANSITION" in warning)
        check("...and names the context whose results therefore mean nothing",
              warning is not None
              and any(c in warning for c in PORTED_CTX - st.ctx))
        check("...and says when the entry must be deleted",
              "DELETE THIS ENTRY" in st.note)

    # --- the ANSI-C decoder, against bash 5.2.37's measured answers. ------
    #
    # Bash-free on purpose: these are the edges where a confident reading of
    # the manual lands in the wrong place, so they are the cases most worth
    # having on a host with no WSL. Every expectation here was measured.
    bs = bytes([BACKSLASH])
    for after, want_out, want_consumed, why in [
        (b"n", b"\n", 1, "the ordinary case"),
        (b"e", b"\x1b", 1, "\\e is escape"),
        (b"E", b"\x1b", 1, "...and so is \\E"),
        (b"x41", b"A", 3, "two hex digits"),
        (b"x4'", b"\x04", 2, "ONE hex digit is legal -- $'\\x4' is 0x04"),
        (b"xg", bs, 0, "no hex digit at all: not an escape, backslash kept"),
        (b"101", b"A", 3, "octal needs no leading zero"),
        (b"0101", b"\x08", 3, "...and stops at three, leaving a literal 1"),
        (b"777", b"\xff", 3, "wraps into a byte rather than erroring"),
        (b"8", bs, 0, "8 is not an octal digit"),
        (b"q", bs, 0, "unrecognised keeps the backslash AND the character"),
        (b"u0041", b"A", 5, "\\u, one byte out"),
        (b"u00e9", b"\xc3\xa9", 5, "\\u, TWO bytes out -- why a queue exists"),
        (b"U0001F600", b"\xf0\x9f\x98\x80", 9, "\\U, four bytes out"),
        (b"cA", b"\x01", 2, "control-A"),
        (b"", bs, 0, "a trailing backslash is a literal backslash"),
    ]:
        got_out, got_consumed = decode_ansi_c(after)
        check(f"decode_ansi_c({after!r}) -> {want_out!r}, {want_consumed} ({why})",
              got_out == want_out and got_consumed == want_consumed)

    # The property that makes `consumed == 0` mean what it says: the byte after
    # an unrecognised escape must NOT be eaten, or `$'\q'` would lose its `q`.
    check("an unrecognised escape consumes nothing after the backslash",
          all(decode_ansi_c(bytes([c]))[1] == 0 for c in b"qz89QZ"))

    # --- the bash-free tables, on the real data. --------------------------
    rc, _ = quietly(score_delim, DELIM)
    check(f"the {len(DELIM)} real DELIM cases agree with the port", rc == 0)
    rc, _ = quietly(score_ctx, CTX)
    check(f"the {len(CTX)} real CTX cases agree with the port", rc == 0)
    rc, _ = quietly(score_ours_half, DIVERGENCES)
    check(f"our side of the {len(DIVERGENCES)} divergence(s) is as pinned",
          rc == 0)

    # ...and that those three would actually notice. A scorer that returns 0
    # unconditionally passes all three checks above, which is why each needs a
    # planted wrong answer to prove the comparison is real.
    rc, text = quietly(score_delim, [(b"echo a > b", ord(">"), 999)])
    check("a wrong DELIM offset is counted", rc == 1)
    check("...and named in the output", "FAIL" in text and "999" in text)
    rc, text = quietly(score_ctx, [(b"echo '$HOME'", DBL)])
    check("a wrong CTX answer is counted", rc == 1)
    check("...and shows what the port actually said",
          f"ctx={SGL}" in text)
    rc, _ = quietly(score_ours_half, [(b"a\\", [b"a"], [b"WRONG"], "fixture")])
    check("a wrong our-side divergence is counted", rc == 1)

    # --- the scanner itself, on the cases whose answer needs no bash. -----
    # `CASES` is graded against bash and so cannot be checked here, but three
    # of its entries have answers that are not in dispute, and they are the
    # three the ported scanner is most likely to get wrong.
    check("quote removal drops the quotes and keeps the apostrophe",
          ours_words(b'"it\'s fine"') == [b"it's fine"])
    check("a backslash-escaped blank does not split the word",
          ours_words(b"a\\ b") == [b"a b"])
    check("the '\\'' idiom round-trips to one word with an apostrophe",
          ours_words(b"'a'\\''b'") == [b"a'b"])
    check("an empty quoted string is still a word",
          ours_words(b"''") == [b""])

    # --- the escape alphabet is consulted, not merely declared. -----------
    # `assert_port_matches_rust` proves DQ_ESCAPABLE equals the Rust's table.
    # Nothing above proves `scan` ever *reads* it: a scanner that treated every
    # byte after a backslash as escaped inside "..." passes that guard with the
    # table perfectly intact, and passed every other case here too. That the
    # one table this file goes to the trouble of pinning against the Rust was
    # also the one whose *use* went unchecked is the whole reason to state it
    # separately -- a guarded constant that nothing consults is decoration.
    def dq_backslash_escapes(nxt: int) -> bool:
        """Is the `\\` at offset 2 of `"a\\<nxt>b"` an escape, or data?"""
        return next(t for t in scan(b'"a\\' + bytes([nxt]) + b'b"')
                    if t[0] == 2)[4]

    check(f"inside \"...\" a backslash escapes each of the {len(DQ_ESCAPABLE)} "
          "alphabet members",
          all(dq_backslash_escapes(b) for b in sorted(DQ_ESCAPABLE)))
    outside = [b for b in b"tnzd0 e" if b not in DQ_ESCAPABLE]
    check("...and escapes nothing outside it -- the backslash stays data",
          not any(dq_backslash_escapes(b) for b in outside))
    # The same rule seen through the consumer, on two cases bash has graded:
    # `\t` is not escapable so both bytes survive, `\\` is so one is removed.
    check(r'"a\tb" keeps the backslash (\t is not in the alphabet)',
          ours_words(b'"a\\tb"') == [b"a\\tb"])
    check(r'"a\\b" collapses to one (\\ is in the alphabet)',
          ours_words(b'"a\\\\b"') == [b"a\\b"])

    # --- the rung tether, driven against fixture trees. -------------------
    #
    # Only the paths that fail *before* bash is consulted are exercised here,
    # which is not a compromise: a corrupted rung expectation is caught by
    # comparing it to this file's transcription, and that comparison is the
    # half that must work on a WSL-less host, because it is the only half that
    # runs there.
    fx = ('assert_eq!(strip_quotes(b"a b"), b"a b".to_vec());\n'
          "assert_eq!(find_bare(b\"a>b\", b'>'), Some(1));\n")
    strip_fx = [('strip_quotes(b"a b")', b"a b", [b"a b"])]
    offset_fx = [("find_bare(b\"a>b\", b'>')", b"a>b", ord(">"), "Some(1)")]

    rc, out = quietly(score_offset_rungs, fx, offset_fx, True)
    check("an offset rung matching the tree and the port passes", rc == 0)
    check("...and says nothing while doing so", out == "")

    rc, out = quietly(score_offset_rungs, fx.replace("Some(1)", "Some(9)"),
                      offset_fx, True)
    check("an offset rung the tree changed is caught", rc == 1)
    check("...and prints both sides, not just a count",
          "Some(9)" in out and "Some(1)" in out)

    # Rung and table agree, port disagrees: the port has drifted from the Rust,
    # which `assert_port_matches_rust` cannot see because it only reads one
    # constant. This is the branch that widens that tether.
    rc, out = quietly(score_offset_rungs, fx.replace("Some(1)", "Some(9)"),
                      [("find_bare(b\"a>b\", b'>')", b"a>b", ord(">"),
                        "Some(9)")], True)
    check("rung and table agreeing against the port is caught as drift",
          rc == 1)
    check("...and says which is which",
          "ported scanner says" in out and "drifted" in out)

    rc, out = quietly(score_offset_rungs, fx,
                      [("find_bare(b\"nope\", b'>')", b"nope", ord(">"),
                        "None")], True)
    check("an offset rung the tree does not contain is caught", rc == 1)
    check("...and says so in those terms", "does not make" in out)

    rc, out = quietly(score_strip_rungs,
                      fx.replace('b"a b".to_vec()', 'b"WRONG".to_vec()'),
                      strip_fx, True)
    check("a corrupted strip_quotes expectation is caught without bash",
          rc == 1)
    check("...and shows what the rung actually asserts", "WRONG" in out)

    rc, out = quietly(score_strip_rungs, fx,
                      [('strip_quotes(b"a b")', b"MISTRANSCRIBED", [b"a b"])],
                      True)
    check("an input transcribed differently from the Rust is caught", rc == 1)
    check("...and names both spellings",
          "MISTRANSCRIBED" in out and "decodes to" in out)

    # A rewrap is not a defect. rustfmt moves these across lines routinely, and
    # a tether that reds on formatting is a tether people rip out.
    rc, _ = quietly(score_offset_rungs,
                    "assert_eq!(\n    find_bare(b\"a>b\", b'>'),\n"
                    "    Some(1)\n);\n", offset_fx, True)
    check("a rustfmt-style rewrap of the same rung is not a failure", rc == 0)

    # --- the coverage sweep, which is the check this rewrite exists for. ---
    try:
        assert_every_rung_is_accounted_for(fx)
    except SystemExit as exc:
        check("a rung no table names is refused", "graded by nothing" in str(exc))
        check("...and the refusal names the rung, not just a count",
              'strip_quotes(b"a b")' in str(exc))
    else:
        check("a rung no table names is refused", False)

    try:
        assert_every_rung_is_accounted_for("fn self_test() {}\n")
    except SystemExit as exc:
        check("a gutted self_test() is refused rather than read as complete",
              "no `assert_eq!` rung" in str(exc))
    else:
        check("a gutted self_test() is refused rather than read as complete",
              False)

    covered = ('assert_eq!(strip_quotes(b"a\\\\ b"), b"a b".to_vec());\n'
               "assert_eq!(find_bare(b\"echo a\\\\>b\", b'>'), None);\n")
    check("a tree whose every rung is in a table passes the sweep",
          assert_every_rung_is_accounted_for(covered) is None)

    # The divergence's third side: what shellquote.rs itself asserts about `a\`.
    rc, out = quietly(score_divergence_rung,
                      'assert_eq!(strip_quotes(b"a\\\\"), b"a\\\\".to_vec());\n',
                      True)
    check("the intended divergence, as the rung states it, is as pinned",
          rc == 0)
    rc, out = quietly(score_divergence_rung, "fn self_test() {}\n", True)
    check("a deleted divergence rung is caught, not read as agreement",
          rc == 1)
    check("...and says the divergence is asserted by nothing",
          "asserted by nothing" in out)

    # The reader all of the above rests on.
    check("the rung reader agrees with its own fixtures",
          rustrungs.self_test() == 0)

    # --- the floors, seen to fire on each table in turn. ------------------
    real = {n: globals()[n] for n in ("CASES", "DIVERGENCES", "DELIM", "CTX",
                                      "STRIP_RUNGS", "OFFSET_RUNGS")}
    for name in real:
        try:
            globals()[name] = []
            try:
                _assert_tables_are_not_gutted()
            except SystemExit as exc:
                check(f"a gutted {name} refuses to return a verdict",
                      "below the floor" in str(exc) and name in str(exc))
            else:
                check(f"a gutted {name} refuses to return a verdict", False)
        finally:
            globals().update(real)
    check("...and the real tables pass the same guard",
          _assert_tables_are_not_gutted() is None)

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


def main():
    # Before bash is asked anything, and before the port check, because
    # neither needs WSL and a gutted table is worth reporting on a host that
    # cannot run the rest of this file at all.
    _assert_tables_are_not_gutted()
    state_digest, state = assert_port_matches_rust()
    src = _read_rust()
    # Before anything is graded: is there anything this file has not heard of?
    # An ungraded rung is an unasked question, not a wrong answer, so it raises
    # rather than joining the failure count.
    assert_every_rung_is_accounted_for(src)
    # These three read the Rust and need no WSL, so on a host that will shortly
    # decline the bash half they are the coverage that remains -- and they are
    # the half that catches a corrupted rung, which is the defect this file was
    # rewritten for.
    rung_fails = score_offset_rungs(src, OFFSET_RUNGS, quiet=True)
    rung_fails += score_divergence_rung(src, quiet=True)
    # Only the tables this tree actually has. A rung whose declaration is
    # absent is a finding when the rung is present and nothing when it is not,
    # and during the DollarSingle transition the four-context table is exactly
    # that: not yet here.
    live_tables = [r for r in TABLE_RUNGS if read_rust_table(src, r[1]) is not None]
    rung_fails += score_table_rungs(src, live_tables, quiet=True)
    # Both checks above run before the transport check and neither needs WSL:
    # a gutted table or a drifted port is worth reporting on a host that cannot
    # run the rest of this file at all, and both exit 1, which is a finding.
    #
    # Their *success* lines, though, are printed after it. `run_checker
    # --may-skip` takes the checker's FIRST line of output as the reason it
    # skipped, so anything printed before the decline becomes the explanation
    # in the transcript -- and "port verified against shellquote.rs" as the
    # reason a gate did not run reads like a pass, in the one place whose job
    # is to say that nothing was checked. Today the ordering survives by
    # accident (stdout block-buffers into the log and stderr does not), which
    # is exactly the kind of accident that ends when someone adds `-u`.
    if rung_fails:
        print(f"\n{rung_fails} rung(s) do not assert what this file says they "
              f"assert.\nBash was not asked anything: until the two agree, "
              f"asking it would be measuring\na case that is not in the tree.")
        return 1
    bashprobe.assert_transport_is_faithful()
    print("port verified against shellquote.rs")
    warning = _transition_warning(state_digest, state)
    if warning is not None:
        print(warning)
    # Count what is LIVE in this tree, not the sum of the tables' lengths. The
    # two differ during a transition -- the looped ANSI-C table is absent from
    # the three-context scanner -- and a summary that counted a rung the tree
    # does not have would be naming a population it had not inspected, which is
    # the failure this whole file is organised around.
    table_cases = sum(len(read_rust_table(src, d) or ()) for _c, d, _p in live_tables)
    n_rungs = (len(STRIP_RUNGS) + len(OFFSET_RUNGS) + 1 + len(EXCUSED_RUNGS)
               + len(live_tables))
    print(f"all {n_rungs} shellquote.rs rungs accounted for "
          f"({len(STRIP_RUNGS)} graded against bash, {len(OFFSET_RUNGS)} "
          f"against the port, 1 divergence, {len(EXCUSED_RUNGS)} excused, "
          f"{len(live_tables)} looped over a table of {table_cases} case(s))")
    print("transport verified faithful\n")

    fails = score_cases(CASES)
    fails += score_strip_rungs(src, STRIP_RUNGS)
    fails += score_divergences(DIVERGENCES)
    fails += score_offset_rungs(src, OFFSET_RUNGS)
    fails += score_divergence_rung(src)
    fails += score_table_rungs(src, live_tables)
    fails += score_delim(DELIM)
    fails += score_ctx(CTX)
    print(f"\n{fails} failure(s)")
    return 1 if fails else 0


def _print_digest() -> int:
    """Re-bless `PORTED_DIGEST` -- after re-reading `scan()`, not instead of it.

    Deliberately prints rather than rewrites this file. A guard that can
    silence itself in one command is a guard that gets silenced; the digest has
    to be pasted in by whoever has just looked at what changed.
    """
    try:
        src = RUST.read_text(encoding="utf-8")
    except OSError as e:
        raise SystemExit(f"cannot read {RUST}: {e}") from e
    variants = _ctx_variants(src) or set()
    print(f"contexts in shellquote.rs : {', '.join(sorted(variants))}")
    print(f"contexts modelled here    : {', '.join(sorted(PORTED_CTX))}")
    print()
    # One digest per DISTINCT region list, because which regions are pinned is
    # itself part of a state -- `decode_ansi_c` exists only in the four-context
    # scanner. Printing a single number would silently pick one.
    for regions in dict.fromkeys(st.regions for st in PORTED_STATES.values()):
        print(f"regions: {', '.join(regions)}")
        # Report per-key rather than giving up on the whole list: "no digest
        # for this list" is what sent lane A to importing `ported_digest` by
        # hand to get a number this command exists to print.
        resolved = [r for r in regions if _region(src, r) is not None]
        for key in regions:
            if key not in resolved:
                print(f"  MISSING  {key}  <- renamed, or the key is too specific")
        text, _ = ported_source(src, tuple(resolved))
        print(f"  {len(text)} bytes normalised over {len(resolved)} of "
              f"{len(regions)} region(s)")
        print('  "%s"' % hashlib.sha256(text.encode("utf-8")).hexdigest())
        if len(resolved) != len(regions):
            print("  ^ over the regions that RESOLVED. Fix the key(s) above "
                  "before pasting this.")
    return 0


if __name__ == "__main__":
    if "--print-digest" in sys.argv[1:]:
        sys.exit(_print_digest())
    if "--self-test" in sys.argv[1:] or "--selftest" in sys.argv[1:]:
        sys.exit(_selftest())
    sys.exit(main())
