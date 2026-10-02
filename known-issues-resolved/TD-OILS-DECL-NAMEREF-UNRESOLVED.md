### TD-OILS-DECL-NAMEREF-UNRESOLVED. The declaration builtins do not follow a nameref — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare` and the
`export`/`readonly` operand loops, which apply attributes to the operand
name itself rather than to the name a `-n` reference resolves to.

bash resolves a nameref before applying an attribute, so a declaration
naming the reference declares the *target*:

```sh
w=5; declare -n r=w
declare -i r; declare -p w   # bash declare -i w="5"        osh declare -- w="5"
declare -x r; declare -p w   # bash declare -x w="5"        osh declare -- w="5"
readonly r;   declare -p w   # bash declare -r w="5"        osh declare -- w="5"
declare -a r; declare -p w   # bash declare -a w=([0]="5")  osh declare -- w="5"
declare -a r; declare -p r   # bash declare -n r="w"        osh declare -an r=([0]="w")
```

The last line is the damaging one: osh applies the array kind to `r`
itself, which converts the reference's *own* value — the target name —
into element 0 of an array, so the nameref is destroyed and every later
`r=…`/`$r` addresses the array instead of `w`. Assignment through a
reference is resolved correctly already (`resolve_ref_use` at the top of
`apply_assignment`, so `r[1]=9` reaches `w`); it is only the declaration
builtins that skip the step.

The fix is to resolve the operand through `resolve_ref_use` — as
`apply_assignment` does — before the attribute/kind is applied, in both
the `declare`/`local`/`typeset` loop and the `export`/`readonly` ones,
and to keep the `-n` operand itself exempt (`declare -n r=x` is about
`r`, not about `x`). Note bash's own exemption: an operand *carrying*
`-n` is not resolved, and neither is `unset -n`.

**✅ RESOLVED 2026-07-31.** Both loops now resolve the operand.

`builtin_declare` follows one immediately after the operand's
identifier validation, so every attribute, kind, case flag and value
below it sees the target. The exemptions are the declarations that are
*about* the reference: `-n` (declaring or retargeting), `+n` (taking the
attribute away) and `unset -n`, plus a subscripted operand — which bash
answers with a rule of its own, now tracked as
**TD-OILS-DECL-NAMEREF-SUBSCRIPT** below. A circular chain names nothing
to declare, so the operand is dropped after the warning with the status
left alone.

`export`/`readonly` share `attr_operand`, which now returns the new
two-variant `AttrOperand` (`Mark` — a name to mark — or `Done` — nothing
left to mark, plus the status this operand contributes) instead of an
`Option` tuple, and delegates a nameref operand to the new
`attr_nameref`. These two have no exemption at all: neither has a `-n`
operand of its own, and `export -n` removes the export attribute from the
*target*. Their two failure shapes are both quiet about the status:

* a **circular** chain reports 0 for a bare `export a` and 1 for
  `export a=5`, the failure being the store that had nowhere to go;
* an **element** reference (`declare -n r=arr[1]`) is refused with
  `` export: `arr[1]': not a valid identifier `` and reports 0 — but the
  *store* still happens, routed through the reference by
  `apply_assignment` so the append form works too (`export r+=9` leaves
  `arr[1]="29"`). This is where `declare` and these two part company:
  `declare -i r` marks `arr` and stores into `arr[1]`.

**Coverage.** New corpus case `tests/corpus/nameref-declare.sh` (which
declarations follow the reference and which are about it, a target that
does not exist yet, chains, element references through all three
builtins, circular chains, a self reference, a subscripted operand,
`readonly` naming the target when it refuses, the listings, and
`local -n`) plus two unit tests. Full differential corpus: 172 matched,
0 failed; 1032 unit tests pass; clippy clean on both targets.

The one remaining divergence is the number of `warning: a: circular name
reference` lines bash prints — two for a bare operand, three for a valued
one, against osh's one. That is the pre-existing
**TD-OILS-NAMEREF-WARNING-COUNT** below, so the corpus case deduplicates
the warning rather than counting it. *(Closed 2026-08-04; the corpus case
counts the warning as of `e5f937b7e`.)*
