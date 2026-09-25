#!/usr/bin/env python3
"""One Rust lexer for the checkers that need to ignore comments and strings.

## Why this is a module and not a copy

Twelve scripts under `scripts/` defined their own version of this on
2026-09-10. Only one handled raw strings, so `r"a\\"` -- which ends at that
quote, because a backslash is not an escape inside a raw string -- swallowed
its terminator in three of them and paired every later quote one off.

The function has been wrong at least three separate times across its copies:

  * a char literal holding a quote. `rest.find('"')` appears in thirty-odd
    crates; without the char branch it opened a string that ran to the next
    quote anywhere later in the file.
  * a raw string ending in a backslash, as above.
  * blanking string BODIES rather than traversing them, which took a real
    count of 37 to 0 in lane A's `check-absent-operand-default` while its gate
    stayed green -- the pattern it measures IS a string literal.

Every one of those failed toward SILENCE: the scan still produced a plausible
list, still passed `--check`, and nothing in the output said the input had been
misread. That is the argument for one implementation with fixtures rather than
twelve without.

## The contract

`strip_noise(src)` returns a string of **exactly the same length** as `src`,
with comments, string literals, char literals and raw strings replaced by
spaces and newlines preserved. Length is load-bearing: callers match against
the returned text and then index the ORIGINAL at the same offsets to show real
source, which is how `check-read-defaults` reports the argument a call was
written with rather than the blanked version.

`keep_literals=True` blanks comments only and leaves strings, char literals
and raw strings intact. That is a different job with a real caller:
`scripts/host-errmsg.py` searches for message text INSIDE string literals, so
blanking them would take its count to zero with its gate green -- which is
exactly how lane A's `mask_noncode` lost 37 findings. A masker that cannot be
asked to spare literals invites its callers to write their own.

It DOES understand nested block comments (`/* /* */ */`), which Rust
allows and which nothing in this tree uses. If that changes, this is the place
to fix it once.
"""

from __future__ import annotations

import re


# A complete char literal: `'x'`, `'\n'`, `'\''`, `'\u{1F600}'`. Deliberately
# NOT a lifetime -- `'a` has no closing quote and must pass through untouched.
_CHAR_LITERAL = re.compile(r"'(?:\\u\{[0-9a-fA-F]{1,6}\}|\\.|[^\\'\n])'")

# A raw-string opener: `r"`, `r#"`, `br##"`. Backslash is NOT an escape inside
# one, so `r"a\"` ends at that quote -- treating it as an escape swallows the
# terminator and pairs every later quote one off. `b"` is deliberately absent:
# a byte string is escaped like an ordinary one.
_RAW_OPEN = re.compile(r'b?r(#*)"')


