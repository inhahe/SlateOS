## 1019. A program that cannot keep its promise execs or refuses, depending on whether the promise was a constraint

**Lane:** B
**Date:** 2026-09-10
**Decided by:** Claude (autonomous)

**In short:** Several programs in `userspace/` printed "would exec ..." and
exited 0, because whoever wrote them believed this OS could not start another
program. It can. But when I went to fix them, "just exec it" turned out to be
right for one and actively dangerous for the others, and the difference is not
about how much of the program is finished. It is about **what the caller was
promised**. If the missing piece was a *restriction* — a sandbox, a capability
drop, a resource limit — then running the program without it delivers the one
outcome the caller was trying to prevent. Those refuse. If the missing piece
was nothing at all, and the exec *is* the job, they exec.

### The rule

> When a program cannot do what its name promises, ask what the caller loses
> if it proceeds anyway.
>
> * If proceeding gives the caller **less** than they asked for, but nothing
>   they were guarding against — proceed. `getty`'s exec drops no privilege
>   and adds no risk; handing the terminal to `login(1)` is the entire job.
> * If proceeding gives the caller **more authority, less isolation, or fewer
>   limits than they asked for** — refuse, with a non-zero status naming what
>   could not be applied. The request was for the constraint, not for the
>   command.

The second case is counter-intuitive and is the reason this entry exists: the
program *is capable* of running the command, and running it looks like the
more helpful choice. It is not, because the person who typed `firejail` or
`capsh --drop=...` or `cgexec -g memory:capped` is by definition the person
who cannot afford the unconstrained version.

### How it decided four programs

| Program | Missing piece | Decision |
|---|---|---|
| `cgexec` | joining the cgroup | **refuse** — exit 1 rather than run outside the limits |
| `getty` | nothing; exec was assumed unavailable | **exec** — `login(1)` inherits the terminal, session and pid |
| `firejail` | namespaces, mount restrictions, seccomp | **refuse** — running unsandboxed is the outcome the request exists to prevent |
| `capsh` | applying capability changes | **refuse** — exec'ing would grant *more* authority than asked to keep |

`firejail` additionally printed `Sandbox <name> started (PID <pid>)`, which was
not a "would" at all but a false statement of fact: nothing was unshared,
remounted or filtered. That line is gone.

### Alternatives considered

**Exec everywhere, and warn on stderr.** Rejected. A warning that the sandbox
is inactive, printed to a stream the caller may not be reading, next to a
program that then runs with full authority, is how a security tool becomes
worse than its own absence — the caller is *more* exposed than if `firejail`
had not been installed, because they believe they are confined.

**Delete them under `design-decisions.md` 1006.** Rejected for these four,
though it remains right for commands that state facts they did not measure.
These do real work — `firejail` parses and validates profiles, `capsh` computes
the capability state and prints it — and they now report accurately that they
cannot finish. 1006 deletes a command that *lies*; a command that refuses with
an accurate reason is the thing 1006 wants stubs replaced *by*.

**Keep exiting 0 with "would exec".** Rejected: a script cannot tell that from
success, which is the whole problem. The exit status is the only part of this
a caller reliably reads.

### What this does not settle

Whether `capsh` and `firejail` should eventually *work*. They should; the
kernel has a capability system and `capsh --drop all` is named in
`kernel/src/syscall/linux.rs` as a caller it expects. This entry is about what
they do in the meantime, and the answer is "refuse audibly" rather than
"pretend" or "vanish".
