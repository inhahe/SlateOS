## 720. `sh` ends a command by unwinding a `Result`, not by exiting the process — which is what lets a failed special builtin be fatal in the right places

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous)
**Lane:** B

**In short:** `exit 3`, `break`, `continue` and `return` all mean "stop what you
are doing and leave", and so does a fatal error such as a bad substitution. The
simplest implementation is to call `exit()` there and then. This one instead
returns an error value that propagates up through every caller.

`Flow` is that value — `Break(n)`, `Continue(n)`, `Return(n)`, `Exit(n)` — and
`Run<T>` is `Result<T, Flow>`, so every executing function's `?` carries it.

*What changes:* `exit 3` inside `( … )` sets the subshell's status and the
script continues; `break` inside a function called from a loop unwinds to the
loop; and a failed `.`, `eval`, `shift` or `export` — the "special builtins",
which POSIX says end a non-interactive shell — ends it, but ends only the
subshell when it is inside one.

- **For:** every one of those behaviours is *positional* — the same `exit 3`
  means different things depending on what encloses it — and a process exit
  cannot be positional. It is also what allows a subshell to be a
  snapshot-and-restore (§717): the restore has to run, and it cannot if the
  process is gone. And it makes the shell testable in-process: the unit tests
  run whole scripts and read the status back, which a shell that calls `exit()`
  cannot offer.
- **Against:** every function in the executor returns `Run<T>` whether or not it
  can fail that way, which is visible noise; and a caller that writes `let _ =`
  instead of `?` silently swallows an `exit`.
- **Why the "for" wins:** the noise is one type alias, and the swallowing risk
  is the same one `Result` carries everywhere in this codebase, where the
  house rule already forbids discarding one without a comment. The alternative
  was in the previous implementation, which used `stdfd::exit_now` and therefore
  could not have had `( exit 3 ); echo still-here` work at all.
