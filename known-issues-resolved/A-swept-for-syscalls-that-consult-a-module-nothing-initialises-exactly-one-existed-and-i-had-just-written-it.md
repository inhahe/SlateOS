### [A] Swept for syscalls that consult a module nothing initialises: exactly one existed, and I had just written it -- 2026-09-21
**Status:** CLOSED (one instance, fixed in `7572d82e4`; sweep found no others)

**In short:** several kernel modules refuse to work until something calls
their `init_defaults()`. For most of them the only thing that ever does is a
`/proc` read or a shell command, which happen late in boot. If a *syscall*
reaches such a module before then, it gets a flat refusal. I created one of
these this morning without noticing, so I checked whether there were others.
There were not.

**The instance.** `sys_dns_resolve` began consulting `fs::nameservice`, whose
`with_state` returns `NotSupported` when the table is unset. Its
`init_defaults` is called from `procfs.rs` (a `/proc/nameservice` read, at
`main.rs:4427`) and `kshell.rs`. The new hosts-table self-test runs from
`self_test_fs` at `main.rs:1697` -- earlier. So the lookup would have got
`NotSupported`, fallen through to DNS, and sent `localhost` to the wire:
**the exact bug the change was written to fix, one layer down.**

**The sweep, narrowed twice because the first two numbers were not the
defect:**

| question | answer |
|---|---|
| modules with an `init_defaults()` | 312 |
| ...whose only callers are `procfs`/`kshell`/themselves | 283 |
| ...**and** that a syscall handler actually consults | **1** |
| ...that survives reading the match | **0** |

283 is not a bug count. Most of those modules are consulted *only* from
`/proc` and `kshell` as well, which is the separate problem filed as A-Q21 --
counting them here would have been the same over-reporting I corrected three
times today. The single survivor was `fdtable`, and it is a false positive:
the one reference under `kernel/src/syscall/` is inside a comment
(`// fdtable::MAX_FDS`), and `MAX_FDS` is a constant that needs no state.

**Why the negative result is worth writing down.** "No other syscall has
this problem" is the kind of claim that is usually an assumption. Here it is
a measurement, and the measurement is cheap to repeat: modules with
`init_defaults`, intersected with modules named under `kernel/src/syscall/`,
minus those a syscall path initialises itself. Anyone adding a syscall that
reaches a stateful module should re-run it, or simply follow
`sys_hostname_set`, `sys_domainname_set` and `sys_keylayout_set`, which all
open by calling their module's `init_defaults` for exactly this reason.
