### TD-OILS-ASSIGN-PREFIX-BYPASSES-DYNVAR. An assignment *prefix* naming a computed variable is ignored by the command — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — the temporary-environment path
that binds a simple command's assignment prefix. It writes the binding
into `vars` directly, and a computed name's read never consults `vars`,
so the name went on computing and the binding was simply invisible for
the command's duration.

```sh
SECONDS=100 eval 'echo "[$SECONDS]"'       # bash [100]   osh [0]
BASH_SUBSHELL=9 eval 'echo "[$BASH_SUBSHELL]"'  # bash [9]   osh [1]
echo "[$SECONDS]"                          # both [0] afterwards
```

The *restore* half is already right — bash puts the old value back when
the command ends, and so does osh — so this is exactly the store.

**Fixed** — and the original "proper fix" note here (run the assign
function and snapshot the counter bases around the command) was **wrong**,
which measuring settled. bash does not run the assign function for a
prefix at all: it binds the name in a **variable scope of its own**, so
for the command's duration the name is an *ordinary* variable. Decisive
evidence: `LINENO=9 eval 'echo $LINENO'` prints `9`, not the live line;
`RANDOM=5 eval 'echo $RANDOM $RANDOM'` prints `5 5`, not two draws; and
`declare -i SECONDS; SECONDS=3+4 eval 'echo $SECONDS'` prints `3+4` — the
string as written, neither parsed as a number nor evaluated, so no assign
function can have seen it.

That is the same rule `local` follows (see TD-OILS-DECL-VALUE-BYPASSES-
DYNVAR), applied to a command rather than a frame, so the fix is the same
guard: `Shell::temp_shadow` records the computed names a prefix binds,
`Shell::push_temp_shadow`/`pop_temp_shadow` bracket the builtin
(`run_builtin_body`) and the function call (`call_function`), and
`Shell::is_ordinary_shadowed` — now asked by both
`dynamic_special_value` and `apply_assignment` — answers `local`-or-prefix
in one place. `clone_for_subshell` carries the stack, because the binding
itself is in `vars`, which a subshell copies.

Covered by the unit test `an_assignment_prefix_shadows_a_computed_name`
and by `tests/corpus/bash-assign-prefix-dynvar.sh`.

**All four follow-ups are since fixed:**
TD-OILS-UNSET-THROUGH-TEMP-SHADOW, TD-OILS-UNSET-LOCAL-KILLS-DYNVAR,
TD-OILS-TEMP-BINDING-ATTRIBUTES and TD-OILS-LOCAL-INHERITS-TEMP-BINDING.
