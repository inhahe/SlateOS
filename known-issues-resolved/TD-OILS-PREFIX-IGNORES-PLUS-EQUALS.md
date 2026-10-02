### TD-OILS-PREFIX-IGNORES-PLUS-EQUALS. A `+=` command prefix assigned rather than appended, and a prefix assignment did not see the ones before it — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::prefix_assignments` and
`Shell::prefix_assignment_value`, which expanded `a.value` against the pre-prefix
state and never looked at the assignment's append flag.

**What:** `t=orig; t+=V eval 'echo "$t"'` prints `origV` in bash and `V` in osh.
The temporary environment's binding is a fresh variable, but its *value* starts
from what the name held before the prefix. The shadowed variable's `-i`
attribute drives the append too, even though the binding itself carries no
attributes: `declare -i n=5; n+=3 eval 'echo "$n"'` prints `8` in bash (`3` in
osh) — arithmetic addition, not string concatenation.

Discovered while fixing TD-OILS-PREFIX-IGNORES-A-NAMEREF, and it compounds with
it: the value to append to is the **resolved target's**, so
`t=orig; declare -n r=t; r+=V eval 'echo "$t"'` is `origV`.

**Related, same root cause — a later prefix assignment does not see an earlier
one.** bash binds each assignment into the temporary environment *before* it
evaluates the next, so `v=a; v=X v=${v}Z eval 'echo "$v"'` prints `XZ`; osh
evaluates every value against the pre-prefix state and prints `aZ`. `+=` is the
same fact seen from one step further along: the value to append to is simply
whatever the name holds at that point in the prefix.

**Fixed 2026-08-04.** The temporary environment is now built **incrementally**:
`Shell::prefix_assignments` opens the prefix scope and *stages* each binding —
binds it for real — as its value is evaluated, so an expansion in a later value
reads the earlier binding and `+=` reads the name's current value. Appending
reuses `Shell::appended_attributed_value`, the rule an ordinary `+=` already
follows, rather than a second copy of it. The staging comes straight back off
before returning, because the bindings are *not* in effect for the command's own
words or redirections; the caller re-applies the finished list once it knows the
command will really run, and the snapshots come out the same either way because
only a name's first occurrence records what it displaced
(TD-OILS-PREFIX-DUPLICATE-NAME-RESTORE). Confirmed against bash 5.2.37:

- A binding is visible to a later value every way an in-effect variable is: to
  its parameter expansion, its arithmetic, its `${x-default}`, a command
  substitution inside it *and that substitution's own environment* (a prefix
  binding is exported), a function called from one, and through a nameref — and
  it shadows a name the shell would otherwise compute for itself
  (`SECONDS=100 z=$SECONDS cmd` passes `100`).
- …but not to the command's own words or redirections, which are expanded with
  the prefix out of effect (`v=orig; v=NEW printf '%s' "$v"` prints `orig`).
- `+=` appends by the ordinary rule: `-i` adds numerically over the whole value
  (`m+=2*3` → `16`), the case attributes fold the new text before it is
  concatenated (`declare -u p=ab; p+=cd` → `ABCD`), an array name reads element
  zero and an associative one key `0`. A bad `-i` expression is **fatal** — the
  command never runs.
- A staged binding is a fresh environment variable carrying no attributes, which
  is why only a name's *first* append can be arithmetic: `declare -i n=5;
  n+=3 n+=4 cmd` passes `84`, not `12`, and `declare -u q=ab; q+=cd q+=ef`
  passes `ABCDef`.
- The trace shows the value actually bound, so an append is traced under a plain
  `=` (`y=a; y+=b cmd` traces `+ y=ab`).

**The one piece of state staging must not disturb** is the command hash.
Everything `Shell::note_var_change` keeps in step with a variable is *derived*
from it, so restoring the variable re-derives it — except `$PATH`, whose change
**empties** the table, which cannot be re-derived. That flush belongs to the real
binding, and bash forks before evaluating the prefix of a pipeline stage or a `&`
job, so `hash -r; w.sh; PATH=dir w.sh | cat; hash` leaves the shell's table
intact. `prefix_assignments` therefore saves the table before a staged `$PATH`
empties it and puts it back after unwinding — while the emptying still stands
*within* the prefix, which is what bash shows: `PATH=dir v=$(hash) cmd` sees an
empty table and `PATH=dir v=$(command -v w.sh) cmd` searches the bound `$PATH`.
Caught by the `path-prefix-assignment-search` corpus case, which now pins both
halves.

**Pinned by** `tests/corpus/a-prefix-assignment-sees-the-ones-before-it.sh` (31
sections), the two new sections of
`tests/corpus/path-prefix-assignment-search.sh`, and the
`a_prefix_assignment_sees_the_ones_before_it` unit test in `src/interp.rs`.
