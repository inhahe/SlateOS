### TD-OILS-A-LIST-IN-AN-OPERAND-TAKES-THE-OTHER-MARKS-WITH-IT. `case a in ${x:-''"${f[@]}"a})` does not match — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::expand_word_annotated`'s
`SplitMode::Operand` marking, read back by `Shell::operand_chars`. Visible only
through a `case`, which is the one construct that does not remove the marks
(`TD-OILS-A-CASE-DOES-NOT-KEEP-THE-MARK-A-QUOTED-EMPTY-LEFT` above).

**Reproduce** — all three are `no` in osh and `match` in bash:

```sh
f=('')
case a   in ${nope:-"${f[@]}"''a}) echo match;; *) echo no;; esac
case a   in ${nope:-''a"${f[@]}"}) echo match;; *) echo no;; esac
case abc in ${nope:-"${f[@]}"a*})  echo match;; *) echo no;; esac
```

**The rule** — measured by asking a `case` how long its subject is
(`for p in '?' '??' '???'; do case ${nope:-…} in $p) echo ${#p};; esac; done`),
with `f=('')`, `g=('' '')`, `h=('' x '')`, `e=()`. `D` below is the mark, the
byte `\177`:

| operand | length | what it is |
|---|---|---|
| `''a` | 2 | `Da` |
| `''''a` | 3 | `DDa` — one mark per stretch |
| `''a"$*"` | 3 | `DaD` — `[*]` is not a list |
| `''a${e[@]}` | 2 | `Da` — an *unquoted* list is not one either |
| `"${f[@]}"a` | 1 | `a` — the empty element merged, so no mark |
| `''"${f[@]}"''a` | 1 | `a` — **both** `''` marks gone |
| `''a"${f[@]}"` | 1 | `a` |
| `''a"$@"` (no args) | 1 | `a` — an empty list still does it |
| `"${g[@]}"a` | 3 | `D a` — the element that stayed an empty *field* keeps one |
| `"${h[@]}"a` | 5 | `D x a` |

So: **a quoted `[@]` list anywhere in the operand drops every mark the ordinary
quoted stretches made, and marks the fields that came out empty instead.** That
is bash's two code paths in `expand_word_internal` — the `istring` one, whose
`QUOTED_NULL`s stand, and the `WORD_LIST` one, which joins the words and where
only a wholly empty word was `quote_string`d into a `CTLNUL`.

It is a **joined** reading of the operand only. In the splitting context the two
rules are indistinguishable, and osh is already right there:
`${nope:-'' "${e[@]}" X}` is `<><X>` and `${nope:-'' "${f[@]}" X}` is
`<><><X>` in both shells.

**It is not only the marks.** The `WORD_LIST` path joins *words*, so the text
between the list's breaks is word-split before anything joins it — which shows
up with the marks removed and is therefore a plain, mark-free difference too:

```text
${x:-''  X}          \177  X   → assigned as `  X`   — istring: two spaces of text
${x:-"${f[@]}"  X}   \177 X    → pattern `\177 X`    — word list: one delimiter
```

**Which readers split.** bash does it except in three contexts, and they are the
three it names: an assignment's RHS (`PF_ASSIGNRHS`), a here-document or
here-string (`Q_HERE_DOCUMENT`), and the inside of double quotes
(`Q_DOUBLE_QUOTES`). So a `case` word, a `[[ ]]` operand and every pattern split;
`v=${x:-"${f[@]}"  X}` and `cat <<< ${x:-"${f[@]}"  X}` keep both spaces. (A
redirect target splits too — `> ${x:-"${f[@]}"  X}` is an *ambiguous redirect*.)

**The fix.** A `Shell::saw_quoted_list` flag, set by the seven list arms of
`Shell::quoted_per_element_parts` (the `[@]` spellings only — `"$*"`/`"${a[*]}"`
join to one string and are not lists), saved/cleared around the operand by
`Shell::expand_operand_fields` so a *nested* operand's list stays its own, and
left in `Shell::operand_saw_list` for `Shell::operand_chars`, which is above
`Shell::split_items` and so cannot watch the flag itself.

`Shell::operand_chars` then takes a `split_words` argument — `true` from
`Shell::expand_word_pattern_inner`, `self.cond_word` from
`Shell::expand_word_joined_annotated` (which is how a `[[ ]]` operand and a
`case` word part company from an assignment's value and a here-document), `false`
from the `SplitMode::Text` arm, that being the posix-mode redirect word bash does
not split. When both it and the flag are set, `Shell::word_list_fields` rebuilds
the operand's fields into bash's words: `split_run` inside each field exactly as
the `SplitMode::Fields` arm does over the same fields, then `drop_marks` every
word and give a `EChar::MARK` to each that came out empty.

**Found:** while fixing `TD-OILS-A-CASE-DOES-NOT-KEEP-THE-MARK-A-QUOTED-EMPTY-LEFT`;
the corpus case there covers everything but this and said so.

**Pinned by** `userspace/oils/tests/corpus/a-list-in-an-operand-makes-it-a-word-list.sh`,
and the two lines the `case` corpus case had been holding back.
