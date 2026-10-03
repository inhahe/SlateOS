### TD-OILS-A-NAMEREF-BASE-IS-FOLLOWED-WHEN-THE-REFERENCE-IS-READ. `declare -n base=n; declare -n r='base[2]'` read the string `n` where bash reads element 2 — 2026-08-05 — ✅ **FIXED 2026-08-05**

**Where:** `userspace/oils/src/interp.rs` — `Shell::ref_target_value`.

**What.** The base of a nameref that designates an array *element* — the `base`
of `declare -n r='base[2]'` — is a **name**, and bash follows it through a
nameref chain of its own before the subscript applies to anything:
`array_variable_part` is `find_variable` on the base, and that chases references
like every other lookup. osh took the written base as final and subscripted
*it*, which for a nameref means subscripting a scalar whose value is the target
name. Measured with `n=(a b c d)`, `i=2`, `s=SCALAR`,
`declare -A mm=([k]=K)`:

```text
                              bash            osh (before)
declare -n base=n
  declare -n r='base[2]'      `c`             `n` — element 0 of the scalar
  declare -n r='base[$i]'     `c`             `n`
  declare -n r='base[-9]'     `n: bad array   `base: bad array subscript`
                              subscript`
declare -n base=s
  declare -n r='base[0]'      `SCALAR`        `s`
declare -n base=mm
  declare -n r='base[k]'      `K`             `mm`
declare -n base=nope          empty           `nope`
declare -n base='n[1]'        empty           `n[1]`
declare -n c1=c2; -n c2=c1
  declare -n r='c1[0]'        two `circular   no warning, reads `c2`
                              name reference`
                              warnings, empty
```

**Fixed 2026-08-05.** `ref_target_value` resolves its base through
`resolve_ref_use_walks(base, 2)` before either branch, and answers
`ElemValue::Absent` when the chain names nothing — circular, unset, or ending on
an element rather than a variable. Every later step then works on the *resolved*
name: the associative/indexed choice, the element lookup, the whole-array
enumeration, and the `bad array subscript` complaint, which is why that
diagnostic now spells the base the chain arrived at.

The walk count is **two**, and measured: the name is resolved once to find the
array and again to read out of it, so a circular base is reported twice — which
is `Shell::param_elem_lookup`'s rule for a subscripted read reached the ordinary
way, `${c1[0]}` warning twice where `${c1}` warns once. Both branches pay it,
`base[@]` included. The enumeration afterwards walks nothing further, the name
handed to it being plain.

**Corpus:** `a-nameref-base-is-followed-when-the-reference-is-read.sh`.

Three divergences found alongside this one have their own entries, and all
three have since been fixed. The *store* side deliberately does not follow the
base at all —
TD-OILS-A-NAMEREF-BASE-IS-FOLLOWED-ON-A-WRITE-WHERE-BASH-BINDS-THE-BASE-ITSELF
— while `unset` follows it as the read does, and osh did neither:
TD-OILS-UNSET-THROUGH-A-REFERENCE-TO-AN-ELEMENT-UNSETS-THE-BASE-INSTEAD. The
third is unrelated to the base:
TD-OILS-A-REFERENCE-TO-AN-ELEMENT-IS-EXEMPT-FROM-SET-U-UNBRACED.
