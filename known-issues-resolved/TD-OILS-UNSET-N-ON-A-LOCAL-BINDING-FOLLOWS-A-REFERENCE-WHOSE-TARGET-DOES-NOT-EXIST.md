### TD-OILS-UNSET-N-ON-A-LOCAL-BINDING-FOLLOWS-A-REFERENCE-WHOSE-TARGET-DOES-NOT-EXIST. `f() { declare -n r=nosuch; declare +n r; }` declares `nosuch` and leaves `r` a reference — 2026-08-05 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_declare_scoped`, the
`follow` rule (`(!unset_nameref || !make_local)`) and the `+n` early exit just
below `nameref_off_name`.

**What.** osh's model is that `+n` follows a reference on the same terms as any
other letter *except* when the declaration binds a local, where it never does —
it is then about the reference's own binding. That is right for a target that
exists, and wrong for one that does not. Measured inside a function, in every
case with the reference local to the frame:

```text
                                          bash                      osh
w=5;      declare -n r=w;      declare +n r     `r` becomes `"w"`,   the same — agreed
                                                `w` untouched
          declare -n r=nosuch; declare +n r     `declare -- nosuch`, `r` becomes
                                                `r` stays `-n`       `"nosuch"`
          declare -n r=nosuch; declare -x +n r  `declare -x nosuch`, `r` becomes
                                                `r` stays `-n`       `declare -x r`
declare nosuch; declare -n r=nosuch; declare +n r
                                                `r` becomes          the same — agreed
                                                `"nosuch"`
n=(a b c); declare -n r='n[1]'; declare +n r    `declare -- n[1]`,   `r` becomes
                                                `r` stays `-n`       `"n[1]"`
n=(a b c); declare -n r='n[1]'; declare -x +n r `declare -x n[1]`,   `declare -x r`
                                                `r` stays `-n`
n=(a b c); declare -n r='n[1]'; declare +n r=zz `declare -- n[1]="zz"`, `r` becomes
                                                `r` stays `-n`       `"zz"`
n=(a b c); declare -n r='n[1]'; declare r=zz; declare +n r
                                                `r` becomes `"n[1]"`, the same — agreed
                                                the local keeps `zz`
```

So the rule is *`+n` on a local-binding declaration follows the reference iff the
name it leads to does not already exist*, and "exists" is bash's `find_variable`
— a name brought into being unvalued (`declare nosuch`) counts, and so does the
spelling-named local a previous operand made (last row). Where it follows, the
whole declaration is about the target and `+n` is a no-op there (the target
carries no nameref attribute), so the operand's own reference survives intact.
At top level `+n` never follows, which osh already gets right.

An element reference reaches this every time, since a subscripted spelling can
only exist as a name after
TD-OILS-A-DECLARATION-BUILTIN-THAT-BINDS-A-LOCAL-THROUGH-A-REFERENCE-TO-AN-ELEMENT-MAKES-A-LOCAL-NAMED-BY-THE-SPELLING
has made one.

**Proper fix.** Peek at the target with the silent `Shell::resolve_ref_name`
before computing `follow`, and let `+n` follow when the resolved target's
spelling is not bound (`Shell::name_is_bound`). The `+n` early exit then has to
step aside for that case — the attribute comes off the *target*, which never has
it, so `r` keeps its own — and `nameref_off_name` has to name the target rather
than the operand.

**Impact.** Two names come out wrong: bash declares the target, osh de-references
the operand. A function that uses `+n` to test whether a reference resolves sees
the opposite answer, and any attribute in the same command lands on the wrong
variable.

**Fixed 2026-08-05.** `Shell::builtin_declare_scoped` peeks at the target with
the silent `Shell::resolve_ref_name` — silent so the peek cannot add a warning to
the walk the declaration is about to do — and lets a `+n` local binding follow
when `Shell::name_is_bound` says the resolved spelling names nothing.
`nameref_off_name` became an `Option`, `None` in exactly that case: the letter is
then about a target that cannot be carrying the attribute, so nothing comes off
and the operand's own reference survives. The `+n`-only early exit steps aside
for it, since such an operand still has a target to bring into being. An element
reference reaches this every time and lands in the spelling-named binding the
entry above supplies, so `declare -x +n r` through `declare -n r='n[1]'` leaves
`declare -x n[1]`. Corpus:
`plus-n-on-a-local-binding-follows-the-reference-when-its-target-does-not-exist.sh`.