def strip_noise(src: str, keep_literals: bool = False) -> str:
    """Blank comments and string literals, preserving length and newlines.

    Without this the scan matches its own documentation: the four fixes above
    each carry a doc comment quoting the line they replaced, and an earlier
    version of this query counted all of them as live code.

    # The char-literal case, which the first version got wrong

    `rest.find('"')` is a char literal holding a double quote, and it appears
    in more than thirty crates here. Without the `'` branch below, that `"`
    opened a string that ran to the next `"` anywhere later in the file --
    after which every quote was paired one off, so real code was blanked as
    string and string contents were left as code.

    It failed silently and in the direction that looks fine: the scan still
    produced a plausible list. It was caught because a survey of `dbus` matched
    the word "simulated" INSIDE a println! whose text should have been blanked,
    and the blanked line showed the string contents surviving while the
    delimiters had gone.

    Lifetimes are not char literals -- `'a` and `'static` must pass through --
    so the branch matches only a complete `'x'`, `'\\n'` or `'\\u{1F600}'`.
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                out[i] = " "
                i += 1
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            # Rust block comments NEST, so a depth counter rather than a scan for
            # the first `*/`. Without this, `/* outer /* inner */ still out */`
            # ended at the inner close and the tail was read as code.
            depth = 0
            while i < n:
                if src.startswith("/*", i):
                    depth += 1
                    for k in range(i, min(i + 2, n)):
                        out[k] = " "
                    i += 2
                    continue
                if src.startswith("*/", i):
                    depth -= 1
                    for k in range(i, min(i + 2, n)):
                        out[k] = " "
                    i += 2
                    if depth == 0:
                        break
                    continue
                if src[i] != "\n":
                    out[i] = " "
                i += 1
        elif (c in "rb") and (_m := _RAW_OPEN.match(src, i)):
            close = '"' + "#" * len(_m.group(1))
            end = src.find(close, _m.end())
            end = n if end < 0 else end + len(close)
            if not keep_literals:
                for k in range(i, end):
                    if src[k] != "\n":
                        out[k] = " "
            i = end
        elif c == "'":
            # A CHAR LITERAL, not a lifetime. `'a` / `'static` fall through to
            # the catch-all below and are left alone; only a complete literal
            # is blanked, so `find('\"')` cannot open a string.
            m = _CHAR_LITERAL.match(src, i)
            if m:
                if not keep_literals:
                    for k in range(i, m.end()):
                        out[k] = " "
                i = m.end()
            else:
                i += 1
        elif c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            if not keep_literals:
                for k in range(i, min(j, n)):
                    if src[k] != "\n":
                        out[k] = " "
            i = j
        else:
            i += 1
    return "".join(out)


# ---------------------------------------------------------------------------
# Moved here from `check-read-defaults.py` on 2026-09-11, because a SECOND
# checker needed it and had written the naive version instead.
#
# `multicall-aliases.py` cut its view of a file at the first `#[cfg(test)]`
# with `text[: m.start()]`. On `userspace/last` that is a `#[cfg(test)] use`
# for the fixture builder's offsets, at line 39 of 2,025 -- so the gate saw 38
# lines, missed the dispatch at 1,233, and reported `last:lastb` and
# `last:lastlog` as NO LONGER PRESENT. `--update-baseline` would have recorded
# that as two aliases fixed. Nothing was fixed; the scanner had gone blind, and
# the ledger cannot tell those apart.
#
# That is the same defect check-read-defaults had and documents below, found
# again in another file eight hours later, and triggered by an edit of mine
# that added an ordinary `#[cfg(test)] use`. One implementation now.
# ---------------------------------------------------------------------------


#: Words that begin an *item* -- something with a `{` block or a `;` of its
#: own. Anything else after an attribute is a struct field, an enum variant or
#: a tuple element, all of which end at a comma.
#:
#: `union` and `macro_rules` are contextual keywords and could in principle
#: name a field. A field called `union` decorated with `#[cfg(test)]` would be
#: read as a union declaration and its block blanked; nothing in this tree
#: does that, and the alternative -- dropping them from the list -- misreads
#: the far commoner real `union`. The ambiguity is Rust's and is recorded
#: rather than resolved.
_ITEM_KEYWORDS = frozenset(
    {
        "pub",
        "fn",
        "mod",
        "impl",
        "struct",
        "enum",
        "trait",
        "union",
        "use",
        "const",
        "static",
        "type",
        "extern",
        "unsafe",
        "async",
        "macro_rules",
    }
)


def _decorated_item_kind(masked: str, at: int) -> str:
    """`"item"` or `"field"` for whatever follows an attribute at `at`.

    Reads the next word, skipping whitespace and any further attributes --
    `#[cfg(test)] #[derive(Debug)] struct X {` is one item with two of them.
    """
    i = at
    while i < len(masked):
        c = masked[i]
        if c.isspace():
            i += 1
            continue
        if c == "#":
            # Another attribute. Step over its brackets rather than its
            # characters: `#[cfg(all(test, feature = "x"))]` has nested ones.
            j = masked.find("[", i)
            if j < 0:
                return "item"
            depth = 0
            while j < len(masked):
                if masked[j] == "[":
                    depth += 1
                elif masked[j] == "]":
                    depth -= 1
                    if depth == 0:
                        j += 1
                        break
                j += 1
            i = j
            continue
        break
    word = ""
    while i < len(masked) and (masked[i].isalnum() or masked[i] == "_"):
        word += masked[i]
        i += 1
    return "item" if word in _ITEM_KEYWORDS else "field"


def _field_end(masked: str, at: int) -> int:
    """Where the non-item thing beginning at `at` ends.

    Three terminators, because an attribute lands on three shapes that are
    none of them blocks:

      * a struct field or enum variant -- ends at its separating comma;
      * a *statement* -- `#[cfg(test)] self.computes.set(..);` in
        `apps/mandelbrot` -- ends at its semicolon. Without this the scan ran
        to the enclosing block's `}` and blanked every live statement after
        it, which is the same defect as the one this function was changed to
        fix, one shape along. It was invisible in `mandelbrot` only because
        the statement happens to be the last in its block;
      * the last field or statement of a container -- ends at the `}` or `)`
        that closes it, and that character belongs to the container, so the
        scan stops *before* it rather than consuming it.

    Commas and semicolons inside `<..>`, `(..)` and `[..]` belong to a type
    or an argument list -- `buf: [u8; 4],` has both -- so only depth-zero
    ones end anything.
    """
    depth = 0
    i = at
    while i < len(masked):
        c = masked[i]
        if c in "([<{":
            depth += 1
        elif c in "}])":
            if depth <= 0:
                return i
            depth -= 1
        elif c == ">":
            depth -= 1
        elif c in ",;" and depth <= 0:
            return i + 1
        i += 1
    return len(masked)


def live_code(src: str) -> tuple[str, str]:
    """`src` with test code removed and nothing else.

    # What this replaced

    `src.split("#[cfg(test)]")[0]` -- "everything before the tests", which is
    only true when the FIRST such attribute is the test module. A
    `#[cfg(test)]` on a single helper is ordinary, and everything after it was
    discarded along with the tests.

    Measured before the fix: **35,706 lines across `userspace/`, 7% of the
    lane, invisible to this checker.** `fdisk/src/main.rs` was read as 21 lines
    of 3,818; `coreutils/src/bin/tar.rs` as 592 of 5,635.

    The floors did not catch it because they are aggregate. Losing 7% of the
    corpus leaves 398 live `read_to_string` calls against a floor of 120, and
    every file was still opened so the file count never moved. **A floor on the
    total cannot see a hole in the distribution** -- which is worth remembering
    before trusting one anywhere else.

    # How it works

    Blank each `#[cfg(test)]` item by matching its braces. Every one of them,
    including the test module, and the scan runs to the end of the file --
    there is no truncation and no special case for `mod`. Blanking preserves
    length, so match offsets still index the original, which is what lets
    `survey` show real argument text.

    It did cut at the first `#[cfg(test)] mod`, which is the same "the first
    one is the last thing in the file" assumption as the split it replaced.
    `oils/src/interp.rs` opens with a `#[cfg(test)] mod stderr_tee` helper at
    line 3,348 of 109,742 and that cut discarded 106,394 lines.

    Brace matching runs over the `strip_noise` output: a brace inside a string
    or a comment must not close an item early, and this file has been wrong
    about string boundaries twice already.
    """
    masked = strip_noise(src)
    # Both views are blanked at the same offsets and returned together, so
    # `survey` does not re-run `strip_noise` on the result. Running it twice
    # per file doubled the honour-head suite's wall clock past ten minutes,
    # which is a timeout rather than a slowdown.
    out = list(src)
    mout = list(masked)
    i = 0
    while True:
        i = masked.find("#[cfg(test)]", i)
        if i < 0:
            break
        # EVERY `#[cfg(test)]` ITEM IS BLANKED AND THE SCAN CONTINUES. There
        # is no special case for `mod`, and no truncation anywhere.
        #
        # There was: `if head.startswith("mod ")` returned everything before
        # the attribute, on the reasoning that "the test module ends the live
        # region". That is the SAME ASSUMPTION as the `split("#[cfg(test)]")[0]`
        # this function was written to replace -- "the first one I find is the
        # last thing in the file" -- moved down one level, from the attribute
        # to the attribute-plus-`mod`. It is wrong for the same reason: a
        # `#[cfg(test)] mod` can be a test HELPER sitting anywhere.
        #
        # `userspace/oils/src/interp.rs` is 109,742 lines. At line 3,348 it has
        # `#[cfg(test)] mod stderr_tee`, a helper that captures stderr; the real
        # `mod tests` is at line 70,020. This function returned 3,347 lines and
        # discarded 106,394, including 64,773 lines of live interpreter. Every
        # checker pointed here -- check-read-defaults and multicall-aliases --
        # was blind to all of it, and their baselines were written that way.
        #
        # Blanking the module instead of cutting at it costs nothing: the tests
        # are just as gone, and nothing is assumed about where they sit.
        # WHAT THE ATTRIBUTE IS ON decides where the item ends, and the
        # answer is not always a brace or a semicolon.
        #
        # `#[cfg(test)] rng: SeededRng,` is a STRUCT FIELD. It ends at a
        # comma. Looking for the next `{` finds one belonging to the next
        # item -- in `apps/speedtest/src/main.rs` the attribute sits on the
        # last field of `SpeedTestUI` and the next brace opens
        # `impl SpeedTestUI`, so brace-matching blanked the whole impl:
        # `start_test`, `begin_simulated_run`, the tick handler, 70% of the
        # file's production code. Every checker reading this function saw an
        # app whose only live code was its type declarations, and
        # `frozen-flag-survey` reported `phase` as a field nothing writes
        # while eight live assignments sat inside the blanked region.
        #
        # This is the third instance of one assumption in this function, and
        # the doc comment above already names the first two: "the first
        # `#[cfg(test)]` is the last thing in the file", then "the first
        # `#[cfg(test)] mod` is". Now: "the next `{` belongs to this
        # attribute". Each time the fix was to stop guessing the extent from
        # a character and read what the attribute is actually on.
        kind = _decorated_item_kind(masked, i + len("#[cfg(test)]"))
        if kind == "field":
            # A field or an enum variant: it ends at the comma that separates
            # it from the next one, or at the `}` that closes the container if
            # it is the last. Commas inside `<..>`, `(..)` and `[..]` are
            # parts of a type, not separators.
            end = _field_end(masked, i + len("#[cfg(test)]"))
            for k in range(i, min(end, len(out))):
                if out[k] != "\n":
                    out[k] = " "
                    mout[k] = " "
            i = end
            continue
        brace = masked.find("{", i)
        semi = masked.find(";", i)
        if brace < 0 or (0 <= semi < brace):
            # The attribute decorates a non-block item -- `#[cfg(test)] use
            # std::cell::RefCell;`. There is no block to match, and searching
            # on for a `{` would find one belonging to the NEXT item and blank
            # everything between. Drop the attribute and carry on.
            end = i + len("#[cfg(test)]")
            for k in range(i, end):
                if out[k] != "\n":
                    out[k] = " "
                    mout[k] = " "
            i = end
            continue
        depth = 0
        j = brace
        while j < len(masked):
            if masked[j] == "{":
                depth += 1
            elif masked[j] == "}":
                depth -= 1
                if depth == 0:
                    j += 1
                    break
            j += 1
        for k in range(i, min(j, len(out))):
            if out[k] != "\n":
                out[k] = " "
                mout[k] = " "
        i = j
    return "".join(out), "".join(mout)


def string_literals(src: str) -> list[str]:
    """Every string literal in `src`, comments excluded, delimiters stripped.

    For the checkers that ask "does this crate SAY something the user can
    read?" -- `find-silent-incapacity` and `find-stale-admissions`. Both used
    `re.findall(r'"(...)"')` on raw source, which is the twelfth copy of the
    mistake this module was written to end: one quotation mark inside a `//`
    comment pairs with the next one in code, and the source between them comes
    back as a "literal".

    That failure is ASYMMETRIC and runs toward silence, like every other one
    recorded above. `find-silent-incapacity` asks whether a crate admits an
    incapacity *anywhere*; a bogus span that swallowed a comment containing the
    word "cannot" made a crate that admits nothing look like one that does, and
    it was skipped. Seven crates were being quietly cleared.

    Rather than lex a third time, this derives the spans from the two passes
    already here: a character belongs to a literal exactly when `strip_noise`
    blanks it and `strip_noise(keep_literals=True)` does not. Everything the
    module has learned about raw strings, char literals and nested comments is
    inherited rather than re-implemented.

    Two characters cannot be told apart by that rule, because blanking maps to
    a space and preserves newlines: a SPACE or a NEWLINE already equal to its
    blanked form looks live wherever it stands. Both continue an open span
    instead. Without that, `r#"has "quotes" inside"#` came back as three
    separate literals, split at each space -- which would have been invisible
    in a caller that only asks whether some literal matches a word.

    Two literals cannot be adjacent across whitespace in Rust without live code
    between them, so continuing a span cannot join two. Whitespace picked up
    past a closing delimiter is trimmed.

    Char literals are literals to `strip_noise` and are dropped here: `'"'` is
    not something the user reads.
    """
    blanked = strip_noise(src)
    kept = strip_noise(src, keep_literals=True)
    out: list[str] = []
    cur: list[str] = []
    for ch, b, k in zip(src, blanked, kept):
        if b != ch and k == ch:
            cur.append(ch)
        elif ch in " \n" and cur:
            cur.append(ch)
        elif cur:
            out.append("".join(cur).strip())
            cur = []
    if cur:
        out.append("".join(cur).strip())
    return [_undelimit(s) for s in out if not s.startswith("'")]


_DELIM = re.compile(r'^b?r?(#*)"(.*)"\1$', re.S)


def _undelimit(lit: str) -> str:
    """`"hi"` -> `hi`, `r#"hi"#` -> `hi`. Anything unrecognised is returned."""
    m = _DELIM.match(lit)
    return m.group(2) if m else lit


def _self_test() -> int:
    """Fixtures for every way this has been wrong.

    Each pins a case that shipped. They are here rather than in a caller
    because the function is here: a caller's suite proves the caller reads the
    lexer correctly, not that the lexer is correct.
    """
    failures = 0

    def expect(label: str, got: object, want: object) -> None:
        nonlocal failures
        ok = got == want
        failures += not ok
        print(f"  {'ok  ' if ok else 'FAIL'}  {label}")
        if not ok:
            print(f"          got  {got!r}")
            print(f"          want {want!r}")

    # THE SHIPPED BUG. `find('"')` is a char literal holding a double quote and
    # appears in more than thirty crates. Without the char branch it opened a
    # string that ran to the next quote anywhere later in the file, after which
    # code was blanked as string and string contents stood as code.
    src = 'let end = rest.find(\'"\')?;\nlet x = "hidden";\n'
    out = strip_noise(src)
    expect("a char literal holding a quote does not open a string",
           "rest.find(" in out, True)
    expect("...and the string after it is still blanked",
           "hidden" in out, False)

    # Lifetimes are not char literals and must survive: they have no closing
    # quote, so blanking on sight would eat the rest of the line.
    expect("a lifetime is left alone",
           "'a" in strip_noise("fn f<'a>(s: &'a str) {}"), True)

    # A raw string does not process escapes, so `r"a\"` ends at that quote.
    # Treating the backslash as an escape swallows the terminator.
    out = strip_noise('let re = r"a\\";\nlet y = "seen";\n')
    expect("a raw string ending in a backslash terminates there",
           "seen" in out, False)
    expect("...and the code after it survives",
           "let y =" in out, True)

    # Hashed raw strings close only on the matching hash count.
    out = strip_noise('let a = r#"x"y"#;\nlet b = "gone";\n')
    expect("a hashed raw string spans an inner quote",
           "let b =" in out, True)
    expect("...and the following string is blanked",
           "gone" in out, False)

    # An ordinary string DOES process escapes.
    out = strip_noise('let a = "x\\"y";\nlet b = 1;\n')
    expect("an escaped quote does not end an ordinary string",
           "let b = 1" in out, True)

    expect("a line comment goes",
           "note" in strip_noise("let a = 1; // note\n"), False)
    expect("a block comment goes",
           "note" in strip_noise("let a = /* note */ 1;"), False)

    # Length is preserved so match offsets index the original -- the property
    # `survey` relies on to show real argument text in the baseline.
    for probe in ["let a = \"x\";", "// c\n", "r#\"q\"#", "'\\n'"]:
        expect(f"length preserved: {probe!r}",
               len(strip_noise(probe)), len(probe))

    # --- live_code -------------------------------------------------------
    #
    # It had NO case here at all until 2026-09-11, which is why the `mod` cut
    # below survived being moved into this module: the suite passed identically
    # with and without the bug. Two gates import this function.
    #
    # THE SHIPPED BUG, and it is the same one `live_code` was written to fix,
    # one level down. The split it replaced assumed the first `#[cfg(test)]`
    # ended the file; this assumed the first `#[cfg(test)] mod` did.
    # `oils/src/interp.rs` opens with a `#[cfg(test)] mod stderr_tee` helper at
    # line 3,348 of 109,742 -- the real `mod tests` is at 70,020 -- so the cut
    # returned 3,347 lines and discarded 106,394.
    helper_first = """fn before() {}
