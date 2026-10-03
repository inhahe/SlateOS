### TD-OILS-A-REFERENCES-SUBSCRIPT-IS-NEVER-EVALUATED-AS-ARITHMETIC. `declare -n r='n[1+]'` should be an arithmetic syntax error, not "not a valid identifier" — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare_scoped` and
`declare_compounds_scoped`, the two places that inspect a `RefTarget`'s `sub`.

**What.** bash *evaluates* a nameref target's subscript arithmetically the
moment a declaration builtin follows the reference, and reports the evaluation's
own error. osh never evaluates it: it either refuses the spelling as a name or
says nothing at all.

```text
$ n=(a b c); declare -n r='n[1+]'; declare r=(x y)
bash: 1+: syntax error: operand expected (error token is "+")
osh : `n[1+]': not a valid identifier

$ n=(a b c); declare -n r='n[1+]'; declare r        # valueless
bash: 1+: syntax error: operand expected (error token is "+")   s=1
osh : (silent)                                                  s=0

$ n=(a b c); declare -n r='n[)]'; declare r=(x y)
bash: ): syntax error: operand expected (error token is ")")
osh : `n[)]': not a valid identifier
```

The valued (`declare r=v`), expansion (`echo "$r"`) and plain-assignment
(`r=(x y)`) forms already agree — osh reaches the arithmetic evaluator on those
routes. Only the two *declaration* routes skip it. `n[ ]` agrees on both sides
(`n[ ]': not a valid identifier`), because an empty subscript is refused before
arithmetic is attempted in bash too.

**Why.** This is the same gate as
`TD-OILS-A-WHOLE-ARRAY-REFERENCE-…-IS-MISSING-ITS-BAD-SUBSCRIPT-LINE`, one
branch further along. `declare.def:733` decides between "reach for the base" and
"bind the operand"; the *bind* path runs `bind_variable(name, NULL, ASS_FORCE)`,
which follows the reference into `array_value_internal`, and that function
evaluates the subscript with `evalexp` before it can decide whether the element
exists. A malformed expression therefore raises the arithmetic error from inside
the read — untagged, from the read rather than from the builtin, exactly as
`err_badarraysub` does for `n[@]`.

**Fixed** by `Shell::declare_ref_bind_read`, which is the element store's own
three answers in its own order — whole-array token, empty associative key,
unevaluable or too-negative index — called from the two sites that already
decided whether the command asks for anything: the `RefTarget { sub: Some(_) }`
arm of `declare_compounds_scoped` and the flagless-valueless branch of
`builtin_declare_scoped`. The arithmetic error returns `Err(Flow::Discard)` with
status 1, which pre-empts the `not a valid identifier` refusal exactly as bash's
`jump_to_top_level` does; a `bad array subscript` only complains and the refusal
still follows it. On a subscript that *does* evaluate, the valueless branch falls
through to the ordinary path as before rather than swallowing the operand —
`declare -n q='nope[1]'; declare q` still has to leave `declare -a nope` behind.

The gate is the same predicate the whole-array line already used, because it *is*
that line: bash reaches for a reference's base only when the command asks for
something, and with nothing asked it falls through to `bind_variable`, which
lands in `assign_array_element` on the target's spelling
(`variables.c:bind_variable_internal`, the `valid_array_reference (newval)` arm).
The two halves disagree about which letters count, because the
compound-assignment machinery rebuilds a command's flags rather than passing them
on — so `declare -r r=(x y)` evaluates and `declare -r r` does not.

Corpus: `a-declaration-with-nothing-to-do-evaluates-the-subscript-the-reference-carries.sh`.

What that bind then *stores* is still not modelled — see
`TD-OILS-A-DECLARATION-WITH-NOTHING-TO-DO-BINDS-A-NULL-THROUGH-THE-REFERENCE`
below.
