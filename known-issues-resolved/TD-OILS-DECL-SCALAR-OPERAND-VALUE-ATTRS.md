### TD-OILS-DECL-SCALAR-OPERAND-VALUE-ATTRS. A scalar operand of an array declaration skipped the value attributes — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare`'s `assoc || indexed`
value branch (~19550), and the new `appended_attributed_value` (~6800).

**Was.** `declare -a name=word` binds `word` at index/key 0, and bash applies the
name's value attributes to it exactly as it would to a scalar's value. osh stored
the raw word, so `-i`/`-l`/`-u`/`-c` were silently skipped whenever the array kind
was given as well — and the `+=` form appended as a string even under `-i`:

```sh
declare -al a=QQ;                    declare -p a   # bash [0]="qq"   osh [0]="QQ"
declare -ac a=hELLO;                 declare -p a   # bash "Hello"    osh "hELLO"
declare -ai a=2+3;                   declare -p a   # bash "5"        osh "2+3"
declare -ai a=5; declare -ai a+=3;   declare -p a   # bash "8"        osh "53"
```

Three smaller shapes came out of the same measurement:

```sh
declare -ai bad=2+ ok=1; declare -p bad; declare -p ok
# bash: declare -ai bad=()   +  ok: not found       osh: bad=([0]="2+"), ok bound
declare -i  bad=2+ ok=1; declare -p bad; declare -p ok
# bash: declare -i bad       +  ok: not found       osh: bad absent,     ok bound
declare -ai b[0]=2+ ok=1; declare -p b;  declare -p ok
# bash: declare -ai b=()     +  ok: not found       osh: b unvalued,     ok bound
```

i.e. a bad `-i` value leaves the name *created* (an array valued-but-empty, a
scalar merely declared-but-unset) and abandons every operand after it — bash's
arithmetic error jumps straight out of the builtin. osh bound the survivors.

**Fix.** `appended_attributed_value` = `apply_value_attrs` with `+=` resolved
against the slot's current contents, which is where the two attribute groups differ
(`-i` adds numerically, the case attributes fold-then-concatenate). All three of
`builtin_declare`'s value stores — array element 0, integer scalar, folded scalar —
now go through it, replacing two hand-rolled append paths; each failure site marks
the name created and `break`s out of the operand loop. Covered by
`a_scalar_operand_of_an_array_declaration_gets_the_value_attributes` and the `s*` /
`bad*` blocks of `tests/corpus/declare-attrs.sh`.
