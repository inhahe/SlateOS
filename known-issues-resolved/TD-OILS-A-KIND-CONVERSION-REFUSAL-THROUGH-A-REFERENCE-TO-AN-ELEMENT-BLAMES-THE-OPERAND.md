### TD-OILS-A-KIND-CONVERSION-REFUSAL-THROUGH-A-REFERENCE-TO-AN-ELEMENT-BLAMES-THE-OPERAND. `declare -n r='n[1]'; declare -A r` says `r:` where bash says `n:` — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_declare_scoped`, the
indexed/associative conversion refusal.

**What.** The *scalar* operand path has the same misplaced blame the entry above
describes for the compound one, and it shows on a valueless operand, where there
is no literal involved at all. With `n=(a b c)` and a `declare -n r='n[1]'`:

```text
                                       bash                     osh
declare -A r                           `declare: n: cannot      `declare: r: cannot
  (top level)                          convert indexed to       convert indexed to
                                       associative array`       associative array`
f() { declare -gA r; }                 the same, `n:`           the same, `r:`
declare -a r                           nothing, s=0             the same — agreed
```

The status and the fact of the refusal agree; only the name in it is wrong.
Attributes through an element reference land on the base array, so the base is
what the conversion is about and what bash names.

**Proper fix.** Name `target.base` in the refusal rather than the operand, on the
same terms the store already follows the reference.

**Impact.** A misleading diagnostic: a script's error output names a variable
that was never the one being converted.

**Fixed.** `builtin_declare_scoped` now computes a `kind_blame` beside
`base_name`/`operand_name` and hands *that* to the conversion refusal. It is the
operand as written — reference and all, so a reference to a plain name still
reports `r` — except where the resolved target carries a subscript and the
operand did not bind the spelling, in which case it is the target's base. An
operand written with a subscript of its own (`declare -A 'n[1]'`) already
reported its base, since that is what `base_name` is; the new name only makes
the two paths agree. The self-conflict refusal just below (`declare -aA r`)
still names the target, because that one is raised against the array the command
has by then already made. Corpus:
`tests/corpus/a-kind-conversion-refusal-through-a-reference-to-an-element-blames-the-base.sh`.
