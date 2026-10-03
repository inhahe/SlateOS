### TD-OILS-DIRSTACK-WRITEBACK. `DIRSTACK` was a snapshot, not a view — writes to it did not reach the directory stack — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `Shell::dirstack_dynamic`,
`refresh_dirstack`, `sync_dirstack_writeback`, `after_var_write`,
`is_locally_shadowed`; hooked from `apply_assignment`, `unset_element`,
`builtin_read`'s `-a` branch, `builtin_mapfile`, `builtin_printf`'s `-v`
branch, the arithmetic `set`/`set_index`, `unbind_var` and the local-frame
pop.

**What:** found while measuring `DIRSTACK` for TD-OILS-NOASSIGN-VARS —
it is one of the two names that *look* like they carry `att_noassign` and
do not. What they carry instead is bash's dynamic-variable pair: a **value**
function that re-materialises the array on every read, and an **assign**
function, called once per element, that pushes the write back into the live
stack. osh had only the first half, and even that as an eager snapshot
refreshed on `cd`/`pushd`/`popd`, so a write simply stuck. Measured against
bash 5.2.37 (MSYS), with a two-deep stack:

| form | bash | osh before |
|---|---|---|
| `DIRSTACK[1]=/` | writes through: `dirs +1` prints `/` and a later `popd` goes there | stored in the array only; `dirs`/`popd` unaffected |
| `DIRSTACK[0]=/nowhere` | ignored — element 0 is the cwd, which the stack does not own — and never moves the shell | stored |
| `DIRSTACK[5]=q`, `DIRSTACK+=(zz)` | past the end, so dropped; the array stays two long | grew the array |
| `DIRSTACK=(a b c)` | elementwise, not a replacement: `a` ignored, `b` written, `c` dropped, array still two long | replaced it with three elements |
| `unset 'DIRSTACK[1]'` | no-op — the value function puts it straight back | left a gap |
| `unset DIRSTACK` | drops the hooks with the variable and bash never restores them: `pushd` stops touching the name and `DIRSTACK=(a b c)` really is a three-element array | re-materialised on the next `pushd` |
| `local DIRSTACK=(a b)` | an ordinary local for the length of the call; a `cd` in the body does not rewrite it | the refresh clobbered the local |

Every one of these succeeds and none of them says anything, so the status
is no help — the only sign is what `dirs` prints afterwards.

**Fixed 2026-07-31:** `sync_dirstack_writeback` copies the array's
elements `1..=dir_stack.len()` back into `Shell::dir_stack` (index 0 and
anything past the end having nowhere to go, and a *missing* index meaning
"unchanged", which is what makes `unset 'DIRSTACK[i]'` a no-op) and then
re-materialises. `after_var_write` is the one hook every user-facing write
path runs afterwards; `apply_assignment` became a wrapper around
`apply_assignment_inner` so it fires on every exit. `dirstack_dynamic`
(cloned into subshells) goes false in `unbind_var`, and
`is_locally_shadowed` suspends both halves while a `local` of the name is
in the way — with a `refresh_dirstack` on the frame pop so the global
catches up with a `cd` made under the shadow, which bash gets for free by
regenerating on read. Corpus case `dirstack-var.sh`; unit test
`dirstack_writes_back_into_the_directory_stack`.

`COMP_WORDBREAKS`, the other name in that shape, really is an ordinary
variable in bash and needs nothing.
