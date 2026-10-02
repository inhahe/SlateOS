### TD-OILS-A-COND-WORD-THAT-MET-A-QUOTED-AT-LIST-IS-BUILT-OUT-OF-WORDS. `IFS=:; [[ P:Q"${n[@]^^}" ]]` sees `P:QA:B C:D` in osh and `P QA:B C:D` in bash — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — the three cond-word funnels
(`Shell::expand_word_joined_annotated`, `Shell::expand_word_pattern_inner`,
`Shell::regex_pattern_parts`), which took the word-list path only for an
*unquoted* `[@]`, and glued a double-quoted section in as one flat run of quoted
characters.

**What.** The entry above established that an **unquoted** `[@]` moves a
`[[ ]]`/`case` word onto bash's `WORD_LIST` path. A **quoted** one does too, and
on its own terms. With `n=(a:b c:d)`, reading `BASH_REMATCH[1]` out of
`[[ … =~ ^(.*)$ ]]`:

| `$IFS` | `P:Q"${n[@]^^}"` | `P:Q${n[@]^^}` (unquoted, for contrast) |
|---|---|---|
| `:` | `P QA:B C:D` | `P QA B C D` |
| `: ` | `P QA:B C:D` | `P QA B C D` |
| `\t:` | `P QA:B C:D` | `P QA B C D` |
| ` :` | `P QA:B C:D` | `P:QA:B C:D` — the unquoted gate is off |
| ` ` | `P:QA:B C:D` | `P:QA:B C:D` |
| (null) | `P:QA:B C:D` | `P:QA:B C:D` |

**The rule, measured.**

- The quoted list's **elements are words** and keep every character they hold.
- **Everything around them** — literals, scalars, command substitutions, and any
  unquoted list in the same word — is text that splits on `$IFS` by ordinary
  field splitting (empty fields kept for a non-whitespace separator, whitespace
  runs collapsed, a trailing empty dropped).
- The finished words are glued back with **single spaces**.

**The gate is not the unquoted spelling's.** That one wants `$IFS`'s first
character not to be a space; this one asks only that `$IFS` be **non-null**. The
` ` and ` :` rows above are the difference: under `IFS=' '` the quoted path is
*on* and simply finds nothing in `P:Q` to split, whereas under `IFS=' :'` the
unquoted path is off and the quoted one splits. A word holding both spellings
shows the two gates at once.

**A `case` arm's pattern reads the finished list differently.** Every other cond
context glues the words with spaces (bash's `cond_expand_word` runs `string_list`
over the list); `execute_case_command` instead reads `es->word->word` — the
**head** — and drops the rest. So under `IFS=:`:

```sh
case X in P:Q"${n[@]^^}")      # pattern is P            — matches `P` alone
case X in "${n[@]^^}"X:Y)      # pattern is A:B
case X in P:Q${n[@]^^})        # pattern is P QA B C D   — unquoted, still one word
[[ 'P QA:B C:D' == P:Q"${n[@]^^}" ]]   # true — the `[[ ]]` pattern joins
```

A one-element or empty array still gives `P`, because it is the literal `P:Q`
that split. Under `IFS=' '` the pattern is `P:QA:B`, and under a null `$IFS` the
path is off and the pattern is the whole `P:QA:B C:D`.

**The `:-`/`:+` family is dynamic, not static.** It is a list only on the branch
that answers *for the operator*. With `IFS=:` and `e=()`:
`[[ P:Q${e[@]:-A:B} ]]` is `P:QA:B` (the operand answered — no list) but
`[[ P:Q${e[@]:+A:B} ]]` is `P Q` (the operator answered, with nothing — still a
list). The previous commit's static `part_is_at_list` got both of these wrong;
this was found by the corpus case, not by hand.

**Impact.** A `[[ ]]` operand, a `[[ == ]]`/`case` pattern, a `case` subject or a
`=~` right-hand side that holds a quoted `[@]` under any non-null `$IFS`.

**Fixed** by:

- `Shell::saw_quoted_at_list` — the quoted counterpart of `saw_at_list`, scoped
  around the word by all three funnels. `Shell::note_quoted_list` sets it from
  every arm of `quoted_per_element_parts`, and `Shell::note_cond_at_list` routes
  the `:-`/`:+` family to whichever of the two flags the `dquote` state names.
- `Shell::cond_quoted_list_on` — the gate: `cond_word`, not inside double quotes,
  `$IFS` non-null. Nothing about a leading space.
- `part_is_at_list` lost its `ArrayOp` arm. That family cannot answer before it
  runs, so `Shell::array_op_fields` — the one place that knows which branch it
  took — reports it instead, at the end of its own body.
- `Shell::cond_dquote_items` replaces `expand_double_quoted` in the three
  funnels. It returns the quoted section as a **list of items**, breaking where a
  quoted list's elements part, and the funnels lay those out with
  `lay_out_fields` into a `Vec<Vec<EChar>>` that `Shell::word_list_fields` then
  splits *inside* each field. A flat buffer with a quoted separator cannot
  express this: nothing would split the separator again, so under `IFS=' '`
  `P:Q"${n[@]^^}"` would come back as one word where bash has two. Off the path
  the helper returns a single item glued with `at_sep`'s space, which is exactly
  what `expand_double_quoted` produced before.
- `Shell::cond_first_word` is the `case`-arm read of the same list, selected by
  the new `case_arm` parameter of `expand_word_pattern_inner` (`true` only from
  `expand_case_pattern`) and only while the *quoted* path is on.

Corpus: `a-cond-word-that-met-a-quoted-at-list-is-built-out-of-words-too` — seven
`$IFS` regimes × ~55 rows: both orders of literal and quoted list, fully quoted,
one-element and empty lists, every `[@]` spelling, scalars carrying separators no
literal could be written with, the `P::Q`/`:P`/`P:` empty-field shapes, command
substitution, both spellings in one word, the `[*]`/length/indirect/cmdsub
non-lists, the `${x:-…}` branches from both sides of the quotes, the eight
`[@]:-` rows, and the `case` subject/pattern, `=~` right-hand side and
`[[ == ]]` pattern — the pattern rows written as a sweep over candidate subjects
so the first-word rule is pinned rather than merely observed.
