### TD-OILS-A-QUOTED-LIST-LOSES-ITS-FIELDS-WHEN-ANYTHING-ELSE-IS-IN-THE-QUOTES. `"${a[@]}Z"` is one field where bash makes two — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::quoted_per_element_parts`.
It matches on the quoted run's parts as a slice, and every arm of that match is
a **one-element** pattern `[ WordPart::… ]`. So a quoted run is recognised as
per-element only when it is *nothing but* the list: add a single literal
character, or a second expansion, and the whole run falls through to
`Shell::expand_double_quoted`, which returns one string.

This is the last general gap in the quoted-list story;
TD-OILS-QUOTED-OPERAND-JOINS-A-QUOTED-AT (fixed) made the *operand* of a quoted
substitution lay its fields out correctly, and `lay_out_fields` is already the
routine that does the gluing. The run itself has never had the equivalent.

**Reproduce (all fixed):**

```sh
show() { printf '  %-20s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }
b=('p q' r)
show 'at then text'  "${b[@]}Z"          # bash: (2)<p q><rZ>       osh: (1)<p q rZ>
show 'text then at'  "Z${b[@]}"          # bash: (2)<Zp q><r>       osh: (1)<Zp q r>
show 'at then at'    "${b[@]}${b[@]}"    # bash: (3)<p q><rp q><r>  osh: (1)<p q rp q r>
show 'at then empty' "${b[@]}${nope}"    # bash: (2)<p q><r>        osh: (1)<p q r>
show 'at then star'  "${b[@]}${b[*]}"    # bash: (2)<p q><rp q r>   osh: (1)<p q rp q r>
show 'op then text'  "${b[@]:-d}Z"       # bash: (2)<p q><rZ>       osh: (1)<p q rZ>
```

Note `at then empty`: bash still makes two fields, because the field break is
the *list's*, and an expansion that contributes no characters after it cannot
close it up. And `at then star`: only the `[@]` breaks; the `[*]` beside it is
text that glues onto the last field.

**The rule.** Inside `"…"`, a `[@]`-spelled list breaks a field between each
pair of adjacent elements no matter what else shares the quotes. Text before it
joins the first element, text after it joins the last, and a second list starts
counting again from wherever the first one left off. This is exactly the layout
`lay_out_fields` implements for the operand case — first field joins what is
open, each of the rest starts a new one, the last is left open.

**The fix.** "Is this run per-element?" stopped being a whole-slice question and
became a per-part one. The `DoubleQuoted` arm of `Shell::expand_word_annotated`
now walks the run itself: literals and single-quoted text go straight onto the
open field, and every other part is offered to `quoted_per_element_parts` as a
slice of one. A `Some` goes through `lay_out_fields` — the same layout the
quoted operand uses, which is the point: a quoted run and a quoted operand are
the same kind of word. A `None` is `expand_dynamic`, exactly what
`expand_double_quoted` would have done with it. `bad_sub_word` and
`Shell::dquote` are set once around the whole loop, because the run is what a
diagnostic names and what the quoting is.

Two things had to be got right, and one of them was a real regression on the way:

* **The scalar-vs-list distinction still lives in the recogniser's arms.**
  `"${nope:+A}"` is one empty field where `"${z0[@]:+A}"` is none, so a part
  that is not list-valued must open a field even when it brought no characters.
  That falls out of the `None` branch setting `open` unconditionally.
* **An empty run has no parts at all**, so a loop over them says nothing —
  and `""` is one *empty* argument, not none. The whole-slice version got this
  by accident, via `expand_double_quoted(&[])` returning an empty string. The
  per-part version has to say it: `if parts.is_empty() { open = true; }`.
  Missing it made `a=("" "")` a zero-element array and broke three unit tests.

**Pinned by** `tests/corpus/a-quoted-lists-field-breaks-are-its-own.sh`.
