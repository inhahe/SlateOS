### TD-OILS-QUOTED-OPERAND-WORD-LIST-DOES-NOT-SPLIT-ITS-LITERAL. `IFS=:; "${x:-${a[@]}:Z}"` is two fields in osh and three in bash — 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the `SplitMode::QuotedOperand` arm of
`Shell::expand_word_annotated`, whose `Some(SplitItems::Fields(items))` and
`Some(SplitItems::List(items))` branches lay the list's breaks out as fields and
then let the literal text around them join on unsplit.

**What.** The `WORD_LIST` path described in
`TD-OILS-A-DEFAULT-WORDS-NESTED-LIST-IS-NOT-SPLIT` is reached from inside double
quotes too, and there the *elements* are protected (as quoting says) but the
**literal text** between and after them is still word-split. With
`a=(p:q r:s)`:

```sh
IFS=:    printf '<%s>' "${x:-${a[@]}:Z}"    # bash <p:q><r:s><Z>   osh <p:q><r:s:Z>
IFS=:    printf '<%s>' "${x:-Z:${a[@]}}"    # bash <Z><p:q><r:s>   osh <Z:p:q><r:s>
IFS=' :' printf '<%s>' "${x:-${a[@]}:Z}"    # bash <p:q><r:s><Z>   osh <p:q><r:s:Z>
IFS=:    printf '<%s>' "${x:-"${a[@]}":Z}"  # bash <p:q><r:s><Z>   osh <p:q><r:s:Z>
IFS=:    printf '<%s>' "${x:-${!a[@]}:Z}"   # bash <0><1><Z>       osh <0><1:Z>
```

**The rule, measured (2026-08-04).** It is *not* general field splitting, and the
earlier reading of it here — "whitespace `$IFS` behaves differently" — was wrong.
Exactly **three characters** can split the literal, and only when they are
literal source text: `:`, `=` and `~`. With `a=(p:q r:s)`, `printf '<%s>'`:

| operand | `IFS=:` | `IFS='='` | `IFS='~'` | `IFS=' '` | `IFS=$'\t'` |
|---|---|---|---|---|---|
| `"${x:-${a[@]}:Z}"` | `<p:q><r:s><Z>` | `<r:s:Z>` | `<r:s:Z>` | `<r:s:Z>` | `<r:s:Z>` |
| `"${x:-${a[@]}=Z}"` | `<r:s=Z>` | `<p:q><r:s><Z>` | `<r:s=Z>` | `<r:s=Z>` | `<r:s=Z>` |
| `"${x:-${a[@]}~Z}"` | `<r:s~Z>` | `<r:s~Z>` | `<p:q><r:s><Z>` | `<r:s~Z>` | `<r:s~Z>` |
| `"${x:-${a[@]} Z}"` | `<r:s Z>` | `<r:s Z>` | `<r:s Z>` | `<r:s Z>` | `<r:s Z>` |
| `"${x:-${a[@]}$'\t'Z}"` | `<r:s\tZ>` | … | … | … | `<r:s\tZ>` |

(the last four columns abbreviate the trailing field; the first two are always
`<p:q><r:s…>`, the list's own breaks). A literal **space** or **tab** never
splits, even when it is `$IFS`. The prefix position behaves the same way:
`"${x:-Z:${a[@]}}"` splits under `IFS=:` and `"${x:-Z=${a[@]}}"` under `IFS='='`.

Three more facts pin the mechanism down:

* the character must be **literal source text**. `"${x:-${a[@]}$(echo :)Z}"` is
  `<p:q><r:s:Z>` under `IFS=:` — a `:` that arrived through an expansion does
  not split;
* a **quoted** one splits *and leaks its quote characters into the result*:
  under `IFS=:`, `"${x:-${a[@]}':'Z}"` is `<p:q><r:s'><'Z>` and
  `"${x:-${a[@]}\:Z}"` is `<p:q><r:s\><Z>`;
* `:=` is not on this path at all: `"${y:=${a[@]}:Z}"` is the single field
  `<p:q r:s:Z>` under every `$IFS`.

**Hypothesis.** This is bash's *assignment-style tilde expansion*
(`tilde_additional_prefixes = {"=~", ":~"}`, `tilde_additional_suffixes =
{":", "="}`), which rebuilds the word from its source text around exactly those
delimiters and drops the `CTLESC` protection on them. `~` splits only where a
tilde prefix could begin, which is why it is in the set at all. The quote-
character leak is the tell that this is a bash **bug** rather than a designed
rule, so bug-for-bug parity here would mean reproducing the leak too.

**Impact.** A quoted `${x:-…}`/`${x:+…}` whose operand holds a list *and* a
literal `:`, `=` or `~`, under an `$IFS` naming that character. Invisible under
the default `$IFS`, which is why it survived the unquoted half of the same rule
being fixed. Low priority, and deliberately not implemented: the shape is
vanishingly rare and matching it exactly means matching a leak.

**Not this entry:** the `[[ ]]`/`case` half of the quoted word-list path, which
*is* ordinary field splitting and *is* implemented — see
`TD-OILS-A-COND-WORD-THAT-MET-A-QUOTED-AT-LIST-IS-BUILT-OUT-OF-WORDS`.