#[cfg(test)]
mod stderr_tee { fn cap() {} }
fn after_helper() { read_to_string(p); }
#[cfg(test)]
mod tests { fn t() {} }
fn after_tests() {}
"""
    live, _ = live_code(helper_first)
    expect("live code before a test helper mod survives", "before" in live, True)
    expect("live code AFTER a test helper mod survives", "after_helper" in live, True)
    expect("live code after the test mod survives", "after_tests" in live, True)
    expect("the helper mod body goes", "cap" in live, False)
    expect("the test mod body goes", "fn t()" in live, False)
    expect("live_code preserves length", len(live), len(helper_first))

    # THE SHIPPED BUG, third instance of one assumption. A `#[cfg(test)]` on a
    # STRUCT FIELD ends at a comma, and the next `{` belongs to whatever item
    # follows the struct. `apps/speedtest/src/main.rs` has the attribute on
    # `SpeedTestUI`'s last field and `impl SpeedTestUI` immediately after, so
    # brace-matching blanked the entire impl: `start_test`, the tick handler,
    # every assignment to `phase`. 29% of that file was read as live where 56%
    # is. `frozen-flag-survey` duly reported `phase` as written by nothing.
    field_then_impl = """struct S {
    real: u8,
    #[cfg(test)]
    rng: SeededRng,
}

