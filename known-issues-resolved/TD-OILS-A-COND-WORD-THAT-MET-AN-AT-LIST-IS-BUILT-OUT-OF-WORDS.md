### TD-OILS-A-COND-WORD-THAT-MET-AN-AT-LIST-IS-BUILT-OUT-OF-WORDS. `IFS=:; [[ ${n[@]^^} ]]` sees `A:B C:D` in osh and `A B C D` in bash — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::expand_word_joined_annotated`
(the funnel for `expand_cond_string` and `expand_case_subject`) and
`Shell::expand_word_pattern_inner` (for `expand_cond_pattern` and
`expand_case_pattern`), neither of which knew about the word-list path. They
glued the parts into one buffer and let `Shell::at_sep`'s `cond_word` space stand
as the whole of the context's rule.

**What.** `TD-OILS-A-DEFAULT-WORDS-NESTED-LIST-IS-NOT-SPLIT` established that an
unquoted `[@]` moves a `${x:-…}` operand onto bash's `WORD_LIST` path. It moves a
`[[ ]]` operand and a `case` word onto the same path, and osh implements the rule
only for the operand. With `n=(a:b c:d)`, reading `BASH_REMATCH[1]` out of
`[[ … =~ ^(.*)$ ]]`:

| word | `IFS=:` bash | osh before |
|---|---|---|
| `${n[@]}` | `a:b c:d` | ✓ |
| `${n[@]^^}` | `A B C D` | ✗ `A:B C:D` |
| `${n[@]#a}` | ` b c d` | ✗ `:b c:d` |
| `${n[@]@Q}` | `'a b' 'c d'` | ✗ `'a:b' 'c:d'` |
| `${n[@]/a/Z}` | `Z b c d` | ✗ `Z:b c:d` |
| `${n[@]:0}` | `a b c d` | ✗ `a:b c:d` |
| `${@#a}` | ` b c d` | ✗ `:b c:d` |
| `${n[*]^^}` | `A:B:C:D` | ✓ |
| `"${n[@]^^}"` | `A:B C:D` | ✓ |
| `P:Q${n[@]^^}` | `P QA B C D` | ✗ `P:QA:B C:D` |
| `P:Q${n[@]}` | `P Qa:b c:d` | ✗ `P:Qa:b c:d` |
| `$v${n[@]^^}` (`v=y:z`) | `y zA B C D` | ✗ `y:zA:B C:D` |
| `P:Q$(echo m:n)${n[@]^^}` | `P Qm nA B C D` | ✗ |
| `${x:-${n[@]^^}}` | `A B C D` | ✗ `A:B C:D` |
| `P:Q` alone | `P:Q` | ✓ |
| `P:Q${n[*]^^}` | `P:QA:B:C:D` | ✓ |
| `P:Q$v` (`v=y:z`) | `P:Qy:z` | ✓ |

**The rule, measured.** A `[[ ]]`/`case` word that contains an unquoted `[@]`
(any spelling but `[*]`, and an *empty* array still counts) is built as a list of
words and then glued with single spaces:

- the **elements of a plain `[@]`** (`${n[@]}`, `$@`, and the `:-` family on the
  branch that answers with the array) are words — their own characters do not
  split;
- **everything else** contributes text whose characters split on `$IFS` —
  literals, scalars, command substitutions, and the elements of a *derived*
  `[@]` (`:off`, `^^`, `#p`, `@Q`, `/a/Z`), which are first glued with `$IFS`'s
  first character;
- **quoted** stretches are protected, as ever: `"P:Q"${n[@]^^}` is
  `P:QA B C D`.

**The gate is `$IFS`'s first character, not merely a space anywhere in it.** This
differs from the operand's join gate (`Shell::operand_word_list`, which asks
`ifs_has_space`) and wants keeping straight:

| `$IFS` | `[[ ${n[@]^^} ]]` | splits? |
|---|---|---|
| `:` | `A B C D` | yes |
| `: ` | `A B C D` | yes — a space that is not first does not stop it |
| ` :` | `A:B C:D` | no |
| ` ` | `A:B C:D` | no |
| (null) | `A:B C:D` | no |
| `\t:` | `A B C D` | yes — only a *space* is special, not whitespace |
| `:\t` | `A B C D` | yes |
| `\n:` | `A B C D` | yes |

