# mono-spike — does the Mono runtime link against SlateOS's libc?

**Yes, as of 2026-10-07: Mono 6.14.1's runtime, `mono-sgen`, links with
nothing missing.** The first build stopped in a header, not the library.
Mono includes both `<sys/mount.h>` and the kernel's `<linux/mount.h>`, and
our `<sys/mount.h>` defined the new mount API's enum and struct a second
time. glibc 2.36 shipped the same break; the fix was glibc's
(known-issues-resolved/D-POSIX-SYS-MOUNT-H-COLLIDED-WITH-LINUX-MOUNT-H.md).
After it, configure and the whole build -- eglib, the metadata library,
SGen, the JIT -- succeed through our libc on the first attempt.

| | mono-sgen |
|---|---|
| undefined symbols | **0** |
| duplicate symbols | **0** |
| link exit | 0 |
| binary | 23,893,768-byte static `ET_EXEC` |
| `--strip-debug` / `--strip-all` | 7,620,760 / 6,695,944 |

`file` calls it "static-pie", which it is not. Mono links with
`-Wl,--export-dynamic`, and that gives a static executable a `PT_DYNAMIC`
segment. readelf's type is `EXEC`, and it has no `PT_INTERP`, which is what
the kernel goes by when it decides whether a program is dynamically linked
(`kernel/src/proc/elf.rs`, `interp_path`). It carries this library's Rust
machinery and none of musl's, so the link is against ours.

What configure decided, from its `config.h`: cooperative suspend on
(`ENABLE_COOP_SUSPEND`), thread-local storage by `__thread`, and
`sigaction`, `dl_iterate_phdr` and `pthread_attr_getstack` present.

Run `./run.sh` from WSL to reproduce. It is the "try the port before you write
a line" step from `roadmap-detailed.md`'s *Porting vs. Reimplementing* policy,
applied to the .NET runtime the operator asked for in design-decisions.md 1050
("I want Mono (dotnet support for Linux) ported too"), which `roadmap.md`
suggests for lane D, as a language runtime like CPython. It is shaped like
`scripts/gdb-spike/run.sh`; the spikes answer the same question.

## What is measured

Mono **6.14.1**, the current release from WineHQ, which has maintained Mono
since Microsoft handed it over in 2024. The source is unmodified and
cross-configured with `--build` and `--host`, so that configure runs nothing it
builds. Everything is compiled with zig's `cc` against musl's headers, with
`posix/include` in front of them. Every link -- configure's tests included --
is zig's `ld.lld` against `toolchain/sysroot/lib/libc.a`
(`slate_make_link_wrappers` in `scripts/lib/worktree.sh`).

What is built is the runtime: `mono-sgen`, the JIT compiler and virtual
machine with the SGen garbage collector, with libmono linked into it. The
omissions are listed with their reasons in `run.sh`. The C# compiler and the
class libraries are .NET assemblies, the same on every system, and are a
second step. Mono.Posix's native half and the TLS provider are each a change
or a port of their own.

## What this does not answer

Whether the runtime runs a program. Beyond the class libraries, two of its
demands on the system are known now, from its source, and neither is a
question of linking:

- **A fault becomes an exception.** Mono turns a null dereference into a
  `NullReferenceException`. Its `SIGSEGV` handler runs on an alternate signal
  stack (`sigaltstack`), so that a stack overflow can be handled too. It
  rewrites the faulting thread's saved context, and returning from the
  handler resumes that thread at the code that throws. Division by zero comes
  the same way through `SIGFPE`. The kernel must deliver these signals with a
  context the handler can change, and must honour the change on return.
- **The collector stops threads.** By default SGen suspends each thread with a
  real-time signal (`pthread_kill`), and the thread waits in `sigsuspend`
  until a restart signal arrives. That needs a signal mask per thread, and a
  thread's mask here is the process's. This spike builds with
  `--enable-cooperative-suspend` instead: threads stop at safepoints the JIT
  emits, and no signal is involved.

## Sources

Pinned in `scripts/lib/worktree.sh` by its sha256. The comment above the pin
names the three packagers, Alpine, Fedora and Homebrew, whose digests match
it.
