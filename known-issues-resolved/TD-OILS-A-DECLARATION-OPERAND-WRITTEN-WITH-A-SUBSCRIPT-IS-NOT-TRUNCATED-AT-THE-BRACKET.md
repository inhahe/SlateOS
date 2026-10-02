### TD-OILS-A-DECLARATION-OPERAND-WRITTEN-WITH-A-SUBSCRIPT-IS-NOT-TRUNCATED-AT-THE-BRACKET. `declare 'r[1]'` through a reference is quiet in bash and refused in osh — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare_scoped`, the
operand-name refusal that produces `` `NAME': not a valid identifier ``.

**What.** bash truncates a declaration operand at its first `[` before doing
anything with it. So `declare 'r[1]'` operates on `r` — and when `r` is a
nameref, on whatever `r` points at — never on the spelling `r[1]`. With **no**
flags this means the command quietly does nothing:

```text
$ n=(a b c); declare -n r='n[@]'; declare 'r[1]'
bash: s=0                     # and `declare -p r` still shows `declare -a r`
osh : declare: `n[@][1]': not a valid identifier    s=1

$ n=(a b c); declare -n r='n[1]'; declare 'r[1]'
bash: s=0
osh : declare: `n[1][0]': not a valid identifier    s=1
```

Once *any* flag is present the two agree — bash then does emit the
`n[@][1]` / `n[1][0]` refusal, because the flagged path reaches for the
reference's base and the doubled subscript is genuinely unusable there:

```text
$ n=(a b c); declare -n r='n[@]'; declare -i 'r[1]'
both: declare: `n[@][1]': not a valid identifier    s=1
```

Valued operands (`declare 'r[1]'=v`) agree too — both sides print
`warning: r: removing nameref attribute` and succeed.

**Why.** `declare.def:566–580` walks the operand for `[`, writes `*t = '\0'` in
place and sets `making_array_special`; the truncated name is what the rest of the
builtin sees. The flagless case then falls into the `declare.def:733` *bind*
path, which has nothing to bind (no value, no flags) and returns success without
touching anything. osh instead concatenates the reference's spelling with the
operand's subscript and refuses the result unconditionally.

**Fixed.** `builtin_declare_scoped` now gates the doubled-subscript refusal on
the same "does this command ask for anything?" predicate the bad-subscript line
already used (`value.is_none() && !make_local && !print_mode && !global &&
!nameref && !unset_nameref && !other_attrs`). Where it does not fire, the
operand is modelled on bash's plain `find_variable` of the *truncated* name,
which follows a reference only as far as a variable that already exists: a
target carrying a subscript of its own, or one nothing has bound, answers
nothing, so `target` is dropped and `unreference_for_declare` makes the array of
the operand's own name — exactly as the circular-chain arm just above already
did. `-G` is `-g`'s twin and asks for a binding, so it goes to the built name;
`-I` asks nothing and stays. Two comments that had lumped `-G` in with `-I` were
corrected at the same time.

The neighbouring half — the `=` that splits `NAME=value` is the one *outside* the
subscript, and an empty subscript is refused as an identifier on the valueless
path — landed with it. Corpus case:
`a-declaration-operand-is-truncated-at-its-bracket-before-anything-looks-at-it.sh`.
Probes: `/d/tmp/hh/bx.sh` (N-series), `/d/tmp/hh/bz.sh`, `/d/tmp/hh/da.sh`, and
`/d/tmp/hh/ca.sh`–`cf.sh` (A/B/C-, F-, G-, H-, J-, K-series), which cover the
model end to end: which name the array is made of, that the subscript is never
evaluated, the `=`-outside-the-subscript split, the empty-subscript refusal, and
which letters put the operand back on the built-name path.
