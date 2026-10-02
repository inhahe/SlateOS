### TD-OILS-BUILTIN-TAG-SURVIVES-A-NESTED-COMMAND. `osh` restores a builtin's diagnostic tag after running a nested command; bash's is cleared — 2026-08-03 — ✅ **RESOLVED 2026-08-08**

**Where:** `userspace/oils/src/interp.rs` — `Shell::arith_cmd`, saved and
restored around builtin dispatch (~21366 / ~21909).

**What:** bash's `this_command_name` is a plain global that every builtin
assigns at entry and nobody restores, and executing *any* command clears it. So
a diagnostic a builtin raises after running nested shell code is unsigned:

```sh
declare -ai q
mapfile -t -C 'echo cb $1 $2' -c 1 q <<< $'1+1\nq+'
# bash:  case.sh: line 2: q+: syntax error: operand expected …
# osh:   case.sh: line 2: mapfile: q+: syntax error: operand expected …
```

osh saves and restores the tag, so `mapfile`'s survives the callback. The line
number is right in both (fixed 2026-08-03); only the tag differs.

**Fixed in `HEAD`.** The entry's guess at the mechanism was wrong in a way that
mattered, so it is worth writing down what bash actually does.

bash does not clear the name on the way *in* to a non-builtin, and it does not
restore it either. It **blanks** it on the way *out* of every simple command:

```c
 return_result:
   …
   this_command_name = (char *)NULL;	/* points to freed memory now */
                                    	   (execute_cmd.c:4828)
```

— the counterpart to the one place it is set, `run_builtin:` at
`execute_cmd.c:4684`, which assigns `words->word->word` *after* word expansion.
osh already modelled exactly this for the sibling tag `expand_cmd`, at the end
of `Shell::exec_simple_command`; the fix is the same line for `arith_cmd`,
immediately beside it. Nothing else changed — in particular the per-builtin
save/restore in `run_builtin_body` stays, because the clear dominates it on
every path that runs a nested command, and it is still what carries the tag
correctly when no command runs at all.

Measured against bash 5.2.37, which pins the rule tighter than the entry did.
"Cleared" means *any executed simple command*, and only that:

| callback in `mapfile -t -C … -c 1 q <<< 'q+'` | bash's complaint |
|---|---|
| `true`, `:` (builtin) | `q+: syntax error…` — unsigned |
| `f` (function) | unsigned |
| `no-such-command-at-all` | unsigned — a failed lookup still reaches `return_result:` |
| `true \| true` (pipeline) | `mapfile: q+: …` — **still signed** |

The pipeline is the load-bearing exception: bash forks for every element of one,
so the child's blanking never reaches the parent. The same is true of command
substitution, and that is what keeps the ordinary cases signed —
`let "x=$(echo 1)+"` is still `let: x=1+: …` and `declare n=$(echo 1)+` is still
`declare: 1+: …`, even though both run a command inside the expansion. osh runs
a substitution in-process, so what stands in for bash's fork there is
`Shell::eval_arith_raw`'s explicit bracketing, which lifts the tag across the
expansion and puts it back.

Covered by the lib test `running_a_command_takes_the_builtins_signature_away`
and the corpus case
`a-builtins-signature-is-taken-away-by-running-a-command.sh`.