impl S {
    pub fn start(&mut self) { self.phase = Phase::Idle; }
}
"""
    live, _ = live_code(field_then_impl)
    expect("a cfg(test) field does not swallow the impl after it",
           "start" in live, True)
    expect("...and the assignment inside it is live",
           ".phase =" in live, True)
    expect("...while the field itself is gone", "SeededRng" in live, False)
    expect("...and the struct still closes", live.count("}") , field_then_impl.count("}"))

    # The last field of a struct has no trailing comma: it ends at the `}`,
    # which belongs to the struct and must survive.
    last_field = """struct S {
    real: u8,
    #[cfg(test)]
    rng: SeededRng
}
fn kept_after_last_field() {}
"""
    live, _ = live_code(last_field)
    expect("a cfg(test) last field does not eat the closing brace",
           "kept_after_last_field" in live, True)

    # A comma inside a generic type is part of the type, not the separator
    # that ends the field.
    generic_field = """struct S {
    #[cfg(test)]
    seen: HashMap<String, u64>,
    real: u8,
}
"""
    live, _ = live_code(generic_field)
    expect("a comma inside a generic does not end the field early",
           "HashMap" in live, False)
    expect("...and the field after it survives", "real" in live, True)

    # An enum variant is the same shape as a field.
    variant = """enum E {
    Real,
    #[cfg(test)]
    OnlyInTests,
}
fn kept_after_variant() {}
"""
    live, _ = live_code(variant)
    expect("a cfg(test) enum variant ends at its comma",
           "OnlyInTests" in live, False)
    expect("...and what follows the enum is live",
           "kept_after_variant" in live, True)

    # And a function is still a block item: the brace path must not regress.
    fn_item = """#[cfg(test)]
