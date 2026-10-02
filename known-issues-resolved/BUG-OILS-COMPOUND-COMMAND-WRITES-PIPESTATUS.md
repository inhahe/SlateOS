### BUG-OILS-COMPOUND-COMMAND-WRITES-PIPESTATUS. A compound command overwrote `${PIPESTATUS[@]}` — ✅ **RESOLVED 2026-08-01**

**Where:** `userspace/oils/src/interp.rs` — `exec_pipeline` ended by calling
`finish_pipeline` unconditionally, so *every* command, including a one-command
"pipeline" whose command was a `{ … }`, `if`, `for`, `while`, `case`, `select`
or a function definition, wrote a fresh one-element array.

**Reproduce:**

```sh
false | false; { true | true; };        echo "[${PIPESTATUS[*]}]"  # bash [0 0], osh [0]
true | true;   while false; do :; done; echo "[${PIPESTATUS[*]}]"  # bash [1],   osh [0]
true | true;   case x in y) :;; esac;   echo "[${PIPESTATUS[*]}]"  # bash [0 0], osh [0]
true | true;   f() { :; };              echo "[${PIPESTATUS[*]}]"  # bash [0 0], osh [0]
```

Row 1 is the group leaving the inner pipeline's two-element array standing. Row
2 is the one place a construct writes an array without any of its *body*
running: `while`'s condition is a simple command, so the refused loop still
leaves `false`'s `[1]` behind. Rows 3 and 4 run nothing at all, so the array is
still the one `true | true` published two commands earlier.

**Why it was hard to see.** A compound's status is usually the status of the
last command inside it, so the wrong one-element array normally holds the right
number. It only shows when the last thing to run inside is a *multi-stage*
pipeline (whose array is longer than one), or when nothing inside runs at all
(so the array should predate the whole construct).

**The rule.** bash writes the array where it waits for something — a simple
command (a builtin, an external, a function *call*), a subshell, `[[ … ]]`,
`(( … ))` — and not where it merely arranges other commands. A compound that
runs in the current shell leaves the array exactly as the last command inside
it wrote it; if nothing inside ran, as it was before the construct. `!` and
`&&`/`||` write nothing either. Redirections do not change the answer for the
command they are attached to, including when they fail: `( … ) > f` writes one
element and `{ … } > f` writes none, error or no error.

**Fixed** by `Shell::publishes_pipestatus`, consulted by `exec_pipeline` before
`finish_pipeline` for a one-command pipeline. `$?` needs no separate handling —
a single command's status is the pipeline's already, so skipping
`finish_pipeline` skips only the array.

Covered by `tests/corpus/pipestatus-is-not-written-by-a-compound-command.sh`.
