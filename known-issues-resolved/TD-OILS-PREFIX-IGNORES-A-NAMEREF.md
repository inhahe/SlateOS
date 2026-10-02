### TD-OILS-PREFIX-IGNORES-A-NAMEREF. A command's assignment prefix bound the reference itself instead of the name it resolves to — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::prefix_assignments`.

**What:** `FOO=bar cmd` binds the name a nameref *resolves to*, not the
reference. osh bound the reference itself, so the target never saw the value and
the command's environment carried the wrong name. Measured against bash 5.2.37:

```sh
t=orig; declare -n r=t
r=V env | grep -E '^(t|r)='
# bash: t=V        osh: r=V
r=V eval 'echo "$t"'
# bash: V          osh: orig
```

**Fixed 2026-08-04.** The name pushed into the temporary environment is now the
one `Shell::resolve_ref_use` walks the chain to, rather than the name as
written. The readonly test, which already resolved the chain, now tests that
same bound name. Confirmed against bash:

- The target is bound **under its own name and only that one** — `env` shows
  `t=V`, never `r=V` — and the nameref cell is *not* shadowed, which is how
  `$r` still reads the binding. A chain is followed to its end (only the far end
  is in the environment), and a target nothing has bound is created for the
  duration and gone afterwards.
- Two endings name no variable a scope could bind, and both fall back to the
  name **as written**, which then shadows the nameref cell for the command's
  duration: a chain ending in an **element** (`declare -n r='a[1]'` binds `r`
  and leaves the array alone — the temporary environment holds variables, not
  elements), and a **circular** chain, which warns first
  (`warning: c: circular name reference`, naming the variable written rather
  than any member of the cycle). Neither affects the command's own status.
- Testing the *bound* name is also why a readonly **array** behind an element
  reference no longer refuses — nothing would be written to it. osh previously
  tested the resolved base even for an element and wrongly reported
  `r: readonly variable`.
- A refusal is still reported by the name **as written**
  (`declare -n n=ro; readonly ro; n=V cmd` says `n`, where the same refusal
  through `((n=5))` says `ro`), and still lets the command run.
- `set -x` is the one place the written name survives: the trace echoes the
  assignment as written, nameref and all.

**Pinned by** `tests/corpus/a-prefix-assignment-binds-what-its-nameref-resolves-to.sh`
(20 sections) and the `a_prefix_assignment_binds_what_its_nameref_resolves_to`
unit test in `src/interp.rs`.