fn helper(a: u8, b: u8) -> u8 { a + b }
fn kept_after_fn() {}
"""
    live, _ = live_code(fn_item)
    expect("a cfg(test) fn with a comma in its arguments is still blanked",
           "helper" in live, False)
    expect("...and the item after it survives", "kept_after_fn" in live, True)

    # A `#[cfg(test)]` on a STATEMENT ends at its semicolon.
    # `apps/mandelbrot/src/main.rs:594` has one on
    # `self.computes.set(..);` inside an `if`. Scanning to the enclosing
    # block's `}` instead blanks every live statement after it -- and that
    # file hid the fault, because there the statement happens to be the last
    # in its block.
    stmt = """fn f(&mut self) {
    let a = 1;
    #[cfg(test)]
    self.counter.set(2);
    let kept_after_stmt = 3;
}
"""
    live, _ = live_code(stmt)
    expect("a cfg(test) statement ends at its semicolon",
           "counter" in live, False)
    expect("...and the statements after it are live",
           "kept_after_stmt" in live, True)

    # A semicolon inside a type is part of the type: `[u8; 4]`.
    array_field = """struct S {
    #[cfg(test)]
    buf: [u8; 4],
    real: u8,
}
"""
    live, _ = live_code(array_field)
    expect("a semicolon inside an array type does not end the field early",
           "buf" in live, False)
    expect("...and the field after it survives", "real" in live, True)

    # An attribute on a non-block item has no braces of its own. Searching on
    # for a `{` finds the NEXT item's and blanks everything in between.
    use_plain = """#[cfg(test)]
