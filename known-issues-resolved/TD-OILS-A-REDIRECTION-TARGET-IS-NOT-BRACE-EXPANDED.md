### TD-OILS-A-REDIRECTION-TARGET-IS-NOT-BRACE-EXPANDED. `> f{1,2}` writes a file called `f{1,2}` where bash calls it ambiguous — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/interp.rs` — wherever a redirection's target word
is expanded. Every other word list that bash brace-expands goes through
`Shell::expand_braces_opt`; a redirection target does not, and it should.

**Reproduce.**

```sh
echo hi > f{1,2}
echo "rc=$?"
ls f*
```

| | bash 5.2.37 | osh |
|---|---|---|
| stderr | `line 1: f{1,2}: ambiguous redirect` | — |
| `rc` | 1 | 0 |
| files | none | `f{1,2}` |

**What bash does.** `redirection_expand` calls `expand_words_no_vars`, which is
`expand_word_list_internal (list, WEXP_NOVARS)`, and `WEXP_NOVARS` is
`WEXP_ALL & ~WEXP_VARASSIGN` — so `WEXP_BRACEEXP` is still set. The word is
brace-expanded like any other, and a target that expands to more than one word
is the `ambiguous redirect` error.

**The fix — done.** `Shell::expand_redirect_word` now loops over
`self.expand_braces_opt(w)` and expands each word that comes back, so the field
count `Shell::expand_redirect_word_once` already enforced is the count *after*
brace expansion. Nothing else moved: the ambiguity complaint, the single-field
rule and the `RedirWord::Dup` quoting all sit above this and were already right.

Two consequences worth naming, both measured:

* The count is what matters, not the braces. `> h{a,a}` makes two words that
  happen to be equal and is still ambiguous; `> g{1}` was never a brace
  expansion (no comma) and just opens that file.
* It brings the target under `Shell::gobble_scan`, which is what bash does for
  the same reason. A scan abort makes `expand_braces_opt` hand back *no* words,
  and that lands on the fatal-expansion-error path rather than the ambiguity
  one — matching bash, which reports only `command substitution: line N:` for
  `echo hi > "/dev/null${z:-'$(fi)'}"`, quoting the whole word (`` `fi)'}"' ``).

**Tests.** `tests/corpus/a-redirection-target-is-brace-expanded-and-then-must-be-one-word.sh`,
plus the redirection row restored to
`tests/corpus/the-brace-scanner-reads-the-command-substitutions-a-single-quote-hid.sh`.
