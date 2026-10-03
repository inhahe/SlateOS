### TD-OILS-LOCAL-NOASSIGN-OUTRANKS-READONLY. A `local` of a readonly call-stack array reports the wrong refusal — 2026-08-04 — ✅ **RESOLVED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — the `self.noassign.contains(base_name)
&& (value.is_some() || make_local)` guard in the `builtin_declare_scoped` operand
loop (~30055), which runs before the readonly refusal.

**What:** when a name is both un-assignable (`NOASSIGN_VARS` — the call-stack
arrays, `GROUPS`, …) and `readonly`, bash reports the *readonly* refusal and osh
reports the un-assignable one. Both give rc 1 and neither binds, so only the
message differs. Measured against bash 5.2.37:

```sh
declare -r BASH_SOURCE; f() { local BASH_SOURCE; }; f
# bash: local: BASH_SOURCE: readonly variable
# osh : local: BASH_SOURCE: variable may not be assigned value
declare -r FUNCNAME;    f() { local FUNCNAME; }; f      # same divergence
declare -r SECONDS;     f() { local SECONDS; }; f       # both: readonly variable ✅
declare -r ord=1;       f() { local ord; }; f           # both: readonly variable ✅
```

So it is specific to the names in `noassign`; an ordinary readonly and a readonly
*scalar* dynamic special already match. Without the readonly the un-assignable
message is the right one, so the guard cannot simply move — it has to yield to a
readonly that is also present.

**Fixed in `d222be4ed`.** The `shadow_new && readonly_blocks` refusal already
existed a few checks further down — it is what makes even a *valueless* `declare
ro` inside a function report — so the fix was to move it *above* the `noassign`
guard rather than to add a condition to the latter. That keeps the un-assignable
refusal for a name that is only in `noassign`, which is what the divergence
required: it is an ordering between two refusals, not one swallowing the other.

`readonly_blocks` is already the "a readonly is in the way" question the rest of
the loop asks, so nothing new had to be kept in sync. A `-g` asks for no shadow,
so `shadow_new` is false and it never reaches the ordering: the value-carrying
readonly refusal one check earlier still speaks for it, which is why
`declare -r GROUPS; f() { declare -g GROUPS=5; }` says `readonly variable` in
both shells.

Covered by a new `noassign-vars.sh` corpus section running the whole of
`NOASSIGN_VARS` through the `local`/`declare`/`-g`/valueless/valued shapes and
pinning the two already-matching baselines (a readonly *scalar* dynamic special
and an ordinary readonly), plus assertions in the lib test
`a_declaration_builtin_refuses_the_variables_the_shell_maintains`.
