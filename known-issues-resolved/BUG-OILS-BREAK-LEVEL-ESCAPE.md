### BUG-OILS-BREAK-LEVEL-ESCAPE. `break N` past the loop nesting depth silently truncated the rest of the script — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** A `break N` (or `continue N`) whose level exceeded the number of
enclosing loops kept unwinding after the last loop was gone. At top level the
flow value walked straight out of the script: every command after the loop
simply never ran, with no diagnostic on stderr and exit status 0 — a silent
truncation, the worst failure mode a shell can have. Reproduced with:

```sh
for a in 1 2; do for b in x y; do break 9; done; done
echo "this line never printed"
```

**Root cause.** `userspace/oils/src/interp.rs`, the `"break"`/`"continue"`
builtin arms: the level was taken as `n.max(1)` and each loop executor
decrements it by one, returning `Flow::Break(n - 1)` while `n > 1`. Nothing
bounded `n` by the actual nesting depth, so the surplus levels propagated past
the outermost loop into `exec_program`'s caller.

**Fix.** `Interp::clamp_loop_level(n, depth)` clamps the level to
`self.loop_depth`, which is exactly what bash does (`continue 9` two loops deep
restarts the *outer* loop; `break 9` leaves both loops with status 0).
`loop_depth` is already reset to 0 on entry to a function body, so a clamped
level can never reach into a caller's loops — matching bash, where `break`
inside a function called from a loop is the "only meaningful in a loop" no-op
rather than a break of the caller's loop.

**Found by** the `loops.sh` differential corpus case, added in the same change;
covered by the `break_continue_level_clamped_to_loop_depth` unit test.

**Related, fixed in the same commit:** `select` terminating at EOF wrote its
closing newline to stderr instead of stdout, left `REPLY` untouched instead of
clearing it, and reported the body's status instead of 1. The existing
`select_eof_terminates` unit test had *encoded the wrong stdout behaviour as
expected*, which is why it survived — a reminder that a unit test asserting our
own output is only as good as the reference it was checked against.
