### TD-OILS7. `osh` `readonly`: enforcement across assignment/`unset`/`declare`, the `read` builtin, and temporary env prefixes — RESOLVED

**Where:** `userspace/oils/src/interp.rs` (`apply_assignment`,
`builtin_unset`, `builtin_declare`/`builtin_readonly`, `builtin_read`, and
the env-prefix guard in `exec_simple`; shared `set_scalar_checked` helper).

**What:** `readonly name[=val]` / `declare -r` mark a variable read-only,
and every write path now enforces it with status 1 and no mutation:
reassigning it (`x=2`), unsetting it (`unset x`), re-declaring a value
(`declare x=…`), reading into it (`read x`, field `read a b`, or
`read -a arr`), and temporarily overriding it as a command prefix
(`readonly x; x=1 cmd`) — all emit `osh: NAME: readonly variable` and leave
the value intact. For a multi-name `read a b`, earlier fields are assigned
before the readonly field aborts (bash field-order semantics). The env
prefix is guarded *before* dispatch in `exec_simple`, so no
function/builtin/external path can mutate a readonly variable.

**Resolution:** added a shared `set_scalar_checked(name, val) -> bool`
helper (resolves namerefs, honors `allexport`/case-fold attrs, consults
`self.readonly`, emits the diagnostic, returns false on rejection) and
routed `builtin_read`'s scalar-assignment sites through it (a rejected
target aborts the read with status 1); the `read -a` array path takes an
inline readonly check; and `exec_simple` guards the temporary env-prefix
names against `self.readonly` before dispatch. Tests:
`read_into_readonly_var_fails`, `read_field_readonly_aborts_after_earlier_fields`,
`read_array_readonly_fails`, `env_prefix_readonly_var_errors`.
