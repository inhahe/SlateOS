### TD-OILS-MISSING-SPECIAL-ARRAYS. `osh` does not define some bash special array variables (`GROUPS`) — 2026-07-19 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` variable seeding
(`seed_shell_vars`) and the dynamic-array materialisers (cf.
`refresh_funcname` for FUNCNAME/BASH_SOURCE, `refresh_dirstack` for
DIRSTACK, `refresh_bash_arg_arrays` for BASH_ARGC/BASH_ARGV).

**What:** bash predefines several dynamic array variables that osh does
not. Confirmed missing (`${VAR+set}` empty in osh, `set` in bash):
`GROUPS` (the invoking user's supplementary group IDs). DIRSTACK is now
implemented (see `refresh_dirstack`); FUNCNAME, BASH_SOURCE, BASH_LINENO
already were; **`BASH_ARGC`/`BASH_ARGV` are now implemented — RESOLVED
2026-07-20** (see below).

**Why deferred (at the time):** `GROUPS` needs a host notion of Unix
supplementary groups, which the Windows host build and the current
slateos user model do not expose.

**GROUPS — RESOLVED 2026-07-31.** Seeded in `seed_shell_vars` as an
indexed array (bash's `declare -a`: not readonly, not integer, not
exported) from the new `reported_groups`, which continues the identity
story `reported_identity` (`UID`/`EUID`) began. Three sources, in order:

1. **`OSH_GROUPS`** in the environment — numeric IDs separated by
   whitespace and/or commas, order kept, repeats dropped (`getgroups`
   never reports one twice), non-numeric entries ignored
   (`parse_group_list`). This is the counterpart of the `OSH_UID` /
   `OSH_EUID` overrides: a login session that already knows the user's
   identity hands it over whole.
2. **The host user database** — `passwd_record_for_uid` finds the
   `/etc/passwd` record carrying the reported UID and takes the user's
   name and *primary* GID from it; `group_ids_for_member` then adds the
   GID of every `/etc/group` record listing that name as a member, in
   file order. That is exactly what `initgroups(3)` builds, which is why
   the primary group is not looked for in the group file (it is usually
   absent from the member lists there).
3. **Group 0**, which goes with the default root `UID`/`EUID` report.
   This is the answer on the Windows host the differential corpus runs
   on, which has no user database at all.

An **inherited environment `GROUPS`** suppresses the seed entirely, so it
is imported as an ordinary exported scalar and no dynamic array is
created — bash's precedence, measured with `env GROUPS=x bash -c
'declare -p GROUPS'` (you cannot see it from bash itself, because a
`GROUPS=x` assignment prefix in bash is a silent no-op).

Tests: `groups_come_from_the_passwd_and_group_files` (both readers, over
temp databases: first-match-wins, unparsable UID/GID fields, comment and
short lines, member-name substrings, missing file),
`the_group_override_list_takes_numbers_in_order_without_repeats`, and
`groups_is_a_bound_non_empty_array_of_ids` for the seeded contract
(bound, non-empty, all-numeric, `declare -a`, `unset`-able). No corpus
case: the *values* are host identity and differ from the reference shell
by construction (MSYS bash reports its Windows RID).

**Still open — assignment semantics.** bash marks `GROUPS` (and
`FUNCNAME`, `BASH_SOURCE`, `BASH_LINENO`, `BASH_ARGC`, `BASH_ARGV`)
`att_noassign`; osh lets all six be clobbered. Tracked separately as
TD-OILS-NOASSIGN-VARS.

**BASH_ARGC / BASH_ARGV — RESOLVED 2026-07-20.** Implemented as the
extended-debugging call-argument stack (`bash_argc: Vec<usize>`,
`bash_argv: Vec<String>`, `arg_frame_pushed: Vec<bool>` on `Shell`;
helpers `extdebug_on`, `push_arg_frame`, `pop_arg_frame`,
`refresh_bash_arg_arrays`). Semantics matched against bash: enabling
`shopt -s extdebug` captures a base frame from the *current* positional
params (count → BASH_ARGC front, params reversed → BASH_ARGV front); each
function call made while extdebug is on pushes its arg count/values on
top; a call's return pops **only** the frame it actually pushed (recorded
per-frame in `arg_frame_pushed`, so toggling extdebug mid-call cannot
desync the stacks); disabling extdebug does not clear the stack; the base
snapshot is static (a later `set --` does not change it); subshells
inherit (clone) the parent stack but push no frame of their own. All of
these cases were verified equal to MSYS bash, plus enabling extdebug
*inside* a function (base = that function's positional). Regression test:
`bash_argc_argv_extdebug_stack`.

*Deliberate divergence (documented, not a bug):* **without** `extdebug`,
bash still exposes an undocumented top-level base frame — e.g.
`echo ${BASH_ARGC[@]:-U}` at a `-c` top level prints `0` in bash but `U`
(unset) in osh. bash builds this via a lazy-materialisation artifact whose
observable behaviour is self-contradictory (referencing `BASH_ARGV` before
a `set --` freezes it at the *pre-set* value; not referencing it first
yields the *post-set* value; and a non-extdebug function call leaves
`BASH_ARGC` unset *inside* the call yet materialises the base *after*
return). bash's own man page says "The shell sets BASH_ARGC only when in
extended debugging mode", so osh follows that documented contract and
populates BASH_ARGC/BASH_ARGV only under `extdebug`. Replicating the
undocumented non-extdebug quirk was judged not worth the bug risk for a
behaviour no real script relies on.