**The `$IFS`-first-character glue is load-bearing**, not a space: with
`z=(':' 'x')`, `[[ ${z[@]^^} ]]` is `  X` (two spaces) under `IFS=:` but ` X`
under `IFS=$'\t:'`, because the glue is a tab there and a tab is `$IFS`
whitespace, which absorbs the empty field the leading `:` would otherwise leave.
Gluing with a space and splitting afterwards gets the first right and the second
wrong.

**Impact.** A `[[ ]]` operand, a `[[ == ]]`/`case` pattern or a `case` subject
that holds an unquoted `[@]` under a non-default `$IFS` whose first character is
not a space. Invisible under the default `$IFS`, and invisible under any `$IFS`
whose characters do not occur in the data — which is why the existing corpus
coverage of this context did not catch it.

**Fixed** by mirroring, at the cond-word funnels, what `Shell::operand_word_list`
and `part_is_plain_at` already do for the operand:

- `Shell::cond_word_list_on` is the `$IFS` half of the gate — `cond_word`, not
  inside double quotes, `$IFS` non-null and not leading with a space. Its doc
  carries the six-row table above, including why the question is narrower than
  `operand_word_list`'s `ifs_has_space`.
- `part_is_at_list` is the static "this part is an unquoted, non-`[*]` `[@]`"
  predicate. Static because an *empty* array still puts the word on the path, so
  the answer cannot wait for the expansion. `Indirect`/`IndirectOp` are excluded:
  what they resolve to is not knowable here, and `Shell::cond_word_indirect` is
  their own rule.
- Both funnels scope a `Shell::saw_at_list` around the word (saving and restoring
  the outer one, so a list inside a *nested* operand stays that operand's
  business), set it from `part_is_at_list`, and finish by running
  `Shell::cond_word_list` over the single buffer — `word_list_fields` followed by
  an unquoted-space re-glue.
- `Shell::cond_plain_at` / `Shell::cond_plain_at_chars` push a **plain** `[@]`'s
  elements as *quoted* characters with an unquoted `star_sep()` between them, so
  the element boundary is a real delimiter and the elements' own `$IFS`
  characters are not. `Shell::operand_chars` applies the same rule on the branch
  where `${a[@]:-w}` answers with the array — the only place that branch can be
  told apart from the one where the operand's own fields are the answer.
- `Shell::at_sep` returns `star_sep()` rather than its `cond_word` space while
  the path is on, since the derived join has to be the real separator (the
  `z=(':' 'x')` evidence above), and `Shell::join_derived_nosplit` is the
  `joined_value` join that routes the slice, key-list and bulk arms through it.
- `Shell::regex_pattern_parts` — a `=~` right-hand side — is a cond word on the
  same terms (`IFS=:; [[ 'P QA B C D' =~ ^P:Q${n[@]^^}$ ]]` matches in bash), and
  had to start building its pattern quote-annotated to take the path. The flag it
  carries is only about splitting: the ERE escaping a quoted stretch needs has
  already happened by the time its characters are pushed.
- `Shell::operand_chars` was the fourth site: it asked only
  `split_words && self.operand_saw_list`, which records a *quoted* list. It now
  also reports `operand_saw_at_list` up into the funnel's `saw_at_list`, so
  `[[ ${x:-${n[@]^^}} ]]` takes the final pass. Read only on the branch where the
  operand actually expanded, the same discipline `operand_saw_list` follows.

Corpus: `a-cond-word-that-met-an-unquoted-at-list-is-built-out-of-words`, which
pins the seven-`$IFS` gate, plain vs. derived, the literal/scalar/command-
substitution halves, the `[*]` and double-quote exclusions, the `z=(':' 'x')`
glue evidence, the `${x:-…}` operand rows, the `=~` right-hand side, and the
`case` subject/pattern and `[[ == ]]` pattern spellings.

Out of scope here, and since resolved on its own terms: the *quoted* half. A
quoted `[@]` moves the word onto the path too — see
`TD-OILS-A-COND-WORD-THAT-MET-A-QUOTED-AT-LIST-IS-BUILT-OUT-OF-WORDS`.
