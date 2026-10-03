### TD-OILS-QUOTED-OPERAND-JOINS-A-QUOTED-AT. `"${x:-"${a[@]}"}"` is one field where bash makes two — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::expand_operand_fields`,
`Shell::expand_word_annotated`, and `Shell::quoted_per_element_parts`. The
symptom named in the title was only the visible corner of it: the operand of a
*quoted* substitution was expanded in `SplitMode::Operand`, the mode written for
the unquoted case, so every list in it was joined to text and the quotes that
should have reached it never did.

**Reproduce (all fixed):**

```sh
a=('p q' r); z0=(); ee=('' '')
printf '  <%s>\n' "${nope:-"${a[@]}"}"   # bash: <p q> <r>;  osh was: <p q r>
printf '  <%s>\n' "${nope:-${a[@]}}"     # the same — quoted in it or not
IFS=:; printf '  <%s>\n' "${nope:-${a[*]}}"   # one field, `p q:r`
printf '  <%s>\n' "${nope:-a:b}"              # still one field: nothing splits
unset IFS
printf '  <%s>\n' "${z0[*]:-"${a[@]}"}"  # two: the star joins elements, not the operand
printf '  <%s>\n' "${nope:-"${ee[@]}"}"  # two empty fields
```

**The rule (measured against bash 5.2.37).** Inside quotes the operand of a
`:-`/`:+` is simply **a double-quoted word**, and it behaves as one in every
respect. Nothing in it splits, under any `$IFS`, whether it is literal text
(`IFS=:; "${x:-a:b}"` is one field) or the result of an expansion. A `[@]` in it
keeps one field per element **whether or not it is itself quoted** — the quoting
that decides that is the substitution's, so `"${x:-${a[@]}}"` and
`"${x:-"${a[@]}"}"` are the same two fields. A `[*]` joins with `$IFS` for the
same reason. Empty elements of a quoted list are fields like any other, and text
glues to the ends at both the inner and the outer edge:
`X"${nope:-A"${a[@]}"B}"Y` is `<XAp q><rBY>`.

Two things are *not* this rule. The `[*]` spelling of the **outer** operator
joins its *elements*, never its operand — `"${z0[*]:-"${a[@]}"}"` is two fields,
because the star found no elements to join and an operand is a word. And a
scalar substitution is one field however little it holds, so an inactive
`"${nope:+A}"` is one empty argument where the `[@]` spelling of the same is
none at all.

This is the quoted half of the rule TD-OILS-OPERAND-QUOTING-IS-LOST records for
the unquoted case, where the operand's characters are handed to the enclosing
word's own scan. Here there is no scan to hand them to, so the only breaks are
the ones a list brings with it.

**The fix.** A fourth `SplitMode::QuotedOperand`, chosen in
`Shell::expand_operand_fields` off the existing `Shell::dquote`. Its branch in
`expand_word_annotated` takes the `SplitItems` classification straight: a `List`
becomes one field per element, a `Joined` is `$IFS`-joined, and everything else
is text — with `lay_out_fields` (a new free function shared with the unquoted
branch) doing the glue so `A"${a[@]}"B` attaches at both ends.

Three subtleties were worth the trouble they caused:

* **`part_is_star(other)`, not the `SplitItems` variant, decides the join.**
  `split_items` reports an *unquoted* `[*]` as a plain `List`, which is right
  where things split and wrong here; inside these quotes the spelling is all
  that is left to ask.
* **The `[*]` join for a quoted `${a[*]:-w}` happens inside
  `quoted_per_element_parts`**, whose `ArrayOp` arm no longer pins `star:
  false`. Returning `None` to decline to the scalar path would run the operator
  — and make its complaints — a second time.
* **`SplitItems::quoted_fields` keeps an operand's own fields** rather than
  joining them, and turns an *empty* `Fields` into one empty field. That last is
  the whole difference between the two operators: `"${a[@]:-}"` on an empty array
  is one empty argument where the inactive `"${a[@]:+A}"` is none.

**Pinned by** `tests/corpus/a-quoted-operand-is-a-quoted-word.sh`, which walks
the whole rule and both of its near-misses.