use std::cell::RefCell;
fn kept() { read_to_string(p); }
"""
    live, _ = live_code(use_plain)
    expect("a cfg(test) use does not swallow the next item", "kept" in live, True)

    # ...but a *braced* use has braces that are its own, and only its own.
    use_braced = """#[cfg(test)]
use utmpfile::{Record, OFFSETS};
fn kept_after_braced_use() {}
"""
    live, _ = live_code(use_braced)
    expect("a braced cfg(test) use does not swallow the next item",
           "kept_after_braced_use" in live, True)
    expect("the braced use itself goes", "Record" in live, False)

    # Brace matching runs over `strip_noise` output precisely so this holds.
    brace_in_string = """#[cfg(test)]
mod tests { fn t() { let s = "}"; } }
fn tail() { read_to_string(p); }
"""
    live, _ = live_code(brace_in_string)
    expect("a brace inside a string does not end a test mod early",
           "tail" in live, True)
    expect("the test mod around it still goes", "let s" in live, False)

    # `string_literals`. The first case is the one that made the naive version
    # dangerous: a quote in a comment must neither open a literal nor be
    # returned as one, or a crate that admits nothing reads as one that does.
    expect("a quote in a comment is not a literal",
           string_literals('// it\'s a "quoted" word\nlet x = "real";'),
           ["real"])
    expect("an unbalanced quote in a comment does not swallow code",
           string_literals('// unbalanced " here\nlet a = 1;\nlet b = "real";'),
           ["real"])
    expect("a char literal holding a quote does not open a string",
           string_literals("let c = '\"'; let s = \"after\";"),
           ["after"])
    expect("a raw string keeps its inner quotes",
           string_literals('let x = r#"has "quotes" inside"#;'),
           ['has "quotes" inside'])
    expect("a raw string ending in a backslash does not swallow its terminator",
           string_literals('let x = r"a\\"; let y = "after";'),
           ["a\\", "after"]),
    expect("a lifetime does not open a char literal",
           string_literals("fn f<'a>(s: &'a str) -> &'a str { \"out\" }"),
           ["out"])
    expect("a multi-line literal stays one literal",
           string_literals('let s = "one\ntwo";'),
           ["one\ntwo"])
    expect("escapes come back as source, not decoded",
           string_literals(r'let s = "a\nb";'),
           [r"a\nb"])

    print(f"rustlex: self-test {'FAILED' if failures else 'passed'} "
          f"({failures} failure(s))")
    return 1 if failures else 0


if __name__ == "__main__":
    import sys
    sys.exit(_self_test())
