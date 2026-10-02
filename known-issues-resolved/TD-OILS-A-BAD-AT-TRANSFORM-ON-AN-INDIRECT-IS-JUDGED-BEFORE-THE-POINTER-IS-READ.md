### TD-OILS-A-BAD-AT-TRANSFORM-ON-AN-INDIRECT-IS-JUDGED-BEFORE-THE-POINTER-IS-READ. `${!u[k]@Z}` is a bad substitution where bash reads the pointer and answers empty — 2026-08-10 — ✅ FIXED 2026-08-10

**Where:** `userspace/oils/src/parser.rs`, the list of modifier shapes that
`${!ref<op>}` accepts as an `IndirectOp` target (`parse_braced_param_in`, the
`matches!(target, WordPart::ParamOp { .. } | … | WordPart::ParamTransform { .. })`
guard). `WordPart::BadTransform` is missing from it, so a `${!ref@BAD}` becomes a
`WordPart::BadSubst` of the whole body and never reaches the indirection at all.

**Reproduce.**

```sh
sv=SVAL
declare -A s=([k]=sv) u=([k]=nosuch) mt=([k]='')
b() { printf '%-14s ' "$1"; ( eval "printf '<%s>' $1"; printf ' st=%s' "$?" ) 2>&1; echo " alive=$?"; }
b '${!u[k]@Z}'     # bash: <> st=0 alive=0        osh: bad substitution
b '${!mt[k]@Z}'    # bash: ": invalid variable name"        osh: bad substitution
b '${!nope[k]@Z}'  # bash: "nope[k]: invalid indirect expansion"  osh: bad substitution
b '${!s[k]@Z}'     # both: "${!s[k]@Z}: bad substitution" — but see below
```

…and the pointer's own subscript never runs:

```sh
o() { printf '%s ' "$1" >&2; }
f() { o f; echo k; }
printf '<%s>' ${!s[$(f)]@$(o T; echo Q)}
# bash: "f f " then the complaint — the pointer is read twice and the
#       transform's operand is never expanded at all
# osh : nothing on stderr but the complaint
```

Finally, the one spelling whose *message* already agrees still differs in
weight: bash's transform complaint is **fatal** (`expand_param_fatal`, so a
non-interactive shell exits), while osh's `BadSubst` is DISCARD-class and the
script carries on:

```sh
echo A; printf '<%s>' ${!s[k]@Z}; echo " st=$?"; echo B
# bash: A, the complaint, exit 1        osh: A, the complaint, " st=1", B, exit 0
```

**Why bash does this.** The `@` operator's validity is judged inside
`parameter_brace_transform` (subst.c), which is reached only *after*
`parameter_brace_expand_indir` has resolved the pointer and
`get_var_and_type` has re-read it — and it is skipped entirely when the
referenced variable is unset, which is why `${!u[k]@Z}` and the plain
`${nope@Z}` are both simply empty. osh reproduces that split for the plain
spelling (`WordPart::BadTransform`'s arm in `Shell::expand_dynamic_with`) and
raises it fatally there (`Shell::bad_transform_substitution`); it is only the
indirect spelling that never gets to it.

**A second half, on the plain path too.** The short-circuit that spares an unset
parameter exempts exactly two letters:

```c
xc = xform[0];
if (value == 0 && xc != 'A' && xc != 'a')
  return ((char *)NULL);
```

so `${nope@Za}` is quietly empty while `${nope@aQ}` and `${nope@AQ}` are refused
— the second pair asks about the *variable*, which an unset name still answers,
and only *then* is the spelling judged and rejected. osh answered `<>` for all
of them, on the scalar path and on the `[@]`/`[*]` bulk path alike
(`${ea[@]@aQ}` through a declared-empty array is fatal in bash).

**Proper fix.** Add `WordPart::BadTransform { .. }` to the accepted-target list
so `${!ref@BAD}` parses as an `IndirectOp`, and let the existing runtime do the
rest — `Shell::indirect_op_reresolves` already has a `BadTransform` arm, and
`WordPart::BadTransform`'s expansion arm already makes the set/unset split and
the fatality. Two things need care: `rename_param_target` must rewrite a
`BadTransform`'s name like every other target, and the diagnostic must name the
*reference* (`${!s[k]@Z}`), not the target it resolved to (`${sv@Z}`) — the
existing `Shell::ref_label` mechanism is what the other indirect complaints use.
Then lift the `a`/`A` test `indirect_op_reresolves` already carries into a shared
`bad_transform_asks_about_the_variable` and let both expansion sites consult it.

**Fixed** by exactly that: `WordPart::BadTransform` joins the `IndirectOp`
target list in `parse_braced_param_in` and `WordPart::set_param_name`; the two
`BadTransform` expansion arms (`Shell::expand_dynamic_with` and
`Shell::bulk_elements`) now prefer `Shell::ref_label` for the spelling they quote
and consult `bad_transform_asks_about_the_variable` for the exemption, which
`Shell::indirect_op_reresolves` now shares. Regression cases:
`userspace/oils/tests/corpus/a-bad-at-transform-is-judged-after-the-indirect-has-read-its-pointer.sh`
and the unit test of the same name.
