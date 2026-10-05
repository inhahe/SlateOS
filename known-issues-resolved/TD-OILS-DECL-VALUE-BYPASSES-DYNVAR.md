### TD-OILS-DECL-VALUE-BYPASSES-DYNVAR. `declare NAME=v` on a variable the shell computes stores a shadow instead of moving the counter — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `builtin_declare`'s scalar
store, the three `put_var` arms at the end of the operand loop (integer
attribute / case attribute / plain). They write the variable table
directly, so none of the dynamic-special assignment machinery in
`apply_assignment` is reached: the name's side effect never runs and the
`vars` entry that is made is then ignored by the dynamic read, which is
what makes the value simply vanish.

This is the same bug `TD-OILS-ATTR-ASSIGN-BYPASSES-DYNVAR` described for
`export`/`readonly`, which were fixed by routing their store through
`Shell::attr_store`; `declare`/`local`/`typeset` were not.

```sh
SECONDS=7;             echo "[$SECONDS]"   # both [7]
declare SECONDS=7;     echo "[$SECONDS]"   # bash [7]   osh [0]
declare -i SECONDS=7;  echo "[$SECONDS]"   # bash [7]   osh [0]
declare -i SECONDS; SECONDS=7; echo "[$SECONDS]"        # both [7]
export SECONDS=7;      echo "[$SECONDS]"   # both [7]   (fixed already)
declare -i RANDOM=1; a=$RANDOM
declare -i RANDOM=1; b=$RANDOM             # bash a=b   osh a≠b (never reseeded)
declare -i BASH_SUBSHELL=9; echo "[$BASH_SUBSHELL]"     # bash [9]   osh [1]
declare -i HISTCMD=3+4; declare -p HISTCMD # bash declare -i HISTCMD="0"   osh "7"
```

**Proper fix.** Route the scalar store through `apply_assignment` the way
`Shell::attr_store` does — the same `WordPart::SingleQuoted` carrier, so
the already-expanded value is not expanded a second time. The three arms
collapse into one call, because `apply_assignment` applies the integer
and case attributes itself. Two things must survive the move: a bad `-i`
value still has to leave the name created-but-unset (`self.declared`) and
`break` out of the operand loop, and the array/nameref arms above must
keep their own paths. Check afterwards that `declare -p NAME` of a
computed name still reports the live reading rather than a shadow.

**Fixed.** `Shell::attr_store` — already the routing `export`/`readonly`
use — gained a `traced: bool` parameter, because the one thing the four
builtins disagree about is the `set -x` output: `export`/`readonly` show
the inner assignment on a line of its own below the builtin's,
`declare`/`local`/`typeset` do not. `builtin_declare`'s two scalar
`put_var` arms collapsed into a single `attr_store(name, append, v,
false)` call, so the declaration now reaches everything a plain
`name=value` reaches — the integer attribute evaluating the value, the
case attributes folding it, `set -a` marking the name for export, and the
computed name's own assign function running.

That last part turned up a rule the old code had accidentally been
getting half-right: **a `local` of one of these names makes an *ordinary*
variable.** bash's value and assign functions belong to the *global*, so
inside the frame the name neither computes on a read nor reaches the
counter on a write, and the shell's own comes back when the frame pops
(`f() { local SECONDS=9; echo $SECONDS; }` prints 9 and leaves the clock
alone; a bare `declare` in a frame is a `local` and behaves the same;
only `declare -g` names the global). Routing the store made the write
half wrong, and the read half — `f() { local SECONDS; SECONDS=7; }`
writing the counter instead of the local — had been wrong all along. Both
are now guarded by the existing `Shell::is_locally_shadowed`: one in
`dynamic_special_value`, one on the `dynamic` flag in `apply_assignment`.

Covered by the unit test
`a_declarations_value_operand_is_the_ordinary_assignment` and by
`tests/corpus/bash-declare-value-dynvar.sh`.
