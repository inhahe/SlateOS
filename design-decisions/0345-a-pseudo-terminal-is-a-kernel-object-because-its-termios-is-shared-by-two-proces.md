## §345 — A pseudo-terminal is a kernel object, because its `termios` is shared by two processes and its `^C` cannot wait for a reader

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)

**In short:** A pseudo-terminal lets one program run another *as if the second
were sitting at a terminal* — that is how `ssh` gives you a shell, how a
graphical terminal window works, and how `sudo` can prompt for a password. We
had none, and I had to choose where to build it: inside our C library (which
Lane B owns and could ship today) or inside the kernel (which Lane A owns, so it
becomes a request and a wait). I chose the kernel. Two specific things make the
library version not merely worse but *incorrect*, and both were found by
designing it rather than by assuming.

### The two blockers

1. **`termios` is shared state across an address-space boundary.** `termios` is
   the settings word for a terminal — whether typed characters are echoed back,
   whether input is delivered a line at a time or a key at a time, which key
   means "interrupt". The shell holds one end of the pty and calls `tcsetattr`
   to switch echo off for a password prompt; the terminal emulator holds the
   other end and must stop echoing *immediately*. Those are two processes. A
   libc-side pty built on two socketpair endpoints has nowhere to keep that one
   shared word — each side would have its own copy, and they would disagree the
   moment either changed it. The visible failure is a typed password appearing
   on screen.

2. **`^C` must fire when it is typed, not when someone next reads.** A
   userspace line discipline only executes inside a `read()` call. A program in
   a compute loop calls no `read()`, so it would be uninterruptible — which is
   the exact case `^C` exists for. Only something that sees the keystroke
   independently of the reader can deliver `SIGINT` on time, and in this system
   that is the kernel.

### The alternative, stated fairly

The libc version had real merits and I do not want them lost. It needs no other
lane, so it could land immediately rather than waiting on a request; it keeps
policy out of the kernel, which is what the microkernel design in `design.txt`
asks for; and `HandleKind::UnixStream` already gives an inheritable, refcounted,
kernel-backed bidirectional byte channel to build on, so the plumbing existed.
It fails only on the two points above — but those two are not edge cases, they
are the first two things any real user of a pty does.

### The tension this decision carries

`design.txt` says drivers live in userspace and the kernel holds only the
scheduler, memory manager, IPC, capabilities and interrupt routing. A line
discipline is none of those five, so putting one in the kernel is a genuine
departure from the stated architecture, and I am recording that rather than
glossing it.

Two things make it the right call anyway. First, the kernel *already contains
this exact code*: `kernel/src/tty.rs` is 1086 lines of canonical-mode editing,
`VMIN`/`VTIME` handling and `ISIG` signal generation for the physical console.
The choice is not "add a discipline to the kernel" but "have one discipline or
two", and two would diverge. Second, the pure alternative — a userspace pty
*service* that owns the pairs and answers over IPC — needs slave reads and
writes to be routed from an arbitrary process's fd out to that service, which is
a FUSE-like filesystem-server mechanism we do not have. That is a much larger
project than the pty, and building the pty first does not preclude it: if the
service mechanism ever lands, the pty is a natural early tenant, because the
object's interface (create pair, read, write, get/set termios, hang up) does not
change with its location.

### Consequence

Filed as `requests/b-a-pty-devices-need-the-line-discipline-that-the-console-already-has.md`.
The request deliberately asks for a *generalisation* of `tty.rs` from one
hardcoded device to N — four shallow couplings (three globals, the hardwired
`keyboard::read_char` input source, the hardwired echo sink, and a
console-specific foreground-pgrp lookup) — rather than for new code, because
`feed()`, `LineBuf`, the `VMIN`/`VTIME` matrix and the `ISIG` classifier are
already device-independent and already self-tested.

Nothing in `posix/src/pty.rs` needs to change when it lands: `openpty`,
`forkpty` and `login_tty` were written as the real glibc/musl algorithm over the
real primitives precisely so they would start working with no edit.
