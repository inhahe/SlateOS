### TD-OILS-A-DEFAULT-WORDS-NESTED-LIST-IS-NOT-SPLIT. an operand that met an unquoted `[@]` is built out of words, not out of text — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — the `SplitMode::Operand` arm of
`Shell::expand_word_annotated`, and the two readers that take an operand's
fields back (`SplitMode::Fields`' and `SplitMode::Operand`'s
`Some(SplitItems::Fields(items))` branches).

**Note on the entry as first written.** The three examples it led with —
`IFS=:; ${e[@]:-"${n[@]}"}`, `IFS=; ${e[@]:-${n[@]}}`, `IFS=; ${u:-${n[@]}}` —
all pass today, and its "Proper fix" (return the operand's *fields*; model
`W_SPLITSPACE`) had already been implemented by the time it was re-read:
`expand_operand_fields`, `SplitMode::Operand` and `Shell::saw_quoted_list`
between them are exactly that. The entry was stale, not wrong-at-the-time. What
was still divergent was a different and larger half of the same rule, measured
below, and the entry has been rewritten around it rather than closed silently.

**What.** bash expands a word along one of two paths: an `istring`, whose answer
is text the surrounding word then splits on `$IFS`, and a `WORD_LIST`, whose
answer is a list of *words*. An **unquoted** `[@]` anywhere in a `${x:-…}`
operand moves the whole operand onto the second path — and a list of words is
glued back together with single **spaces**, not with `$IFS`, and the glue is
final. osh had none of this: it joined the elements and then split the result
like any other text.

Two things follow, and `$IFS` decides which of them apply. With
`a=(p:q r:s)`:

| shape | `$IFS` | bash | osh (before) |
|---|---|---|---|
| `${x:-${a[@]}}` | `:` | `<p:q r:s>` | `<p><q r><s>` |
| `${x:-${a[@]}}` | `': '` | `<p:q><r:s>` | `<p><q><r><s>` |
| `${x:-${a[@]}}` | `' :'` | `<p><q><r><s>` | `<p><q><r><s>` |
| `${x:-${a[@]}:Z}` | `:` | `<p:q r:s Z>` | `<p><q r><s><Z>` |
| `${x:-${a[@]:0}}` | `:` | `<p q r s>` | `<p><q r><s>` |
| `${x:-${a[@]^^}}` | `:` | `<P Q R S>` | `<P><Q R><S>` |
| `${x:-${a[@]}}` (in `d/` holding `aa`, `ab`, with `a=('a*')`) | `:` | `<a*>` | `<aa><ab>` |
| `c=(':' 'q'); ${x:-${c[@]}}` | `:` | `<: q>` | `<><q>` |

The two rules the table is made of:

  * a **plain** `[@]` — the array named and nothing more (`$@`, `${a[@]}`, or
    `${a[@]:-w}` on the branch where it answers with the array) — contributes
    its elements as *words*. Their own characters stop being the operand's to
    split or to glob, and the single space put between them is the only
    separator the list itself adds. A **derived** `[@]` (`${a[@]:0}`,
    `${!a[@]}`, `${a[@]^^}`, `${a[@]#p}`) hands over text bash has already made
    one string of, so its characters split like any other;
  * then, if `$IFS` names no space **at all**, the operand is word-split and the
    words are glued back with single spaces into **one finished field**, which
    neither splits again nor globs.

The question `$IFS` is asked is not quite the same one twice — joining asks
whether a space is a separator at all, protection asks whether it is the *first*
separator named — which is what makes three regimes out of two rules, and why
the ordinary `$IFS` (leading space) hides both.

A `[*]` is never this path, quoted or not; nor is a command substitution, whose
output is text. A **quoted** `"${a[@]}"` is the other half of the same
`WORD_LIST` path and was already modelled — see `Shell::saw_quoted_list`.

**Fixed.** `Shell::saw_at_list` / `operand_saw_at_list` record an unquoted `[@]`
for the operand that is being expanded, scoped exactly as `saw_quoted_list` is.
`part_is_plain_at` separates the plain spelling from the derived ones — it is
consulted only where `split_items` has already peeled off
`SplitItems::Fields`, so an `ArrayOp` that reaches it is one that answered with
its array rather than with its word. `Shell::ifs_has_space` /
`ifs_leads_with_space` are the two questions, and `Shell::operand_word_list` is
the join, wired into both readers of an operand's fields.

Corpus:
`an-operand-that-met-an-unquoted-at-list-is-built-out-of-words.sh` — the three
regimes, the separator that comes back as a space, plain vs derived `[@]`, the
`[*]` contrast, elements holding `$IFS` characters or nothing, the glob test,
an empty list, `${a[@]:-w}` on both its branches, a nested operand finishing its
own word list first, and the `:+`/`:=` operands.

**Still divergent (separate entry):**
`TD-OILS-QUOTED-OPERAND-WORD-LIST-DOES-NOT-SPLIT-ITS-LITERAL`.
