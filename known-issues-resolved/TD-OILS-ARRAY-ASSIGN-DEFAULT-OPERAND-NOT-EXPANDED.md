### TD-OILS-ARRAY-ASSIGN-DEFAULT-OPERAND-NOT-EXPANDED. `${a[@]:=w}` complains without expanding `w` — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::array_op_fields`, the
`ParamOp::AssignDefault` arm, which reports `a[@]: bad array subscript` before
looking at the operand.

**Reproduce:**

```sh
a=()
echo "[${a[@]:=$(echo ran >&2; echo v)}]"   # bash: prints `ran` on stderr, then
                                            #   `a[@]: bad array subscript`
                                            # osh: only the complaint
```

**The rule.** bash expands the default word *first* and complains afterwards, so
a command substitution in it runs even though the assignment can never happen —
the same order `${!ref:=w}` follows (see the "the default word is expanded
first" comment in `Shell::expand_param_op`). The positional spelling `${@:=w}`
is the opposite and is already correct: bash refuses `$@: cannot assign in this
way` *without* expanding, which is why the two are separate branches.

**The fix.** One line in the `AssignDefault` arm of `array_op_fields`: expand
the operand and drop the value, immediately before the bad-subscript
diagnostic. The positional branch and the associative branch already sat ahead
of it — the first refusing without expanding, the second expanding and really
assigning — so neither moved.

**Pinned by** `userspace/oils/tests/corpus/an-assign-default-expands-before-it-refuses.sh`,
which also pins the three things beside it that were already right: the active
array reads nothing, the positional refusal comes with status 1 where the
subscript complaint comes with 2, and an associative array assigns a key spelled
`@` or `*`.
