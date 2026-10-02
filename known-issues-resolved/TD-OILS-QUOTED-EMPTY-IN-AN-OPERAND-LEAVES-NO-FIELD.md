### TD-OILS-QUOTED-EMPTY-IN-AN-OPERAND-LEAVES-NO-FIELD. `${x:-'' ''}` is one argument, not two — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `struct EChar`, which is a character
plus a quoted flag and therefore cannot represent *a quoted stretch that
produced no character*. `Shell::expand_word_annotated` tracks that today with
its `open` boolean, which says "this field exists even though it is empty" — but
`open` lives for the duration of a word and is gone by the time the operand's
characters are handed to the enclosing word's `split_run`.

**Reproduce** (`show() { printf '  %-10s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }`):

```sh
show 'two'  ${nope:-'' ''}      # bash (2)<><>      osh (0)
show 'mid'  ${nope:-'' x ''}    # bash (3)<><x><>   osh (1)<x>
IFS=:
show 'two'  ${nope:-'' ''}      # bash (1)< >       osh (1)< >   (agree)
```

**The rule.** A `''`/`""` inside a `:-`/`:+` operand leaves a mark that survives
the field splitting the substitution then undergoes: it is not a delimiter, it
is not removed before the scan, it makes its field exist, and it contributes
nothing to the text. Under `IFS=:` the two marks and the literal space end up in
one field, which is why that line already agrees — the divergence only shows
when the operand's own unquoted text *is* split. bash spells the mark `CTLNUL`
and drops it during quote removal.

Note what it is *not*: the field breaks a quoted `[@]` makes inside an operand
are already right (`${nope:-A"${a[@]}"B}` is `<Ap q><rB>`), because those are
breaks between whole fields, which `SplitItems::Fields` carries. This is a mark
*inside* one field.

**Which stretches leave one** — measured, and wider than the two lines above:
`''`, `""`, `"$unset"`, `"$null"`, `"$(true)"`, `$''` and `$""` each leave a
mark, and so does every *empty element* of a quoted list (`f=(''); ${x:-"${f[@]}" X}`
is `<><X>`). An empty list leaves none — `e=(); ${x:-"${e[@]}" X}` is `<X>`,
because it produced no field to be empty. Nothing unquoted leaves one.
`${x:=w}` is the exception that proves the rule: that operator answers with the
variable it just assigned, and the value went through quote removal on the way
in, so `${t:='' ''}` assigns one space and then splits it away to nothing.

**The fix.** `struct EChar { c: Option<Ch>, quoted: bool }`, `None` being the
mark (`EChar::MARK`, always `quoted` so that nothing can make it a delimiter).
The sentinel `EChar { c: Ch::B(0), quoted: true }` — bash's own trick — was
rejected: osh does not otherwise drop NUL bytes, so a real `$'\0'` would be
indistinguishable from the mark. Making the character optional instead means
every reader has to answer for it, which is the point; the answers are three,
and only the first is interesting:

* **field splitting** keeps it: `split_run`'s `is_ws`/`is_nonws` ask
  `EChar::as_char`, which a mark answers `None`, so it delimits nothing while
  still setting `open`.
* **quote removal** drops it: `echars_text` filters, and `drop_marks` does the
  same for the two paths that read a field as something other than text —
  `expand_word_fields` before globbing and `operand_chars` before a pattern.
* **the glob compiler** therefore never sees one, and says so where the
  compiler asks (`compile_glob`, `compile_class`, `cond_trace_pattern`).

The marks are made in `expand_word_annotated`, and only when
`mode == SplitMode::Operand` — the one context whose result is split again by
someone else. The `SingleQuoted` arm marks an empty `''`; the `DoubleQuoted` arm
marks each empty element of a quoted list, and marks the run itself when it
asked for a field and produced no character (which is what tells `"${e[@]}"`
from `"${f[@]}"`).

**Pinned by** `userspace/oils/tests/corpus/a-quoted-empty-in-an-operand-still-makes-a-field.sh`.

**Left behind:** the one place quote removal does *not* happen is a `case`, and
osh drops the mark there too — see
`TD-OILS-A-CASE-DOES-NOT-KEEP-THE-MARK-A-QUOTED-EMPTY-LEFT` above.
