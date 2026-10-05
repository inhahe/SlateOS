## `A-KSHELL-COLUMN-S-TAKES-ONE-CHARACTER-WHERE-GNU-TAKES-A-SET` (lane A, 2026-08-25) — ✅ **FIXED** 2026-08-25 (both halves); one cosmetic gap below remains open

**In short:** `column -s` was documented and implemented as "use this one
character as the delimiter". GNU treats the argument as a *set* — any one of
the characters in it separates — so `column -t -s ',;'` split on a comma or a
semicolon under GNU and on a comma only here. A user who typed the GNU form got
a plausible-looking table built on the wrong split, with no diagnostic.

**Where.** `kernel/src/kshell.rs` — `column_parse_args`, which did
`s.chars().next()`.

**What the proper fix looked like.** Carry the whole argument as a set of
characters (as bytes, one entry per character, since a multi-byte one must
still match as a unit) and split on any member. `split_on_bytes` becomes
`split_on_any_of`. The empty-field rule is unchanged: an explicit separator
keeps them.

**How it was fixed** (`7cfbd2ac8`, in its own commit as this note asked). Exactly
that. Three things worth carrying forward:

- **It is a set of characters, not of bytes.** Each member is the bytes of one
  character, matched as a unit, so `-s '→,'` splits on the whole `→`. Expanding
  the argument byte-wise would make `E2`, `86` and `92` each a separator, so one
  `→` would produce three breaks and two empty fields. Pinned by an assertion.
- **Longest match wins** where two members could both match. For a set built
  from characters that cannot happen — no character's UTF-8 encoding is a prefix
  of another's — but "first member in the order the user typed them" would make
  the output depend on argument order for no reason a user could predict, and
  longest-match is the rule that stays right if this ever takes multi-character
  separators.
- **A second `-s` replaces the set rather than extending it**, matching
  util-linux, where the option's argument *is* the set. Unioning would be just
  as implementable, so there is an assertion on input where the two readings
  differ visibly, to keep it a decision.

The return type became a named `ColumnArgs` struct on the way: with the
separator an `Option<Vec<Vec<u8>>>`, the old three-tuple tripped
`clippy::type_complexity`, which is deny-level here and right — see Lesson 49
for how that error nearly shipped past a delta check that filtered on
`grep warning`.

**Why it was filed rather than done at the time.** It is a behaviour change, not
a correctness fix, and it was found while fixing
`A-KSHELL-COLUMN-PADS-TO-BYTES-AND-REWRITES-WHAT-IT-CANNOT-DECODE` above.
Smuggling it into that commit would have made a bug fix and a semantic change
indistinguishable in the history — the same reason `diff`'s missing
`Binary files X and Y differ` was kept out of the `diff` byte fix (and was
then landed on its own, 2026-08-25).

**A second, smaller gap in the same place — ✅ FIXED 2026-08-25** (`e4444e980`, its
own commit). The flag walk was `args.split_whitespace()`, so a separator that
*is* whitespace could not be expressed at all: `column -t -s ' '` lost the
argument to the splitter and fell back to the default. Harmless in that one case
because space is the default, but `column -t -s '<TAB>'` — meaning tab and *not*
space, which is how one lines up a TSV whose fields contain spaces — was
unreachable. Pre-existing; not introduced by the byte conversion or by the set
fix.

> **The fix was available and the earlier note was wrong to imply otherwise.**
> This was previously written up as needing a quote-aware splitter that
> `kshell.rs` did not have. It has one: `split_words`, and the mechanism for
> reaching it is `command_parses_own_quotes`, the list of commands that receive
> their arguments with the quoting still in place. `column -s` is *precisely* the
> shape of `cut -d`, which was already on that list for the identical reason —
> `cut -d' ' -f1` is how you say "split on spaces", and dequoted it became
> `-d  -f1`, so the delimiter turned into the string `-f1`. `tr` is there for the
> same reason and failed silently rather than loudly.

**How the second half was fixed.** Exactly as that note said: `"column"` joined
`command_parses_own_quotes`; `column_parse_args` moved from
`args.split_whitespace()` to `split_words(args)` with an index-based loop
modelled on `parse_cut_args`, since an option that takes a value has to look at
the *next* word; `ColumnArgs::file_path` became a `String` (the words are owned
now), which dropped the struct's lifetime parameter. Three things worth carrying
forward:

- **A control assertion is what proves the separator arrived**, not the
  separator assertion. `column -t -s ' '` and `column -t` produce *different*
  output on `zz_a  zz_b` — the explicit space separator keeps the empty field
  between the two spaces, the default collapses runs of blanks — so the pair,
  asserted together, distinguishes "the tab/space reached `-s`" from "`-s` was
  lost and the default did the work". Asserting only the former would have
  passed before this change too.
- **`-s ''` is now refused, not silently defaulted**, because `split_words`
  drops an empty word so `-s` arrives with nothing after it. That is the better
  answer — a separator you asked for and did not get should say so — and the
  paragraph in `column_parse_args`'s doc comment that described the old fallback
  was rewritten in the same commit rather than left to contradict the code.
- **The quoted *operand* comes free and was pinned anyway.** Once the line
  arrives quoted, `column -t '/tmp/a file.txt'` names one file instead of
  producing an "extra operand" error. It is a consequence of the same change,
  not a separate feature, and an assertion holds it in place so a later revert
  to `split_whitespace` cannot take it back quietly.

Kept in its own commit, separate from the set fix, because they are different
changes: one is what the separator *means*, the other is whether the separator
*arrives*.

**A third, smaller one, noted while reading `parse_cut_args` — STILL OPEN.**
`column` takes `-s` only as two words. getopt also accepts the attached form,
`column -s,;`, which `cut` supports via `cut_opt_value` (`cut -d:` and
`cut -d :` are both legal). Not a wrong answer — `column -s,;` is refused as an
unrecognised option, loudly — but it is a legal invocation this shell rejects.
The fix is to route the `-s` branch of `column_parse_args` through
`cut_opt_value` the way `parse_cut_args` does, which also picks up `--separator=`
if that is ever wanted.

**Severity.** Low, and lower again. Both halves that could produce a *wrong or
unreachable* answer are fixed. What remains is one option *form* that is
rejected rather than misread — the failure mode is a diagnostic, not a bad
table.
