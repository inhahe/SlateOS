### TD-OILS-SELECT-OVERWRITES-A-NAMEREF. `select NAME` wrote over the nameref cell instead of assigning through it — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::exec_select`.

**What:** `select NAME in …` binds the choice with an *ordinary scalar
assignment* to `NAME`, so it goes **through** a nameref exactly as a plain
`NAME=value` does. osh wrote the choice straight into the nameref cell,
destroying the nameref and leaving its target untouched. Measured against bash
5.2.37:

```sh
t=orig; declare -n r=t
select r in aa bb; do break; done <<< 1
declare -p r t
# bash: declare -n r="t" / declare -- t="aa"
# osh:  declare -n r="aa" / declare -- t="orig"
```

This is exactly where `select` parts company with `for`, which deliberately
does *not* follow a nameref and overwrites the cell — osh already had `for`
right, which is presumably how `select` came to be modelled on it.

**Fixed 2026-08-04.** The assignment now goes through `Shell::set_scalar_checked`,
the same helper every other checked scalar write uses, instead of storing
directly to `ScalarDest::Var(name)`. That one change brings the whole family of
behaviours over at once, all confirmed against bash:

- A nameref to an **array element** writes the element (`declare -n r='a[1]'`),
  a **chain** is followed to its end, and a nameref to a name nothing has bound
  **creates** it.
- Every refusal ends the loop with **status 1**, leaving the target as it was,
  and each is reported the way that write is reported anywhere else: a circular
  chain warns `warning: NAME: circular name reference` blaming the name
  *written*; a readonly target blames the name **resolved to**, so
  `declare -n n=ro; select n …` says `ro: readonly variable`; and a name the
  shell maintains itself (`FUNCNAME`) refuses **silently** — status 1 with
  nothing said at all. All of them happen only after the choice has been read,
  so the menu and prompt have already been written.
- The value attributes of whatever the write lands on still apply (`-i`
  evaluates the item as arithmetic — `3*4` → `12`, a non-numeric item → `0`;
  `-u` upper-cases it) and `set -a` exports it, because this is now the same
  store as any other scalar assignment. Previously only the attributes of the
  *nameref cell* could apply.

The old code's hand-rolled readonly guard blamed the name written rather than
the one resolved to, which is why it could not simply be kept alongside.

**Pinned by** `tests/corpus/select-binds-its-name-through-a-nameref.sh` (18
sections, including the contrasting `for`-loop case) and the
`select_binds_its_name_through_a_nameref` unit test in `src/interp.rs`.
