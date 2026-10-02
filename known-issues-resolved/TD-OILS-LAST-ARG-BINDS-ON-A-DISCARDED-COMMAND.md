### TD-OILS-LAST-ARG-BINDS-ON-A-DISCARDED-COMMAND. `$_` takes the words of a command that never ran — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/interp.rs` — `Shell::exec_simple_inner` (~14013)
arms `pending_last_arg` with the last expanded word *before* the three
word-expansion-error checks below it (`unbound_error`, `take_discard_flow`,
`glob_error`), each of which returns without running the command; `exec_simple`
then binds `$_` from the armed slot regardless.

**What.** bash updates `$_` when a command *runs*. A command discarded by a
word-expansion error leaves the previous binding alone:

```sh
echo alpha beta
echo "boom $(( 1/0 ))"          # reported, command discarded
echo "[$_]"                     # bash [beta]   osh [boom 0]
```

All three discard kinds are affected — the arithmetic-expansion error above, a
`failglob` no-match (`bash [delta]` / `osh [echo]`), and a bad subscript
(`${a[1/0]}`, `bash [a]` / `osh [s 1]`).

**Proper fix.** Move the `pending_last_arg` arming below the three checks, so
only a command that reaches execution arms it. The slot-saving dance in
`exec_simple` is unaffected — it restores the *outer* command's pending word, and
a discarded inner command simply never sets one.

**A second, independent `$_` divergence** turned up in the same probe and wants
its own measurement before it is fixed: a declaration's compound array assignment
contributes the *name* to bash's `$_`, not the word.

```sh
declare -a a=(1 2); echo "[$_]"   # bash [a]      osh [-a]
b=(3 4);            echo "[$_]"   # bash []       osh []      (agree)
declare -i c=5;     echo "[$_]"   # bash [c=5]    osh [c=5]   (agree)
```

So it is specific to the *compound* (`name=(…)`) form inside a declaration
builtin — bash's argv for it apparently carries the bare name — while the scalar
form and a plain assignment already match.

**Impact.** Scripts that read `$_` after an error, which is a debugging idiom
more than a scripting one, and after `declare -a name=(…)`. Narrow, but `$_` is
supposed to be a faithful record of what just ran and here it records what did
not.

**Fixed.** The slot is no longer written at the end of the word-expansion loop.
It is written through one new method, `Shell::arm_last_arg`, called from exactly
the two points where the command is committed to running: after the pure-
assignment branch's own error checks (with an empty slice, which is what binds
the empty string), and after the command branch's prefix-assignment checks, just
before the `set -x` trace. Everything that returns before one of those two — the
three command-word expansion checks, the assignment branch's three, the readonly
rejection, and a compound operand that could not bind — now leaves the previous
binding alone by construction rather than by remembering to undo an arming.

The second divergence went with it: the command branch arms from `spliced` when
the command is a declaration builtin with compound operands, which is the word
list that already carries a bare name where each `name=(…)` was written (it is
what `set -x` traces and what `declare -p` is asked with). Measured: bash binds
`a` for `declare -a a=(1 2)`, `w=2` for `declare q=(1) w=2`, `q2` for
`declare w2=2 q2=(1)`, `t=9` for `export s=(1) t=9`.

Also measured while placing the calls, and now covered: a command that merely
*fails* has still run and does arm (`nosuchcmd a b` binds `b`, `echo mm >
/nosuch/dir/f` binds `mm`), and a redirect failure on a null command does not
take back the empty binding (`< nosuchfile` binds `""`). Corpus case
`dollar-underscore-takes-the-words-of-a-command-that-ran.sh`.
