### TD-OILS-HOST-ARGV0. External commands see the resolved absolute path as `argv[0]` on the *Windows host build* instead of the command word as typed (correct on the slateos/unix target) — NOT-A-BUG on target / host-only test artifact — 2026-07-20 — **`cfg(unix)` path now measured, 2026-08-25**

> **"Validate argv[0] behavior on the slateos target when a ring-3 exec
> self-test exists" — partly discharged, sooner and more cheaply than that.**
> `scripts/osh-diff.sh` builds osh for `x86_64-unknown-linux-gnu`, where
> `cfg(unix)` is true, so the `CommandExt::arg0` override below is *compiled in
> and exercised* — the same code the slateos target uses. Every host-only claim
> in this entry, including the `exec -a`/`-l`/`-c` paragraph, was re-measured
> against glibc bash:
>
> | probe | osh | glibc bash |
> |---|---|---|
> | `cat /nope` | `cat: /nope: No such file or directory` | identical |
> | `sh -c 'echo $0'` | `sh` | identical |
> | `exec -a myname sh -c 'echo $0'` | `myname` | identical |
> | `exec -l sh -c 'echo $0'` | `-sh` | identical |
> | `exec -c env` | (empty) | identical |
>
> So `argv[0]` is the word as typed, `exec -a` chooses it, `exec -l` prefixes
> the `-`, and `exec -c` really does clear the environment. All five were
> unobservable under the old apparatus and all five are correct.
>
> **What this does not prove:** slateos is not Linux, and the ring-3 exec
> self-test is still the thing that would test *slateos'* exec. What is now
> settled is that the code is right and the Windows host was the only thing
> hiding it — the remaining risk is in the target's exec implementation, not in
> osh's use of it.

**Where:** `userspace/oils/src/interp.rs` (`exec_external`, the `PCommand`
construction ~line 5664). The `arg0` override is `#[cfg(unix)]`.

**What:** bash execs the PATH-resolved binary but hands the child `argv[0]` set
to the command word *exactly as typed* (`cat`, not `/usr/bin/cat`), so a program
reports its own name the way the user invoked it. Probe symptoms on the host:
`osh -c 'cat /nope'` prints `/usr/bin/cat: /nope: No such file or directory`
(bash: `cat: …`), and `osh -c 'sh -c "echo \$0"'` prints `/usr/bin/sh` (bash:
`sh`). This affects any external program's self-named error prefix, `$0` in a
child shell script, and `ps`/process listings.

**Why NOT a bug on target:** osh *does* set `argv[0]` to the typed name via
`std::os::unix::process::CommandExt::arg0`, which is compiled in for the
slateos target (`cfg(unix)` true) — so the shipped OS matches bash. The
divergence exists only on the `x86_64-pc-windows-gnu` host build, where
`std::process::Command` has no `argv[0]` override and the MSYS runtime
reconstructs `argv[0]` from the full executable path. Same disposition family as
the `/tmp`→`D:\tmp` path and `$HOME` format artifacts: a host-execution
difference, not a target-behavior bug. No action needed; validate argv[0]
behavior on the slateos target when a ring-3 exec self-test exists.

**Same mechanism, same host-only limits (2026-07-31):** `exec -a name` and
`exec -l` (which choose the `argv[0]` a command sees, and prefix it with `-`)
are implemented through that very `#[cfg(unix)]` override, so on the Windows
host they parse and are consumed but have no observable effect; on the slateos
target they work. `exec -c` has a neighbouring artifact: it clears the child's
environment (`Command::env_clear`), which is exact, but the MSYS runtime then
synthesises `HOME`/`TERM` for an MSYS child, so a host probe of `exec -c env`
prints those two lines where MSYS bash prints `MSYSTEM`/`SYSTEMROOT`/`WINDIR`.
Both are why `tests/corpus/exec-options.sh` exercises the option *parsing* and
diagnostics rather than the observable `argv[0]`/environment.
