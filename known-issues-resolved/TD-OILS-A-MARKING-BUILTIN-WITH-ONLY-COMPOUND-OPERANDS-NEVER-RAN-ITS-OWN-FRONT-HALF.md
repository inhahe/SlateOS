### TD-OILS-A-MARKING-BUILTIN-WITH-ONLY-COMPOUND-OPERANDS-NEVER-RAN-ITS-OWN-FRONT-HALF. `readonly -i g=(1 2)` marked and succeeded where bash rejects the option and exits 2 — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`,
[`Shell::exec_declare_with_arrays_scoped`]. The builtin dispatch was

```rust
    "readonly" if has_scalar_operand => self.builtin_readonly(&argv[1..], out, redir, limit),
    "readonly" | "export" => 0,
```

so a command whose operands were *all* compound never reached
[`Shell::builtin_readonly`]/[`Shell::builtin_export`] at all. Its attribute was
applied instead by a `mark_bound_compounds` of this file's own, which started at
the operand loop and skipped the option scan the builtin would have run first.

bash's step 3 (`expand_declaration_argument`, subst.c:12653) is the *real*
builtin over the whole word list with the compound operands truncated to bare
names, so the front half always runs — and a scalar operand, which never enters
the decomposition at all (it is reached only for `W_COMPASSIGN` words,
subst.c:12815), arrives at that same call whole. One call, one getopt scan.

```sh
$ readonly -i g=(1 2); echo rc=$?; declare -p g
bash: readonly: -i: invalid option
      readonly: usage: readonly [-aAf] [name[=value] ...] or readonly -p
      rc=2
      declare -ai g=([0]="1" [1]="2")
osh : rc=0
      declare -air g=([0]="1" [1]="2")
```

| command | bash 5.2.37 | osh, before |
|---|---|---|
| `readonly -i g=(1 2)` | usage error, rc 2, `declare -ai g` | rc 0, `declare -air g` |
| `readonly -x g=(1 2)` | usage error, rc 2, `declare -a g` | rc 0, `declare -ar g` |
| `export -i g=(1 2)` | usage error, rc 2, `declare -ai g` | rc 0, `declare -aix g` |
| `readonly -f g=(1 2)` | `readonly: g: not a function`, rc 1, `declare -a g` | rc 0, `declare -ar g` |
| `export -f g=(1 2)` | `export: g: not a function`, rc 1, `declare -a g` | rc 0, `declare -ax g` |
| `readonly -i g=(1 2) s=5` | usage error, rc 2, `declare -ai g` | usage error, rc 2, but `declare -air g` |

The last row was the tell: a scalar operand beside the compound one *did* reach
the builtin, so the diagnostic and the status were right — and the `-r` still
leaked onto the compound, because the private loop ran regardless. Note also
that step 1's rebuilt option string carries only the array kind, the scope and
the value-transforming letters, which is why `readonly -x g=(1 2)` leaves
neither `-x` nor `-r`: the `-x` is not carried and the `-r` never runs. `-i`
*is* carried, which is why that row's array is `-ai` even though the command
failed.

**Fixed** in this commit. `mark_bound_compounds` is gone and the two builtins
are called unconditionally over [`DeclWords::spliced`] — `argv` with every
compound operand's bare name back at the position it was written, which is
exactly bash's step-3 argument list. Nothing can now end option parsing at a
word the builtin cannot see, so the `flag_limit` dance is not needed on this
path either and the natural getopt stop is the only limit; that is also what
gives `readonly +a q=(1)` the same `` `+a': not a valid identifier `` the scalar
spelling gives. The call stays where phase 3 was — after
`std::mem::take(&mut self.declare_global_swap)` and
[`Shell::leave_global_scope`], inside the builtin's own `2>` — so the
value/attribute split the previous fix established is untouched.

The merge deleted a duplicate rather than adding a caller: the private loop was
[`Shell::attr_operand`] + [`Shell::in_scope`] +
[`Shell::readonly_operand`]/[`Shell::export_operand`], which is the builtin's
own operand loop verbatim. With it went `DeclCompounds::flags` and the marking
letters of [`Shell::declare_compounds_scoped`]'s flag prescan (`r`, `x`, `t`
and the `+` spellings): the builtin half re-scans the same words, so reading
them twice only invited the two scans to disagree. `-n` is still read there,
and still cancelled for `readonly`/`export`, because a reference and an array
are a combination phase 1 must refuse *before* the literal binds.

Corpus:
`a-marking-builtin-with-only-compound-operands-still-runs-its-own-flag-scan.sh`.
Unit test:
`a_marking_builtin_with_only_compound_operands_still_runs_its_own_flag_scan`.

**How it was found:** checking, after the value/attribute fix, that the shapes
the fix did *not* touch still agreed — a sweep over `readonly`/`export`
× every option letter × compound-only and mixed operand lists.
