### BUG-OILS-TRAP-CLOBBERS-PIPESTATUS. A trap handler left its own `${PIPESTATUS[@]}` behind — ✅ **RESOLVED 2026-08-01** — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `fire_trap_status`, which saved and
restored `$?` around a DEBUG/ERR/RETURN/signal handler but not the array.

**Reproduce:**

```sh
h() { :; }
trap h DEBUG
false | true | false
echo "[${PIPESTATUS[*]}]"
```

bash prints `[1 0 1]`; osh printed `[0]` — the handler's own `:` had rewritten
the array, and then so had the handler firing for the `echo` itself.

**Why it mattered more than it looks.** Nearly every handler body runs *some*
command, and in osh as in bash every command writes a one-element
`${PIPESTATUS[@]}`, so the array was destroyed by the mere presence of a trap.
The DEBUG trap makes it worse still: it fires once per simple pipeline stage
*and* once for the command that goes on to read the array, so several handler
bodies stood between a pipeline and its own status list. This is what made a
side-by-side probe of the pipeline DEBUG-trap verdict unreadable — every
`${PIPESTATUS[*]}` reading in it came back `[0]` whatever the verdict did.

**Fixed.** `fire_trap_status` now saves `arrays["PIPESTATUS"]` next to
`last_status` and puts it back on the same terms — that is, unless the handler
asked to exit, in which case the handler's own outcome is the shell's. The
handler's *body* still sees the array live and moving, because only the
caller's view is restored; a handler can therefore read the pipeline that fired
it and then read its own.

Covered by `tests/corpus/trap-handler-preserves-pipestatus.sh`.
