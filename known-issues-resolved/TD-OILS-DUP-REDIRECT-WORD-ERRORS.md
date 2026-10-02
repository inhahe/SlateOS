### TD-OILS-DUP-REDIRECT-WORD-ERRORS. `<&WORD`/`>&WORD` misclassifies and misquotes its failures — ✅ RESOLVED (command redirects) — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — the redirect path that resolves a
`<&`/`>&` whose target is a *word* rather than a literal fd number.

**Reproduce:**

```sh
e=""
read -r l <&"$e"                    # empty after expansion
echo hi >&"$e"
read -r l <&"abc"                   # not a number
read -r l <&"-1"
read -r l <&"99999999999999999999"  # all digits, out of range
```

Four separate divergences, all measured against bash 5.2.37:

| word | bash | osh |
|---|---|---|
| `""` (input) | `"$e": Bad file descriptor` | `"$e": ambiguous redirect` |
| `""` (output) | `"$e": Bad file descriptor` | `: No such file or directory` |
| `"abc"` | `abc: ambiguous redirect` | `"abc": ambiguous redirect` |
| `"-1"` | `-1: ambiguous redirect` | *(nothing printed)* |
| `"99999999999999999999"` | `…: Bad file descriptor` | `…: ambiguous redirect` |

**Root cause (bash's rule).** `do_redirection_internal` splits on
`all_digits(word)`, which is vacuously true for the empty string: an all-digit
word that is not a *legal number* (empty, or out of `int` range) is `BADFD`
— "Bad file descriptor" — while anything with a non-digit in it is
`AMBIGUOUS_REDIRECT`. osh appears to test "parses as a number" instead, which
sends the empty and the out-of-range cases down the ambiguous path. The empty
*output* case is worse still: `>&""` is being taken for a redirect to a file
named by the empty string, so it reports `No such file or directory` and never
reaches the dup logic at all. And `<&"-1"` fails silently, with no diagnostic.

The quoting difference is bash's too: the ambiguous message names the *expanded*
word (bash overwrites the redirect's word with the expansion before reporting),
while the bad-fd message names the word as it was written, quotes and all. osh
prints the source text in both.

**Impact.** Any script that duplicates onto a computed fd — which is exactly
what a disposed coproc's `<&"${R[0]}"` becomes — gets the wrong diagnostic, and
`>&""` gets the wrong *kind* of failure entirely. This is why
`coproc-is-disposed-of-when-it-is-reaped.sh` saves the fd numbers by hand rather
than reading through the unset array.

**Proper fix.** Classify exactly as bash does: expand the word, then branch on
"every byte is a digit" (empty included) → parse as an `i32` and report
`WORD: Bad file descriptor` on failure, else `WORD: ambiguous redirect`. Route
`>&WORD` through the same code as `<&WORD` so an empty word cannot fall through
to the file-redirect path, give `-1` its message, and pass the expanded word to
the ambiguous message and the raw source word to the bad-fd one.

**Fixed** for redirects written on a *command* (`interp.rs`): a `DupWord` enum
and `Shell::classify_dup_word` now split the word bash's way — `-` closes,
every-byte-a-digit (the empty word included) parses to an `i32` or is `BadFd`,
anything else is `NotAFd` — and `resolve_dup_out`/`resolve_dup_in` branch on it,
so `>&""` can no longer reach the file-redirect path and `<&"-1"` can no longer
fall off the end of the chain silently. The bad-fd message names the raw source
word, the ambiguous one the expansion. Corpus case:
`tests/corpus/dup-word-that-is-not-a-descriptor.sh`.

**Fixed** for `exec` too, in `TD-OILS-EXEC-DUP-WORD-MISCLASSIFIES` below — both
follow-ups (that one and `TD-OILS-DUP-BADFD-NAMES-THE-WRONG-THING`) are now
resolved, so the whole `<&WORD`/`>&WORD` classification is one shared model.
