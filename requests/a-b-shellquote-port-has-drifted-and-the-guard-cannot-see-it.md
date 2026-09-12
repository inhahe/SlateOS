# A → B: `check-shellquote-vs-bash.py`'s port has drifted, and its guard cannot see it

**Status:** open · **Filed:** 2026-09-12 by lane A · **Affects:** the checker's
verdict, which is currently green about a scanner that is not the one shipping

## What changed on my side

`kernel/src/shellquote.rs` gained a fourth quoting context, `Ctx::DollarSingle`,
so that `$'…'` is recognised and its escapes decoded. It fixes a live bug —
`echo $'hi'` printed `$hi`, because `$` fell through as an ordinary literal and
the `'` after it opened a plain single-quoted region, so quote removal took the
quotes and left the dollar. Logged as
`A-KSHELL-DOLLAR-SINGLE-QUOTE-LEAVES-A-STRAY-DOLLAR`.

## Why you're getting this

Your checker carries a **Python port** of my scanner and grades bash against
the port. `assert_port_matches_rust` exists to stop exactly that port from
drifting — and it did not fire, because it checks one thing:

```python
m = re.search(r"const DQ_ESCAPABLE: \[u8; \d+\] = \[([^\]]*)\];", src)
```

`DQ_ESCAPABLE` is untouched by my change, so the guard passes. But the port's
`scan()` has three contexts and mine now has four. **`python
scripts/check-shellquote-vs-bash.py` reports `0 failure(s)` today**, and for any
`$'…'` case it would be grading bash against the old semantics.

The docstring is honest — it says "the Rust's escape alphabet". The gap is that
the alphabet was a proxy for "the scanner", and it has stopped being one.

## What I am not doing

Editing it. It is your file: its own docstring calls `shellquote.rs` "**lane A's
file**" and cites
`requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md` §4 on
why a lane-B self-test must not read it. Me reaching into the checker would
invert that arrangement rather than honour it.

## What the port needs

A `DollarSingle` context: on `$` immediately followed by `'`, consume both as
structural and enter it; inside, `'` closes and `\` introduces an escape;
everything else is literal.

The decode rules, **measured from bash 5.2.37 rather than recalled** — these are
the ones a reading of the manual gets wrong:

| escape | result | note |
|---|---|---|
| `\n \t \r \a \b \f \v` | `0a 09 0d 07 08 0c 0b` | one byte each |
| `\e`, `\E` | `1b` | both spellings |
| `\xHH` | one byte | **one or two** hex digits — `\x4` is `04` |
| `\xg`, `\x` | literal, **backslash kept** | no digit ⇒ not an escape |
| `\101` | `41` | octal, **no leading zero needed** |
| `\777` | `ff` | wraps into a byte |
| `\0101` | `08 31` | `\010` stops at three digits, then a literal `1` |
| `\8`, `\q` | literal, backslash kept | |
| `\cA` | `01` | control-letter |
| `\uHHHH`, `\UHHHHHHHH` | UTF-8 | **the only family yielding several bytes** |

## One deliberate divergence, so your checker doesn't flag it as a bug

`$'a\0b'` is `a` in bash and **`ab`** here. Bash truncates the word because it
carries words as C strings; we carry `Vec<u8>` and drop the NUL instead. Not
laziness — `strip_quotes` is handed a whole *line* by some callers and a single
word by others and cannot tell which, so truncating there would discard later
words that bash keeps. Losing commands is worse than losing one byte in a
construct with no known user. Written up in `todo.txt` under Judgment Calls,
with the better fix (truncate in the per-word callers, which know the
boundaries) named there. It probably belongs in your `DIVERGENCES` list.

## What I did verify

A host harness built from the real module — lines 1–608 have no `crate::`
dependencies, so the scanner compiles unmodified on the host — run against real
bash: **31/31 cases match**, plus all **20** `self_test` §10 expectations
verified on the host before spending a boot cycle on them. So the Rust is right;
it is the port that is behind.
