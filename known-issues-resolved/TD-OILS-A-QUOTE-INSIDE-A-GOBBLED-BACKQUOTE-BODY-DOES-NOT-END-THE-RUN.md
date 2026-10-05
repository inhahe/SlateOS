### TD-OILS-A-QUOTE-INSIDE-A-GOBBLED-BACKQUOTE-BODY-DOES-NOT-END-THE-RUN. `brace_gobbler`'s quoting state is flat, and osh's is nested — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/interp.rs`, `Shell::gobbled_backtick_subs`. It
lexes a backquote body as **one** double-quoted run, so `quoted` stays `"` for
the whole of it. bash's scan is a flat character loop, and a `"` inside the body
is `c == quoted` — it sets `quoted = 0`, after which a `'` opens a skip, a
`` ` `` opens another, and a `<(`/`>(` starts reading.

**Reproduce.** With `z` unset:

```sh
echo "p`echo " ' $(fi)`q{,}"
```

| | bash 5.2.37 | osh |
|---|---|---|
| stdout | `pq{,}` | — |
| stderr | the backquote's own run: ``` `echo " ' $(fi)' ``` | the scanner's: ``` `fi)`q{,}"' ``` |
| `$?` | 0 | 1 |

The `"` closes the gobbler's state, the `'` then opens a single-quote skip, and
the `$(` inside it is never read — so the word brace-expands and the backquote
runs. osh reads the `$(fi)` and drops the command.

**What bash does.** braces.c:637-682, in order: `\` passes the next character
over unless `quoted == '\''`; `${` is treated like `\{`; then `if (quoted) { if
(c == quoted) quoted = 0; if (quoted == '"' && c == '$' && text[i+1] == '(')
goto comsub; … }`; then the unquoted `"`/`'`/`` ` `` row; then the unquoted
`($|<|>)(` row. Nothing about it nests.

**The fix.** Split the body on the gobbler's own state machine before lexing:
run the flat loop over the verbatim text, cut it into runs by whether `quoted`
is one the `$(` row can fire in (`0` or `"`) or one it cannot (`'` or `` ` ``),
lex only the former and leave the latter as literals. The catch is the comsub
row itself — `extract_command_subst` skips a whole `$( … )` extent, and a
quote inside *that* must not flip the state either, so the split needs an extent
count of its own rather than a byte scan. That is the only reason this was not
done with the fix above.

Reaching this needs a `"` (or a `` ` ``) inside a backquote body inside `" … "`
inside a word with a `{`, so it is not on any ordinary road. All 24 other cases
of the probe matrix that produced it agree with bash.

**Second site, added 2026-08-14 — and this one is a regression.**
`unparse::fill_quoted_runs` (from
TD-OILS-A-QUOTE-RUN-IN-A-PATTERN-OPERAND-IS-NOT-GOBBLED) lexes a `' … '` run
inside `" … "` the same way — one double-quoted word, `quoted` pinned at `"` —
so a `"` inside *that* run has the same unmodelled effect. Measured against
bash 5.2.37, with `z` unset:

```sh
echo "A[${z#'a"`$(fi)`'}]"
```

| | bash 5.2.37 | osh |
|---|---|---|
| stdout | `A[]` | — |
| stderr | — | ``command substitution: … `fi)`'}]"' `` |
| `$?` | 0 | 1 |

bash's `"` closes `quoted`, the `` ` `` then opens a backquote skip, and the
`$(fi)` inside it is never read. osh keeps `quoted` at `"` for the whole run, so
the backquote is not a skip, `gobbled_backtick_subs` walks into its body, and
the `$(fi)` is read. Before the fill, osh did not enter the run at all and
agreed with bash by accident — so this word matched before and does not now.
That is the cost of the fill, taken knowingly: it is one narrow shape, against a
common one that was wrong for every word.

The fix above is one fix for both sites: a helper that runs the flat loop over
verbatim text from a given starting `quoted` and hands back the stretches the
`$(` row can fire in, used by `Shell::gobbled_backtick_subs` and
`fill_quoted_runs` alike. For the latter the unreadable stretches go back in as
`WordPart::Literal`, which keeps the run re-printing to its own bytes — the
property every tail measured against it depends on.

**Fixed 2026-08-14, both sites, exactly that way.**
`crate::wordscan::gobbler_readable(text, dquoted)` is the flat loop itself —
braces.c:637-682 transcribed, `skip_matched` for the `extract_command_subst`
row so a quote inside a `$( … )` extent flips nothing — answering with the
stretches in which `quoted` is `0` or `"`. Its ranges are in order, disjoint,
and cover the text; a stretch may be empty, and an empty text answers with one
empty stretch rather than none.

Both readers then lex per stretch instead of whole:

* `unparse::gobbler_reading` (used by `fill_quoted_runs`) lexes each readable
  stretch with `dquote_word_from_source` and puts each skipped one back as a
  `WordPart::Literal` of its own bytes, so the filled run still re-prints to its
  source and every tail measured against it lands where it did. It declines the
  whole run if that does not hold.
* `Shell::gobbled_backtick_subs` lexes each readable stretch of the body and
  glues `rest-of-body` + `` ` `` + the word's own remainder onto every
  substitution the stretch contributes — the scan was handed the whole word, so
  a diagnostic raised inside a stretch still quotes everything after it.

The regression above is gone with it: `echo "A[${z#'a"`$(fi)`'}]"` gives `A[]`
and `$?` 0 as bash does, while `'a"`x`$(fi)'` — where the backquote closes again
and reading resumes — reports, also as bash does. Covered by
`tests/corpus/the-brace-gobblers-quoting-is-flat-so-it-reads-inside-a-quote-run.sh`
(12 rows) and
`tests/corpus/a-backquote-body-inside-double-quotes-is-gobbled-in-stretches.sh`
(9 rows), both matching bash 5.2.37 exactly.

**Residual, and it is a different bug.** The original repro's *scan* now agrees
— `pq{,}`, `$?` 0, no diagnostic from the gobbler — but its stderr still
differs, because the backquote then runs and its body (`echo " ' $(fi)`) has an
unterminated `"` around an unparseable `$( … )`. bash reports the substitution's
own syntax error; osh reports the unterminated quote. That is
TD-OILS-AN-UNPARSEABLE-SUBSTITUTION-IN-AN-UNTERMINATED-DQUOTE-LOSES-TO-THE-EOF,
which has nothing to do with brace expansion. Fixed the same day; with both in,
the original repro agrees end to end — same two diagnostic lines, same `pq{,}`,
same `$?` 0.
